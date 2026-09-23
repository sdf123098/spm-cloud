#!/usr/bin/env python3
"""Create a reviewable inventory for legacy SPM model assets.

The command is intentionally inventory-only: it never deletes, moves, uploads,
or grants Cloud permissions. The resulting manifest is the safe hand-off to a
future authenticated upload/import step.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tempfile
from pathlib import Path
from typing import Iterable

SUPPORTED_EXTENSIONS = {".ysm", ".zip", ".bbmodel"}
DEFAULT_MAX_BYTES = 512 * 1024 * 1024


def iter_files(root: Path) -> Iterable[Path]:
    for current, directories, filenames in os.walk(root, followlinks=False):
        current_path = Path(current)
        directories[:] = sorted(name for name in directories if not (current_path / name).is_symlink())
        for name in sorted(filenames):
            path = current_path / name
            if path.is_symlink() or path.suffix.lower() not in SUPPORTED_EXTENSIONS:
                continue
            yield path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inventory(root: Path, scope_id: str | None, world_epoch: str | None, tenant_id: str | None, max_bytes: int) -> dict:
    root = root.resolve()
    if not root.is_dir():
        raise ValueError(f"legacy root is not a directory: {root}")
    assets: list[dict] = []
    skipped: list[dict] = []
    for path in iter_files(root):
        relative = path.relative_to(root).as_posix()
        try:
            size = path.stat().st_size
            if size > max_bytes:
                skipped.append({"path": relative, "reason": "file_too_large", "byte_length": size})
                continue
            digest = sha256_file(path)
        except OSError as error:
            skipped.append({"path": relative, "reason": "read_failed", "detail": str(error)})
            continue
        assets.append({
            "legacy_path": relative,
            "asset_key": f"legacy-{digest[:24]}",
            "format": path.suffix.lower().lstrip("."),
            "byte_length": size,
            "raw_sha256": digest,
            "review_status": "PENDING_REVIEW",
            "source_deleted": False,
        })
    return {
        "schema": "spm.cloud.legacy-inventory.v1",
        "migration_mode": "INVENTORY_ONLY",
        "source_root_name": root.name,
        "tenant_id": tenant_id,
        "scope_id": scope_id,
        "world_epoch": world_epoch,
        "assets": assets,
        "skipped": skipped,
        "notes": [
            "This manifest does not prove ownership or grant Cloud ACL.",
            "Review identity, target kind, scope and world epoch before import.",
            "Original files remain untouched; upload and deletion are separate operations.",
        ],
    }


def _asset_destination(destination_root: Path, asset: dict) -> Path:
    legacy_name = Path(asset["legacy_path"]).name
    return destination_root / asset["asset_key"] / legacy_name


def plan_import(manifest: dict, source_root: Path, destination_root: Path) -> dict:
    """Build a deterministic, non-destructive import plan from an inventory."""
    source_root = source_root.resolve()
    destination_root = destination_root.resolve()
    actions: list[dict] = []
    summary = {"copy": 0, "skip_identical": 0, "conflict": 0, "missing": 0}
    for asset in manifest.get("assets", []):
        relative = Path(asset["legacy_path"])
        source = (source_root / relative).resolve()
        if source_root not in source.parents and source != source_root:
            raise ValueError(f"legacy path escapes source root: {relative}")
        target = _asset_destination(destination_root, asset)
        action = "COPY"
        if not source.is_file():
            action = "MISSING"
        elif target.exists():
            action = "SKIP_IDENTICAL" if sha256_file(target) == asset["raw_sha256"] else "CONFLICT"
        summary[action.lower()] += 1
        actions.append({
            "asset_key": asset["asset_key"],
            "legacy_path": asset["legacy_path"],
            "source": str(source),
            "target": str(target),
            "raw_sha256": asset["raw_sha256"],
            "action": action,
        })
    return {
        "schema": "spm.cloud.legacy-import-plan.v1",
        "manifest_schema": manifest.get("schema"),
        "source_root": str(source_root),
        "destination_root": str(destination_root),
        "summary": summary,
        "actions": actions,
    }


def apply_import(plan: dict, source_root: Path, destination_root: Path, backup_root: Path) -> dict:
    """Apply only COPY actions atomically; conflicts and missing files remain untouched."""
    del source_root, destination_root
    backup_root = backup_root.resolve()
    backup_root.mkdir(parents=True, exist_ok=True)
    journal = {
        "schema": "spm.cloud.legacy-import-journal.v1",
        "plan_schema": plan.get("schema"),
        "imported": [],
        "backups": [],
        "conflicts": [action for action in plan["actions"] if action["action"] == "CONFLICT"],
        "missing": [action for action in plan["actions"] if action["action"] == "MISSING"],
    }
    for action in plan["actions"]:
        if action["action"] != "COPY":
            continue
        source = Path(action["source"])
        target = Path(action["target"])
        target.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix=".spm-import-", dir=target.parent, delete=False) as temporary:
            temporary_path = Path(temporary.name)
        try:
            shutil.copyfile(source, temporary_path)
            if sha256_file(temporary_path) != action["raw_sha256"]:
                raise ValueError(f"source changed during import: {source}")
            os.replace(temporary_path, target)
        finally:
            temporary_path.unlink(missing_ok=True)
        journal["imported"].append({"target": str(target), "raw_sha256": action["raw_sha256"]})
    journal_path = backup_root / "migration-journal.json"
    journal["journal_path"] = str(journal_path)
    journal_path.write_text(json.dumps(journal, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return journal


def rollback_import(journal: dict | Path) -> None:
    """Remove only files recorded by the journal and still matching their imported hash."""
    if isinstance(journal, Path):
        journal = json.loads(journal.read_text(encoding="utf-8"))
    for entry in journal.get("imported", []):
        target = Path(entry["target"])
        if target.is_file() and sha256_file(target) == entry["raw_sha256"]:
            target.unlink()
            try:
                target.parent.rmdir()
            except OSError:
                pass


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, nargs="?", help="legacy custom/auth/built root to inventory")
    parser.add_argument("-o", "--output", type=Path, help="JSON manifest output path")
    parser.add_argument("--import-manifest", type=Path, help="existing inventory manifest to plan/apply")
    parser.add_argument("--destination", type=Path, help="staged Cloud import destination")
    parser.add_argument("--backup-dir", type=Path, default=Path("migration-backup"))
    parser.add_argument("--apply-import", action="store_true", help="apply COPY actions after showing the plan")
    parser.add_argument("--rollback-journal", type=Path, help="rollback a previous import journal")
    parser.add_argument("--tenant-id")
    parser.add_argument("--scope-id")
    parser.add_argument("--world-epoch")
    parser.add_argument("--max-file-bytes", type=int, default=DEFAULT_MAX_BYTES)
    args = parser.parse_args()
    if args.rollback_journal:
        rollback_import(args.rollback_journal)
        print(f"rolled_back={args.rollback_journal}")
        return 0
    if args.import_manifest:
        if not args.root or not args.destination:
            parser.error("--import-manifest requires root and --destination")
        manifest = json.loads(args.import_manifest.read_text(encoding="utf-8"))
        plan = plan_import(manifest, args.root, args.destination)
        if args.apply_import:
            journal = apply_import(plan, args.root, args.destination, args.backup_dir)
            print(f"imported={len(journal['imported'])} conflicts={len(journal['conflicts'])} journal={journal['journal_path']}")
        else:
            print(json.dumps(plan, ensure_ascii=False, indent=2))
        return 0
    if not args.root or not args.output:
        parser.error("inventory mode requires root and --output")
    if args.max_file_bytes <= 0:
        parser.error("--max-file-bytes must be positive")
    manifest = inventory(args.root, args.scope_id, args.world_epoch, args.tenant_id, args.max_file_bytes)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"inventoried={len(manifest['assets'])} skipped={len(manifest['skipped'])} output={args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
