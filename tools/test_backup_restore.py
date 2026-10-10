"""Local stopped-service backup integrity tests; no production data is touched."""
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest

from backup_restore import backup, restore


class BackupRestoreTest(unittest.TestCase):
    def setUp(self):
        fixture_root = Path(__file__).resolve().parents[1] / "target/test-backups"
        fixture_root.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="backup-fixture-", dir=fixture_root)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.database = self.root / "source.db"
        self.objects = self.root / "objects"
        self.objects.mkdir()
        (self.objects / "model.bin").write_bytes(b"original-model\x00\xff")
        with closing(sqlite3.connect(self.database)) as connection, connection:
            connection.execute("CREATE TABLE assets(id TEXT PRIMARY KEY, revision INTEGER)")
            connection.execute("INSERT INTO assets VALUES ('model', 7)")
        self.saved = self.root / "backup"

    def create_backup(self):
        return backup(self.database, self.objects, self.saved)

    def test_round_trip_preserves_database_and_object_bytes(self):
        manifest = self.create_backup()
        self.assertEqual(manifest["objects"]["entries"][0]["path"], "model.bin")
        restored_db, restored_objects = self.root / "restored.db", self.root / "restored-objects"
        restore(self.saved, restored_db, restored_objects, False)
        with closing(sqlite3.connect(restored_db)) as connection, connection:
            self.assertEqual(connection.execute("SELECT * FROM assets").fetchall(), [("model", 7)])
            self.assertEqual(connection.execute("PRAGMA integrity_check").fetchone(), ("ok",))
        self.assertEqual((restored_objects / "model.bin").read_bytes(), (self.objects / "model.bin").read_bytes())

    def test_corrupt_object_is_rejected_before_existing_destination_changes(self):
        self.create_backup()
        (self.saved / "objects/model.bin").write_bytes(b"tampered")
        target = self.root / "existing.db"
        target.write_bytes(b"keep")
        with self.assertRaisesRegex(ValueError, "contents"):
            restore(self.saved, target, self.root / "target-objects", True)
        self.assertEqual(target.read_bytes(), b"keep")

    def test_missing_objects_and_corrupt_database_are_rejected(self):
        self.create_backup()
        (self.saved / "objects/model.bin").unlink()
        with self.assertRaisesRegex(ValueError, "count"):
            restore(self.saved, self.root / "restored.db", self.root / "restored-objects", False)
        (self.saved / "spm-cloud.db").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "database SHA"):
            restore(self.saved, self.root / "restored.db", self.root / "restored-objects", False)

    def test_existing_destination_requires_force_and_backup_is_never_merged(self):
        self.create_backup()
        target = self.root / "existing.db"
        target.write_bytes(b"keep")
        with self.assertRaises(FileExistsError):
            restore(self.saved, target, self.root / "target-objects", False)
        self.assertEqual(target.read_bytes(), b"keep")
        with self.assertRaises(FileExistsError):
            self.create_backup()

    def test_unsafe_overlap_is_rejected_and_missing_source_does_not_create_database(self):
        self.create_backup()
        with self.assertRaises(ValueError):
            restore(self.saved, self.root / "target-objects/db.sqlite", self.root / "target-objects", True)
        with self.assertRaises(ValueError):
            restore(self.saved, self.saved / "restored.db", self.root / "target-objects", True)
        missing = self.root / "missing.db"
        with self.assertRaises(ValueError):
            backup(missing, self.objects, self.root / "second")
        self.assertFalse(missing.exists())

    def test_empty_object_directory_and_legacy_manifest_remain_readable(self):
        (self.objects / "model.bin").unlink()
        self.create_backup()
        manifest_file = self.saved / "manifest.json"
        manifest = json.loads(manifest_file.read_text())
        del manifest["objects"]["entries"]
        manifest_file.write_text(json.dumps(manifest))
        target = self.root / "target-objects"
        restore(self.saved, self.root / "restored.db", target, False)
        self.assertTrue(target.is_dir())
        self.assertEqual(list(target.iterdir()), [])

    def test_force_restores_complete_object_set_and_rejects_wrong_destination_type_first(self):
        self.create_backup()
        target_db, target_objects = self.root / "restored.db", self.root / "restored-objects"
        target_db.write_bytes(b"keep")
        target_objects.write_bytes(b"not-a-directory")
        with self.assertRaisesRegex(ValueError, "types"):
            restore(self.saved, target_db, target_objects, True)
        self.assertEqual(target_db.read_bytes(), b"keep")
        target_objects.unlink()
        target_objects.mkdir()
        (target_objects / "obsolete.bin").write_bytes(b"obsolete")
        restore(self.saved, target_db, target_objects, True)
        self.assertEqual(sorted(path.name for path in target_objects.iterdir()), ["model.bin"])


if __name__ == "__main__":
    unittest.main()
