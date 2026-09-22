import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from migrate_legacy import inventory


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


if __name__ == "__main__":
    unittest.main()
