#!/usr/bin/env python3
"""Build an offline Rust database from a D1 export, preserving IDs and ACLs.

The target service must be stopped for the final snapshot/cutover. This tool
never modifies the template database or source SQL and refuses occupied targets.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3

def migrate(source_sql, template, output, object_root, source_objects):
    if output.exists():
        raise ValueError('Output already exists')
    source = sqlite3.connect(':memory:')
    source.row_factory = sqlite3.Row
    source.executescript(source_sql.read_text(encoding='utf-8'))
    baseline = sqlite3.connect(f'file:{template.as_posix()}?mode=ro', uri=True)
    target = sqlite3.connect(output)
    baseline.backup(target)
    target.execute('PRAGMA foreign_keys=OFF')
    protected = ['account_credentials', 'identities', 'assets', 'scopes', 'targets']
    for table in protected:
        if target.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0]:
            raise ValueError(f'Template has existing user data: {table}')
    accounts = target.execute('SELECT account_id FROM accounts').fetchall()
    if any(row[0] != 'account_local' for row in accounts):
        raise ValueError('Template has additional accounts; explicit merge required')
    verified = {}
    for row in source.execute('SELECT raw_sha256,byte_length,object_key FROM asset_revisions'):
        digest, size, key = tuple(row)
        if not re.fullmatch(r'[0-9a-f]{64}', digest) or key != 'assets/' + digest:
            raise ValueError('Invalid asset storage mapping')
        path = source_objects / digest
        if digest not in verified:
            if not path.is_file() or path.stat().st_size != size or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                raise ValueError('Missing or corrupt object: ' + digest)
            verified[digest] = size
        elif verified[digest] != size:
            raise ValueError('Inconsistent object size')
    report = {'tables': {}, 'objects': len(verified), 'object_bytes': sum(verified.values()), 'ephemeral_challenges_skipped': 0}
    skip = {'_cf_KV', 'd1_migrations', 'sqlite_sequence', 'identity_challenges'}
    source_tables = [r[0] for r in source.execute("SELECT name FROM sqlite_master WHERE type='table'")]
    target.execute('BEGIN')
    for table in source_tables:
        if table in skip:
            if table == 'identity_challenges':
                report['ephemeral_challenges_skipped'] = source.execute('SELECT COUNT(*) FROM identity_challenges').fetchone()[0]
            continue
        columns = [r[1] for r in target.execute(f'PRAGMA table_info("{table}")')]
        if not columns:
            raise ValueError('Unsupported source table: ' + table)
        rows = source.execute(f'SELECT * FROM "{table}"').fetchall()
        if table == 'player_motion':
            # Worker stores NULL rows for withdrawn motion; Rust represents the
            # same absence with no row in its NOT NULL motion table.
            rows = [row for row in rows if row['motion_json'] is not None]
        for row in rows:
            data = dict(row)
            if table == 'accounts' and data['account_id'] == 'account_local' and accounts:
                continue
            if table == 'account_credentials':
                stored = data['password_hash']
                if not re.fullmatch(r'[0-9a-f]{32}:[0-9a-f]{64}', stored):
                    raise ValueError('Unsupported Worker password encoding')
                data['password_hash'] = 'spm-worker-pbkdf2-sha256-100000$' + stored
            if table == 'asset_revisions':
                digest = data['raw_sha256']
                data['object_path'] = str(object_root / digest[:2] / digest)
            canonical = data.pop('canonical_name', None) if table == 'identities' else None
            data.pop('object_key', None)
            unknown = set(data) - set(columns)
            if unknown:
                raise ValueError(f'Unsupported columns in {table}: {sorted(unknown)}')
            selected = list(data)
            names = ','.join('"' + name + '"' for name in selected)
            placeholders = ','.join('?' for _ in selected)
            if table == 'identity_providers':
                updates = ','.join('"' + name + '"=excluded."' + name + '"' for name in selected if name != 'provider_id')
                suffix = ' ON CONFLICT(provider_id) DO UPDATE SET ' + updates
            else:
                suffix = ''
            target.execute(f'INSERT INTO "{table}"({names}) VALUES({placeholders})' + suffix, [data[name] for name in selected])
            if canonical is not None:
                target.execute('INSERT INTO player_canonical_names(identity_id,name) VALUES(?,?)', [data['identity_id'], canonical])
        expected = len(rows)
        if table == 'accounts' and accounts and not any(row['account_id'] == 'account_local' for row in rows):
            expected += 1  # Preserve the new deployment's bootstrap owner.
        actual = target.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0]
        if actual != expected:
            raise ValueError(f'Row count mismatch: {table}: {actual} != {expected}')
        report['tables'][table] = actual
    problems = target.execute('PRAGMA foreign_key_check').fetchall()
    if problems:
        raise ValueError('Foreign key violations: ' + repr(problems[:10]))
    if target.execute('PRAGMA integrity_check').fetchone()[0] != 'ok':
        raise ValueError('SQLite integrity check failed')
    target.commit()
    target.close()
    baseline.close()
    source.close()
    return report

if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source-sql', type=Path, required=True)
    p.add_argument('--template-db', type=Path, required=True)
    p.add_argument('--output-db', type=Path, required=True)
    p.add_argument('--object-root', type=Path, required=True)
    p.add_argument('--source-objects', type=Path, required=True)
    args = p.parse_args()
    print(json.dumps(migrate(args.source_sql, args.template_db, args.output_db, args.object_root, args.source_objects), indent=2))
