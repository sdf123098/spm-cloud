#!/usr/bin/env python3
"""Portable, explicit backup/restore for the SQLite database and object CAS.

The service must be stopped by the operator before either operation. Restore
never overwrites an existing destination unless --force is supplied.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sqlite3
from pathlib import Path


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def copy_tree(source: Path, destination: Path) -> int:
    if not source.is_dir():
        return 0
    count = 0
    for path in sorted(source.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(source)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        count += 1
    return count


def object_inventory(root: Path) -> list[dict]:
    if root.is_symlink() or not root.is_dir():
        raise ValueError("object directory is missing or is a symbolic link")
    entries = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError("object directory contains a symbolic link")
        if path.is_file():
            entries.append({"path": path.relative_to(root).as_posix(),
                            "bytes": path.stat().st_size, "sha256": digest(path)})
    return entries


def validate_destinations(database: Path, objects: Path) -> None:
    if database.is_symlink() or objects.is_symlink():
        raise ValueError("restore destinations must not be symbolic links")
    resolved_database, resolved_objects = database.resolve(), objects.resolve()
    if resolved_objects == Path(resolved_objects.anchor) or resolved_database.is_relative_to(resolved_objects):
        raise ValueError("object destination is a filesystem root or contains the database")


def backup(database: Path, objects: Path, destination: Path) -> dict:
    if database.is_symlink() or not database.is_file():
        raise ValueError("database is missing or is a symbolic link")
    validate_destinations(database, objects)
    entries = object_inventory(objects)
    if destination.is_symlink() or destination.resolve().is_relative_to(objects.resolve()):
        raise ValueError("backup destination must be outside the object directory")
    if destination.exists() and (not destination.is_dir() or any(destination.iterdir())):
        raise FileExistsError("backup destination is not empty")
    destination.mkdir(parents=True, exist_ok=True)
    db_backup = destination / "spm-cloud.db"
    source = sqlite3.connect(database)
    target = sqlite3.connect(db_backup)
    try:
        source.backup(target)
        target.commit()
    finally:
        target.close()
        source.close()
    object_backup = destination / "objects"
    object_backup.mkdir()
    files = copy_tree(objects, object_backup)
    manifest = {
        "schema": "spm.cloud.backup.v1",
        "database": {"path": str(db_backup), "sha256": digest(db_backup)},
        "objects": {"path": str(object_backup), "file_count": files, "entries": entries},
        "operator_note": "Service was expected to be stopped during backup.",
    }
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return manifest


def restore(backup_root: Path, database: Path, objects: Path, force: bool) -> None:
    manifest_path = backup_root / "manifest.json"
    db_source = backup_root / "spm-cloud.db"
    object_source = backup_root / "objects"
    if not manifest_path.is_file() or not db_source.is_file():
        raise ValueError("backup is missing manifest.json or spm-cloud.db")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "spm.cloud.backup.v1":
        raise ValueError("unsupported backup manifest schema")
    expected = manifest["database"]["sha256"]
    if digest(db_source) != expected:
        raise ValueError("backup database SHA-256 does not match manifest")
    entries = object_inventory(object_source)
    if len(entries) != manifest["objects"]["file_count"]:
        raise ValueError("backup object count does not match manifest")
    if "entries" in manifest["objects"] and entries != manifest["objects"]["entries"]:
        raise ValueError("backup object contents do not match manifest")
    validate_destinations(database, objects)
    if database.exists() and not database.is_file() or objects.exists() and not objects.is_dir():
        raise ValueError("restore destination types do not match database/object storage")
    for target in (database.resolve(), objects.resolve()):
        if target.is_relative_to(backup_root.resolve()) or backup_root.resolve().is_relative_to(target):
            raise ValueError("restore destinations must not overlap the backup")
    if not force and (database.exists() or objects.exists()):
        raise FileExistsError("restore destination exists; pass --force after stopping the service")
    database.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(db_source, database)
    if objects.exists():
        shutil.rmtree(objects)
    objects.mkdir(parents=True, exist_ok=True)
    copy_tree(object_source, objects)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    backup_parser = subparsers.add_parser("backup")
    backup_parser.add_argument("--database", type=Path, required=True)
    backup_parser.add_argument("--objects", type=Path, required=True)
    backup_parser.add_argument("--destination", type=Path, required=True)
    restore_parser = subparsers.add_parser("restore")
    restore_parser.add_argument("--backup", type=Path, required=True)
    restore_parser.add_argument("--database", type=Path, required=True)
    restore_parser.add_argument("--objects", type=Path, required=True)
    restore_parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    if args.command == "backup":
        print(json.dumps(backup(args.database, args.objects, args.destination), indent=2))
    else:
        restore(args.backup, args.database, args.objects, args.force)
        print(f"restored={args.backup}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
