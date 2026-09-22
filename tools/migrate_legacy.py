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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="legacy custom/auth/built root to inventory")
    parser.add_argument("-o", "--output", type=Path, required=True, help="JSON manifest output path")
    parser.add_argument("--tenant-id")
    parser.add_argument("--scope-id")
    parser.add_argument("--world-epoch")
    parser.add_argument("--max-file-bytes", type=int, default=DEFAULT_MAX_BYTES)
    args = parser.parse_args()
    if args.max_file_bytes <= 0:
        parser.error("--max-file-bytes must be positive")
    manifest = inventory(args.root, args.scope_id, args.world_epoch, args.tenant_id, args.max_file_bytes)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"inventoried={len(manifest['assets'])} skipped={len(manifest['skipped'])} output={args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
