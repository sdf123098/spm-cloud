# Administrator JSON deployment

GitHub Releases are built by `cargo-dist`: they include checksummed Linux x86_64/ARM64, Windows x64 and macOS binaries plus shell/PowerShell installers. The release pipeline also builds checksummed amd64/arm64 Docker image archives. It runs Rust validation and publishes the Release only after package builds succeed.

When started with a selected JSON file, the binary polls it every two seconds. Valid changes to runtime settings apply in the existing process; invalid edits are logged and the last active settings remain in effect. Runtime settings include upload/message/entity limits, registration policy, bootstrap access token, identity-provider records, trusted proxies, feature switches and visual retention/publication settings. In particular, `limits.max_asset_bytes` defaults to 134217728 bytes (128 MiB) and changes apply without restarting `spm-cloud`. Explicit environment values continue to override JSON. Listener address, instance ID/origin, storage paths, bootstrap account/password initialization and logging settings require a service restart. Use `--check-config` to validate edits; `--print-effective-config` shows merged public settings and provenance without the bootstrap token or password hash.

## Native systemd

Initialize `/etc/spm-cloud/config.json` using `spm-cloud --init-config /etc/spm-cloud/config.json`. This creates a persistent `secrets/bootstrap-token.txt`; initialization refuses to overwrite either file. Restrict the directory to the service account and administrators. Set the public HTTPS origin and use these absolute storage paths so the read-only configuration directory contains no mutable database:

```json
"storage": {
  "data_dir": "/var/lib/spm-cloud/data",
  "database_path": "/var/lib/spm-cloud/data/spm-cloud.db",
  "object_dir": "/var/lib/spm-cloud/objects"
}
```

Preserve the paths of an existing database and object directory instead of creating a new empty instance. `deploy/spm-cloud-json.service` uses the explicit JSON path and optional `/etc/spm-cloud/overrides.env`. The existing `spm-cloud.service` keeps environment-only compatibility. Check file ownership under the service account before installing either unit.

## Docker Compose

Initialize `deploy/runtime/config.json` and its `secrets` directory with the native binary. Edit origin, instance ID, storage paths above and proxy settings. The container runs as UID 10001; both configuration and secret file must be readable by that UID. Mount them read-only. The runtime directory is ignored by Git.

```text
docker compose -f docker-compose.json.yml config
docker compose -f docker-compose.json.yml run --rm spm-cloud --config /etc/spm-cloud/config.json --check-config
docker compose -f docker-compose.json.yml up -d --build
```

The Compose template preserves the existing data/object named volumes and Caddy setup. It bind-mounts the whole runtime directory, so replacing `config.json` atomically from an editor remains visible to the running container. Container bind/data/object environment values intentionally override JSON for these mounts; the effective report records those overrides. Set `SPM_CLOUD_CONFIG_DIR` and `SPM_CLOUD_SECRETS_DIR` to other host paths when needed. The original `docker-compose.yml` retains its environment deployment.

### Automatic GitHub release updates (Docker Compose)

The optional updater checks the latest public GitHub Release every 30 minutes, downloads the prebuilt Docker archive for the host architecture, verifies SHA-256, loads the image, and updates only the `spm-cloud` Compose service. It does not compile the source on the server and leaves data volumes and Caddy running. It requires Linux systemd, Docker Compose, `curl`, `jq`, `gzip`, `sha256sum`, `flock` and a host architecture of amd64 or arm64. The supplied unit files assume the checkout and Compose `.env` are in `/opt/spm-cloud`:

```bash
sudo apt install -y curl jq gzip coreutils util-linux
sudo cp deploy/spm-cloud-auto-update.service deploy/spm-cloud-auto-update.timer /etc/systemd/system/
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
