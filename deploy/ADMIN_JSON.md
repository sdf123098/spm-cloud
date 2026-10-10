# Administrator JSON deployment

GitHub Releases are built by `cargo-dist`: platform archives include checksummed Linux x86_64/ARM64, Windows x64 and macOS binaries, shell/PowerShell installers, `config.example.json`, `config.schema.json`, and the bilingual README files. The Docker image also ships the JSON example and schema under `/usr/share/doc/spm-cloud/`. The release pipeline builds checksummed amd64/arm64 Docker image archives and publishes only after platform packages pass CI.

**New installations must use JSON.** Environment-only startup remains temporarily supported for existing deployments. If a JSON file is selected, it is authoritative and environment variables cannot silently override its values. `spm-cloud --init-config <path>` creates a starter config and unique bootstrap token, so operators do not need to author the file from scratch. Tokens, public origins and storage paths are deployment-specific; initialize each instance separately.

When started with a selected JSON file, the binary polls it every two seconds. Valid changes to runtime settings apply in the existing process; invalid edits are logged and the last active settings remain in effect. Runtime settings include upload/message/entity limits, registration policy, bootstrap access token, identity-provider records, trusted proxies, feature switches and visual retention/publication settings. In particular, `limits.max_asset_bytes` defaults to 134217728 bytes (128 MiB) and changes apply without restarting `spm-cloud`. Listener address, instance ID/origin, storage paths, bootstrap account/password initialization and logging settings require a service restart. Use `--check-config` to validate edits; `--print-effective-config` shows effective settings and their sources without exposing credential contents.

## Paths and portability

The binary does not hard-code an installation directory. A relative `--config` argument (or `SPM_CLOUD_CONFIG` in legacy mode) is resolved from the process working directory. Within JSON, relative database, object and credential paths are resolved from the selected JSON file's parent directory. Absolute paths are also accepted when a deployment intentionally uses standard OS locations such as `/var/lib` or `C:\SPMCloud`. The JSON example uses relative paths and can be moved with its data directory. Docker/systemd examples select explicit mounted/system paths in their service definitions; adjust those paths when installing elsewhere.

## Legacy ENV migration map

All backend settings in the existing `.env.example` and `.env.native.example` have JSON equivalents. Credential values move to protected files referenced by JSON rather than being embedded as plain text in a dotenv file.

| Legacy setting | JSON field |
|---|---|
| `SPM_CLOUD_INSTANCE_ID` | `instance.instance_id` |
| `SPM_CLOUD_ORIGIN` | `instance.origin` |
| `SPM_CLOUD_BIND` | `instance.bind` |
| `SPM_CLOUD_TRUSTED_PROXY_IPS` | `instance.trusted_proxy_ips` |
| `SPM_CLOUD_DATA_DIR` | `storage.data_dir` |
| `SPM_CLOUD_DATABASE` | `storage.database_path` |
| `SPM_CLOUD_OBJECT_DIR` | `storage.object_dir` |
| `SPM_CLOUD_ACCESS_TOKEN` | `auth.bootstrap_access_token_file` (store the same token in the referenced file) |
| `SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` | `auth.bootstrap_password_hash_file` |
| `SPM_CLOUD_BOOTSTRAP_ACCOUNT` | `auth.bootstrap_account_id` |
| `SPM_CLOUD_ALLOW_SELF_REGISTRATION` | `auth.allow_self_registration` |
| `SPM_CLOUD_HAS_JOINED_URL` | One entry in `auth.identity_providers` using `has_joined_url` |
| `SPM_CLOUD_IDENTITY_PROVIDERS` | `auth.identity_providers` |
| `SPM_CLOUD_MAX_ASSET_BYTES` | `limits.max_asset_bytes` |
| `SPM_CLOUD_MAX_MESSAGE_BYTES` | `limits.max_message_bytes` |
| `RUST_LOG` | `logging.level` |

`logging.level` accepts the same tracing `EnvFilter` syntax as `RUST_LOG`, including module-level directives such as `spm_cloud=debug,tower_http=info`.

`SPM_CLOUD_CONFIG` only selects a JSON file; it is not an application setting. `SPM_CLOUD_HOST`, Compose volume/network/image variables and updater variables configure Caddy, Docker or the updater process, so they remain deployment orchestration options rather than backend JSON fields.

For example, replace the empty `auth.identity_providers` array with a custom Yggdrasil endpoint like this (keep the endpoint's real HTTPS hostname and the provider ID stable):

```json
"identity_providers": [
  {
    "provider_id": "my-yggdrasil",
    "display_name": "My Yggdrasil",
    "has_joined_url": "https://auth.example.com/all-in-one/hasJoined",
    "enabled": true
  }
]
```

Put the array inside the `auth` object in `config.json`. `has_joined_url` is the complete HTTPS endpoint; the backend appends `username` and `serverId`. The alternative is `base_url` plus `session_path`.

### Migrating an existing ENV deployment

Stop the old process before switching configuration. Initialize a JSON file, preserve the existing database and object paths as absolute paths or equivalent paths relative to the JSON file, and put the existing bootstrap token in a protected token file referenced by `auth.bootstrap_access_token_file`. Map the current instance ID, origin, listener, registration choice, proxy list, limits, account ID, provider IDs, logging filter and password hash into their JSON fields. Run `--check-config`, then install the JSON systemd unit or start with `--config PATH`. Do not run `--init-config` over an existing config/token; it intentionally refuses to overwrite either file. Keep the old ENV service unit available until the JSON instance starts against the same data.

## Native systemd

After installing the binary and creating the `spm-cloud` service account, initialize the config as that account. This creates `/etc/spm-cloud/secrets/bootstrap-token.txt`; initialization refuses to overwrite either file. Keep the config directory writable during initialization and readable by the service account. Edit the generated JSON to set the public HTTPS origin and use these absolute storage paths so the read-only configuration directory contains no mutable database:

```bash
sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
```

```json
{
"storage": {
  "data_dir": "/var/lib/spm-cloud/data",
  "database_path": "/var/lib/spm-cloud/data/spm-cloud.db",
  "object_dir": "/var/lib/spm-cloud/objects"
}
}
```

Preserve the paths of an existing database and object directory instead of creating a new empty instance. `deploy/spm-cloud-json.service` selects `/etc/spm-cloud/config.json`; this is an example systemd path, not a binary restriction. The existing `spm-cloud.service` keeps environment-only compatibility. Check file ownership under the service account before installing either unit.

After editing the JSON, validate it and inspect the merged, redacted settings before enabling the service:

```bash
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --print-effective-config
```

## Docker Compose

Build the image once, then use its binary to initialize `deploy/runtime/config.json` and the token file. Edit the origin, instance ID, storage paths and proxy address. The container runs as UID 10001; the config and token must be readable by that UID. Mount the runtime directory read-only. The runtime directory is ignored by Git.

```bash
mkdir -p deploy/runtime
docker compose -f docker-compose.json.yml build spm-cloud
sudo chown 10001:10001 deploy/runtime
sudo docker run --rm --user 10001:10001 \
  -v "$PWD/deploy/runtime:/etc/spm-cloud" spm-cloud:local \
  --init-config /etc/spm-cloud/config.json
# Edit deploy/runtime/config.json, then validate and start:
docker compose -f docker-compose.json.yml config
docker compose -f docker-compose.json.yml run --rm spm-cloud --config /etc/spm-cloud/config.json --check-config
docker compose -f docker-compose.json.yml up -d
```

The Compose template preserves the named data/object volumes and Caddy setup. It bind-mounts the whole runtime directory, so replacing `config.json` atomically remains visible. Set `SPM_CLOUD_CONFIG_DIR` to use a different host directory. The original `docker-compose.yml` retains environment-only compatibility.

### Automatic GitHub release updates (Docker Compose)

The optional updater checks the latest public GitHub Release every 30 minutes, downloads the prebuilt Docker archive for the host architecture, verifies SHA-256, loads the image, and updates only the `spm-cloud` Compose service. It does not compile the source on the server and leaves data volumes and Caddy running. It requires Linux systemd, Docker Compose, `curl`, `jq`, `gzip`, `sha256sum`, `flock` and a host architecture of amd64 or arm64. The supplied unit files assume the checkout and Compose `.env` are in `/opt/spm-cloud`:

```bash
sudo apt install -y curl jq gzip coreutils util-linux
sudo cp deploy/spm-cloud-json-auto-update.service /etc/systemd/system/spm-cloud-auto-update.service
sudo cp deploy/spm-cloud-json-auto-update.timer /etc/systemd/system/spm-cloud-auto-update.timer
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-auto-update.timer
sudo systemctl start spm-cloud-auto-update.service
sudo systemctl status spm-cloud-auto-update.timer
```

Edit `SPM_CLOUD_COMPOSE_DIR` and both `/opt/spm-cloud` paths in the service if the checkout lives elsewhere. The timer applies the latest published release on its first run too. If container startup or the reported version check fails, it attempts to restore the previously running image. GitHub creates a Release from a matching `v*` tag after Rust checks and platform package builds pass.

### Automatic native Linux updates

Install the generated Linux package with the shell installer, forcing the install prefix to `/usr/local` so the backend lands in `/usr/local/bin`:

```bash
curl --fail --location --proto '=https' --tlsv1.2 \
  https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
```

After the `spm-cloud.service` backend is running, install the systemd updater. It downloads the matching prebuilt Linux archive and checksum, validates both the hash and embedded version, stages the binary on the same filesystem, restarts the service, then checks its local health endpoint. A failed health check restores the previous binary. Install `curl`, `jq`, `tar`, `xz-utils`, `coreutils` and `util-linux` if missing.

```bash
sudo install -d -m 755 /usr/local/libexec
release="$(curl --fail --silent --show-error https://api.github.com/repos/sdf123098/spm-cloud/releases/latest | jq -er '.tag_name')"
base="https://raw.githubusercontent.com/sdf123098/spm-cloud/${release}/deploy"
for file in native-auto-update.sh spm-cloud-native-update.service spm-cloud-native-update.timer; do
  curl --fail --location "$base/$file" --output "/tmp/$file"
done
sudo install -m 755 /tmp/native-auto-update.sh /usr/local/libexec/spm-cloud-native-update.sh
sudo install -m 644 /tmp/spm-cloud-native-update.service /tmp/spm-cloud-native-update.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-native-update.timer
sudo systemctl start spm-cloud-native-update.service
sudo systemctl list-timers spm-cloud-native-update.timer
```

The updater defaults to `/usr/local/bin/spm-cloud`, `spm-cloud.service`, and `http://127.0.0.1:8787/health`. Override them in `/etc/spm-cloud-auto-update.env` with `SPM_CLOUD_BINARY`, `SPM_CLOUD_SYSTEMD_SERVICE` or `SPM_CLOUD_AUTO_UPDATE_HEALTH_URL`. Review `journalctl -u spm-cloud-native-update.service` after the first run. Keep database and object backups before enabling unattended updates; database migrations may not be reversible by restoring only the executable.

## Visual capabilities and compatibility

`features.player_display_state` is opt-in and controls `player_display_state_v1`. It shares the verified player's appearance CAS, requires explicit scope membership and current epoch/dimension, and expires with the player's 60-second publication. Limits `max_visual_state_bytes`, `max_visual_variables` and visual publication rate/burst apply to those inputs and appear in instance discovery. Old clients omitting display input clear prior state. Model privacy also clears the state.

`features.vehicle_bindings` is opt-in and controls `vehicle_appearance_v1`. A binding requires an existing VEHICLE target, both scope and target edit ACL, exact resource access and revision CAS. It persists until explicitly unbound or the world epoch is rotated; query acknowledgements last at most 60 seconds and recheck publisher and reader permissions. Riding or observing never creates editing rights. Send explicit `resource:null` and empty variables to unbind.

`features.entity_motion` is opt-in and advertises `entity_motion_v1` for explicitly bound MAID/FAKE_PLAYER display timelines. Publications require both scope and target edit ACL, exact binding/appearance revisions and resource access; CAS, stable event clocks, 60-second expiry and publisher withdrawal apply. Client integration compiles on all six branches; real game acceptance remains pending. `projectile_snapshots` remains unavailable until its native source integration passes real game acceptance. The projectile contract is under development and is not advertised as a usable capability. Rust now runs visual cleanup at `retention.cleanup_interval_seconds`: expired/withdrawn appearance payloads are removed, compact replay identifiers persist within the current world epoch, and epoch rotation removes them. Worker has an equivalent scheduled handler; configure a Cron Trigger before enabling cleanup in deployment. Existing player motion limits are unchanged. New clients negotiate the entity query batch limit; old clients use 64, so keep 64 for deployments serving those clients.

## Migration and rollback

Stop writes and back up the database together with objects, configuration and credential files before upgrading. Preserve instance ID/origin, provider IDs, account credentials and ACLs. Removing a provider from JSON does not delete its database row; use an explicit disabled entry to revoke it. Bootstrap password initialization never resets an existing user's password.

New display/projectile tables are additive. To roll back, stop the new binary and restore the consistent backup with its previous binary/configuration. Do not drop old users/assets/identities or remove model objects as part of JSON conversion. A health response or successful configuration check does not establish game interoperability or deployment acceptance.

For a stopped local service, `python tools/backup_restore.py backup --database DATA/spm-cloud.db --objects OBJECTS --destination NEW_BACKUP` records database and per-object SHA-256/size. Use a fresh destination. Restore with `python tools/backup_restore.py restore --backup NEW_BACKUP --database RESTORED/spm-cloud.db --objects RESTORED/objects`; existing destinations require explicit `--force`. Missing/corrupt objects, database hash failures, destination type mismatches and overlapping paths are rejected before restoration writes. Legacy v1 manifests without per-object hashes remain readable but only offer the original count/database integrity checks; create a new backup for complete object checks. These checks do not make database-plus-directory replacement an atomic transaction; keep the service stopped and preserve the backup until startup and resource tests pass.

Seven isolated backup/restore tests pass on Windows, including intact model bytes, corruption refusal, empty storage, force replacement and existing destination preservation. This is local tool evidence, not a Linux production upgrade/rollback run. Exact revision downloads also expose `X-Asset-Format`; an older revision retains its own parser format when later uploads change format.

`python tools/service_contract.py --binary target/x86_64-pc-windows-gnu/release/spm-cloud.exe --model ../TestModel/tenna.ysm` additionally runs the actual release on loopback, stops it, backs up and restores SQLite plus objects into fresh directories, and restarts with JSON. The Windows run passed 84 HTTP requests and preserved instance identity, passwords, active sessions, PRIVATE denial, PUBLIC access, scope/target appearance, providers and exact model bytes. This is same-binary local data recovery; an older binary and Linux/systemd/Compose rollback have not been validated.

These templates have been prepared on Windows. Native systemd and a Linux container have not been executed locally.
