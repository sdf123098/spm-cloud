import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from migrate_legacy import apply_import, inventory, plan_import, rollback_import


class LegacyInventoryTest(unittest.TestCase):
    def test_inventory_is_sha_audited_and_non_destructive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = root / "custom" / "hero.ysm"
            model.parent.mkdir()
            payload = b"legacy-model"
            model.write_bytes(payload)
            (root / "notes.txt").write_text("ignored", encoding="utf-8")

            manifest = inventory(root, "scope-a", "epoch-1", "tenant-a", 1024)

            self.assertEqual(manifest["schema"], "spm.cloud.legacy-inventory.v1")
            self.assertEqual(len(manifest["assets"]), 1)
            self.assertEqual(manifest["assets"][0]["legacy_path"], "custom/hero.ysm")
            self.assertEqual(manifest["assets"][0]["raw_sha256"], hashlib.sha256(payload).hexdigest())
            self.assertTrue(model.exists())

    def test_large_file_is_skipped_with_reason(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = root / "large.bbmodel"
            model.write_bytes(b"12345")
            manifest = inventory(root, None, None, None, 4)
            self.assertEqual(manifest["assets"], [])
            self.assertEqual(manifest["skipped"][0]["reason"], "file_too_large")

    def test_import_is_idempotent_and_rollback_restores_backup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "legacy" / "hero.ysm"
            destination = root / "cloud-cache"
            backup = root / "backup"
            source.parent.mkdir()
            source.write_bytes(b"new-model")
            manifest = inventory(root / "legacy", "scope-a", "epoch-1", "tenant-a", 1024)

            plan = plan_import(manifest, root / "legacy", destination)
            self.assertEqual(plan["summary"]["copy"], 1)
            journal = apply_import(plan, root / "legacy", destination, backup)
            imported = destination / ("legacy-" + hashlib.sha256(b"new-model").hexdigest()[:24]) / "hero.ysm"
            self.assertTrue(imported.exists())

            retry = plan_import(manifest, root / "legacy", destination)
            self.assertEqual(retry["summary"]["skip_identical"], 1)
            source.write_bytes(b"replacement")
            replacement_manifest = inventory(root / "legacy", "scope-a", "epoch-1", "tenant-a", 1024)
            replacement_manifest["assets"][0]["asset_key"] = manifest["assets"][0]["asset_key"]
            conflict = plan_import(replacement_manifest, root / "legacy", destination)
            self.assertEqual(conflict["summary"]["conflict"], 1)
            rollback_import(journal)
            self.assertFalse(imported.exists())


if __name__ == "__main__":
    unittest.main()
