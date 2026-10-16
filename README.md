# SPM Cloud: installation, use, and operations

SPM Cloud is a self-hosted service used by the SparkleMorpher mod. The server owner runs the service and manages content. Players add the same Cloud address in the mod to browse and use models, appearances, and actions they are allowed to access. The service includes HTTPS APIs, a real-time WebSocket, and an embedded administration website; no separate web frontend is required.

> [简体中文](README_zh.md) | **English**

## 1. Choose a deployment

### Docker Compose (recommended)

Use a Linux host with Docker Engine, the Compose plugin, a domain pointing to the host, and inbound TCP ports 80 and 443. The repository includes Caddy HTTPS proxy and persistent data-volume templates:

    git clone https://github.com/sdf123098/spm-cloud.git
    cd spm-cloud
    mkdir -p deploy/runtime
    docker compose -f docker-compose.json.yml build spm-cloud
    sudo chown 10001:10001 deploy/runtime
    sudo docker run --rm --user 10001:10001 \
      -v "$PWD/deploy/runtime:/etc/spm-cloud" spm-cloud:local \
      --init-config /etc/spm-cloud/config.json

Initialization creates deploy/runtime/config.json and a separate secrets/bootstrap-token.txt. Set the public HTTPS origin and a unique instance ID in the JSON file. Replace the example domain in Caddyfile.compose with the same domain, then start and inspect the services:

    sudo docker compose -f docker-compose.json.yml up -d
    sudo docker compose -f docker-compose.json.yml ps
    sudo docker compose -f docker-compose.json.yml logs -f spm-cloud

Players use the HTTPS root address, such as https://cloud.example.com. Do not expose backend port 8787 directly to the internet. Do not run docker compose down -v for routine upgrades; it deletes named volumes.

### Native Linux

On a systemd host, download the shell installer from the latest GitHub Release and install to /usr/local:

    curl --fail --location --proto '=https' --tlsv1.2 \
      https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
      --output /tmp/spm-cloud-installer.sh
    sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
    sudo useradd --system --home /var/lib/spm-cloud --create-home --shell /usr/sbin/nologin spm-cloud
    sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
    sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud
    sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json

Initialization creates configuration and token files under /etc/spm-cloud. Set storage.data_dir, storage.database_path, and storage.object_dir in config.json to persistent paths under /var/lib/spm-cloud. Review deploy/spm-cloud-json.service, then install and start it:

    sudo cp deploy/spm-cloud-json.service /etc/systemd/system/
    sudo systemctl daemon-reload
    sudo systemctl enable --now spm-cloud-json.service
    sudo systemctl status spm-cloud-json.service
    sudo journalctl -u spm-cloud-json.service -f

The HTTPS reverse proxy should forward to 127.0.0.1:8787.

### Native Windows

In PowerShell, use the official installer to place the program in the current user's Cargo bin directory. Initialize the service in a separate data directory:

    $env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
    irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
    $exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
    New-Item -ItemType Directory -Force C:\SPMCloud | Out-Null
    Set-Location C:\SPMCloud
    & $exe --init-config C:\SPMCloud\config.json

Set the public HTTPS origin, instance ID, database, and object paths, then start:

    & $exe --config C:\SPMCloud\config.json

For production, configure a Windows service or another managed startup method and place an HTTPS reverse proxy in front of Cloud. Restrict access to the token, config, database, and model objects to the service account and owner.

## 2. Sign in and manage account access

The initial access token is the owner's first and highest-privilege credential for /admin/. It is stored in a file on the server, is not sent to the browser, and is not a Minecraft password. By default, find it at secrets/bootstrap-token.txt beside the config file. If auth.bootstrap_access_token_file is customized, use that path.

1. Read the token file on the server and copy its complete single line. Use sudo cat on Linux or Get-Content in PowerShell.
2. Open https://your-domain/admin/ and paste the token into the sign-in form.
3. Under “Accounts & permissions,” set a password for the owner account.
4. Use the owner account ID and password for later sign-ins. Keep the initial token protected for recovery.

Never put the token in source control, screenshots, chat, command-line arguments, or public logs. If it is exposed, rotate it from the admin console. If you cannot sign in, stop the service and restore trusted credential backups.

The owner creates administrator accounts and assigns their access. Admins can manage models and view settings/audit records, but cannot create admins or change server configuration. Player self-registration follows the owner's setting. Disabling an account, changing its role, or resetting credentials invalidates affected sessions.

Players add the Cloud root address in a compatible version of the mod, select the instance, then register or sign in. At /account/, players can manage links for accounts they own; they should not give game passwords to the server owner. Microsoft web sign-in requires the owner's Microsoft application and applicable Minecraft service approval. Until configured, use in-mod account linking or an enabled third-party Yggdrasil provider.

## 3. Use the WebUI

- The “Model library” supports search, server-side pages, grid and 3D cards, multi-select, and bulk actions. “All models” shows the accessible catalog; the owner is pinned first in the account grid.
- Open a model name for details. Select models to change visibility, rescan, manually label, delete, or restore. Ordinary deletion is reversible; permanent cleanup is not. Back up before permanent cleanup.
- Model cards show the original file size with B/KiB/MiB units. Minecraft previews face the front and support BBModel, Bedrock ZIP/YSM, and GLTF/GLB files with embedded resources. Encrypted YSM previews require YSMParser. GLTF/GLB previews do not access external network resources.
- Settings, identity links, and audit records support search. Model search includes name, asset ID, owner, and format. Pages avoid loading the entire catalog at once.
- Open “Server settings” near the bottom of the sidebar, above the current account and sign-out button. Update and audit tools are inside these settings; “My account & links” remains in the main navigation.

Uploads are public by default. A private model belongs to one Cloud account and is accessible only to its owner. Choose the correct account during upload. File selection happens on the operator's computer even when Cloud is remote; only selected files are uploaded.

### Server settings

| Tab | What you can configure or do |
|---|---|
| General & network | Public address, instance identity, listener, trusted proxies, and WebUI availability. |
| Accounts & authentication | Registration, Microsoft sign-in, third-party services, initial owner credentials, and actual owner password changes. |
| Models & content safety | Upload size, detection, blocking, scanner connection, texture budgets, and YSMParser. |
| Performance & cache | Performance presets, custom concurrency, memory/Redis/disabled caching, and all cache limits. |
| Storage & database | Data paths, SQLite/MySQL connection settings, and database-switch guidance. |
| Client features | Supported capabilities, protocol limits, and temporary-state cleanup. Unimplemented projectile features remain unavailable. |
| Logging | Log level, per-module filter rows, and text/JSON output. |
| Updates & backups | Owner-only update checks/installation, automatic-update settings, backups, and library/audit/runtime-log exports. |
| Audit records | Search, page through, inspect, and export filtered records as an owner or admin, preserving existing permissions. |

Only the owner can edit server configuration, and saving requires a writable configuration file selected with `--config`. Admins can read settings without receiving secret values; environment-only configuration is read-only. Search settings by name or technical alias to open the relevant tab. Use the arrow keys, Home, and End to navigate the tab bar.

Changes across tabs share one draft. Switching tabs, searching, viewing audit records, or checking for updates keeps that draft. Validate before saving, restore individual defaults, or undo all changes. “Reload” discards the draft after confirmation. A failed save preserves it. On a revision conflict (HTTP 409), load the new server values for comparison, choose to keep your changes with the new revision, then validate and save again. Reloading or leaving the page may discard drafts; sign-out clears them. Secret inputs stay only in browser memory. The optional configuration import/export tool edits the same draft and excludes secret values.

The page distinguishes saved values, values actually in use, and settings awaiting restart. Listener, instance identity/public address, storage, and logging changes require restart. A save with no relevant change does not create a restart requirement. Initial password hashes apply only to initial account creation. Install updates after saving or discarding drafts; backups and exports use the active configuration and exclude drafts.

### Add a third-party authentication service

In “Accounts & authentication,” choose “Add third-party service.” For a standard Yggdrasil service, enter its display name and public HTTPS base address. A custom compatible service exposes separate session-verification and account-login path fields; gateway mode accepts the complete session-verification URL and an optional login path. Use the form rather than writing provider JSON.

“Check service” tests the supported address/connectivity rules without saving or linking a player. “Reachable” does not establish that a player's credentials work; verify login/linking separately. Add the service to the draft and save. New services receive a stable internal ID automatically. Editing its display name keeps that ID and existing links. Disable a service to stop authentication while retaining links, then restore it from the disabled-services list when needed. Removing a configuration-managed service also soft-disables it. Legacy database entries can be brought under configuration management; the built-in `official` provider is protected.

### Credentials, database switching, and WebUI recovery

MySQL and Redis forms accept host, port, username, password, and database separately; Redis also offers TLS. Microsoft secrets and initial owner credentials have their own controls. Choose “Keep,” “Replace,” or explicit “Clear”; existing server-side credential files remain an option. Replacing a secret writes a new server-side credential file, and saved secrets cannot be read back. Generating an initial token displays its value once after saving; copy and protect it immediately. Changing the real owner password uses “Change owner password,” revokes existing password sessions, and requires signing in again. Replacing the initial password-hash reference does not change an existing account password.

To switch from SQLite to MySQL, check the target connection, download and verify a backup, preserve the existing model directory and SQLite path, save the MySQL settings, then restart. On startup, the supported import requires a valid SQLite source with accounts and a MySQL target with no accounts or models. Connection checks do not create tables or migrate data. Non-empty targets are not overwritten or merged, and MySQL-to-SQLite migration is not automatic. Changing a directory setting does not move files. Redis settings apply live; an unavailable Redis service falls back to bounded memory.

Before disabling the WebUI under “General & network,” copy the instance-specific recovery command shown by the confirmation dialog. Run it on the server with the installed executable, for example:

    spm-cloud --config /path/to/config.json --enable-web-ui

The running file watcher reopens the website and management API; otherwise the next service startup applies it. This command changes WebUI availability without initializing or migrating a database. It requires a valid writable configuration file.

### Content detection and blocking

Configure “Detection” and “Blocking” independently under “Models & content safety”:

- Detection reads supported model metadata and can use NudeNet to inspect readable textures. It labels content R18, clean, or unclassified.
- Blocking hides detected R18 models from ordinary player listings, downloads, and use.

Both are disabled by default. Detection creates labels but does not block content. Blocking has no detection result to act on unless detection is enabled. Rescan existing models after enabling detection. To start NudeNet with this repository's Docker deployment:

    docker compose -f docker-compose.json.yml \
      -f docker-compose.moderation.yml up -d

Then set the scanner URL to http://nudenet:8080/infer. Encrypted .ysm files also need YSMParser. Detection can miss or misclassify content; provide a manual review path. Whether to block unclassified files is a separate setting.

## 4. Configuration, paths, and secrets

Use JSON for new deployments. Create the starter config and token:

    spm-cloud --init-config /path/to/config.json

Validate without starting the service:

    spm-cloud --config /path/to/config.json --check-config

Print redacted effective settings and their sources:

    spm-cloud --config /path/to/config.json --print-effective-config

When a JSON file is selected, it is authoritative and environment variables do not override its values. SPM_CLOUD_* variables are read only by the legacy environment-only startup mode. Relative paths in JSON resolve from the config file's directory; relative paths in legacy mode resolve from the process working directory. See config.example.json and config.schema.json for the editable template and complete field types.

| Setting | Purpose and default |
|---|---|
| instance.instance_id / origin / bind | Stable instance ID, public HTTPS root, and listen address. Changing the ID, origin, or listener requires a restart. |
| storage.data_dir / database_path / object_dir | Database and model object locations. SQLite is stored under the data directory by default. Back up the database and objects together. |
| auth.bootstrap_account_id | Owner account ID. |
| auth.bootstrap_access_token_file | Server-side initial token file. Do not put the token itself in JSON. |
| auth.bootstrap_password_hash_file | Optional Argon2id password-hash file; never put a plaintext password here. |
| auth.allow_self_registration | Allow or refuse player self-registration. |
| auth.identity_providers | Optional Yggdrasil providers. Use has_joined_url, or base_url plus session_path. Optional authenticate_path selects a relative login endpoint; omitting it preserves legacy inference. Keep provider IDs stable. To disable an existing provider, keep its record and set enabled=false. |
| limits.max_asset_bytes / max_message_bytes | Per-model and ordinary protocol message limits. Defaults are 128 MiB and 64 KiB. The reverse proxy upload limit must be large enough too. |
| performance.preset | Low: 2 uploads / 1 model processor; medium: 4 / 2 (default); high: 8 / 4; or custom upload and scan/decode concurrency. Each custom value may be 0 for unlimited; 0 / 0 leaves both unlimited. Lowering a limit does not interrupt existing work. |
| updates.auto_update / check_interval_hours | Automatic updates are disabled by default; checks default to every 24 hours. Container images are updated by the deployment platform. |
| cache.backend | memory (default), redis, or none. If Redis is unavailable, the service falls back to bounded local memory. |
| cache.ttl_seconds / max_pages / timeout_ms | Expiry, maximum cached pages, and Redis response timeout. |
| moderation.* | Detection, blocking, scanner URL, thresholds, decode budgets, and YSMParser path. |
| features.* / retention.* | Optional game capability switches and state retention. Enable only capabilities supported by deployed clients. |
| logging.level / format | Log filter and text/JSON format. A restart is required. |
| web_ui.enabled | Whether to expose the admin website and management API. Recover with --config PATH --enable-web-ui. |

The process checks a selected JSON file every two seconds. Valid runtime options apply in place; invalid edits leave the last valid settings active. Listen address, instance identity, public origin, storage paths, and logging settings require a restart. Bootstrap tokens can be replaced live. The bootstrap password hash initializes new accounts only; changing its file does not overwrite an existing password. Saving through WebUI requires startup with --config PATH and write permission to the config directory. Do not reinitialize an existing instance: initialization does not overwrite files or reset passwords.

Legacy ENV files are not loaded automatically by the Rust service. To migrate, stop the service, back up data and credentials, create JSON, map the existing database/object/token paths, then run --check-config. SPM_CLOUD_CONFIG only selects a JSON file. Historical environment names and their JSON equivalents are covered in the configuration reference above and in .env.example.

Recover a disabled WebUI without opening a database, migrating data, or resetting accounts:

    spm-cloud --config /path/to/config.json --enable-web-ui

The running file watcher restores access, or the next startup applies the change. Environment-only deployments remain read-only.

The settings API distinguishes saved `config`, actual `active` settings, and `pending_restart_fields`. Owners can validate drafts with `/admin/api/config/validate`, check fixed MySQL/Redis/NudeNet connections with `/admin/api/config/check-connection`, and probe candidate authentication services with `/admin/api/providers/check`. A successful probe does not verify an account login or create a binding. Administrators may read configuration but cannot save settings or run these checks.

`PUT /admin/api/config` retains `{config, revision}` and accepts optional `managed_secrets` entries (mysql, redis, microsoft, bootstrap_token, bootstrap_password_hash): `keep`, `replace`, or explicit `clear`. Bootstrap tokens also support `generate`; the generated value appears only in that successful save response. Structured MySQL/Redis connection inputs create distinct server-side files in `.managed-secrets/` beside the configuration. Passwords and complete connection strings are never read back. Existing credential files remain supported and old files are retained for rollback. Backups exclude configuration and credential files; back these up securely and separately. Actual owner password changes continue through the account update endpoint and revoke old sessions.

Removing a previously configuration-managed provider soft-disables it while retaining bindings. Legacy database providers are preserved on the first save. `provider_admin_entries` includes sources, disabled entries, and endpoint metadata; `official` is protected. Database switching still supports only existing SQLite data imported into empty MySQL. Connection checks neither create business tables nor migrate data.

## 5. Optional MySQL storage and Redis cache

SQLite and in-memory caching remain the defaults; a small single-host deployment needs no external service.

### MySQL

Create a dedicated database and least-privilege service account. Put the connection URL in a file readable only by the service account, then configure:

    {
      "storage": {
        "backend": "mysql",
        "mysql_url_file": "secrets/mysql-url.txt"
      }
    }

The URL has the form mysql://user:password@host:3306/database. Keep credentials out of source control and shared logs. Restrict remote database access and enable supported transport encryption.

On first startup, if storage.database_path is a valid SQLite database and the MySQL target has no accounts or models, the service imports the old database tables. Keep object_dir pointed at the existing model files so stored object paths still work. Existing non-empty MySQL data is not merged automatically. Keep a stopped-service backup before migration; afterward, check accounts, permissions, model downloads, and a WebUI backup before archiving the old database. MySQL backups export as SQLite files. tools/backup_restore.py is for stopped-service SQLite file backups.

The current MySQL adapter uses one synchronous connection and serializes SQL calls. Load-test your expected traffic before using it for a high-concurrency deployment. MySQL is an option for external persistence; switching away from SQLite is not required for the default installation.

### Redis

Redis caches short-lived, rebuildable catalog pages. It does not store accounts, permissions, audit history, or model files. Put its credentials in a protected file and configure:

    {
      "cache": {
        "backend": "redis",
        "redis_url_file": "secrets/redis-url.txt",
        "ttl_seconds": 30,
        "max_pages": 256,
        "timeout_ms": 300
      }
    }

Legacy ENV-only installations may use SPM_CLOUD_REDIS_URL. The cache bounds page count and TTL. If Redis is unavailable, the service falls back to bounded memory. Cache hits still validate current permissions against the persistent store. Set backend to none to disable caching.

## 6. Back up, restore, and update

### WebUI exports

Under “Server settings → Updates & backups,” the owner can download a consistent backup ZIP containing the database and immutable model revisions, a ZIP of the full library in original formats, the current process's most recent 2 MiB of logs, and all or filtered audit records. “Audit records” also offers filtered export to owners and admins, preserving its existing permission. Full backups, library archives, runtime-log exports, and updates remain owner-only. Single-model export preserves its original bytes. Config, credentials, proxy certificates, and container environment files are not included; protect them separately. Get logs from systemd, Docker, or your deployment platform for periods before the current process started.

Extract a WebUI backup ZIP before restoring it:

    python tools/backup_restore.py restore \
      --backup /path/to/extracted-backup \
      --database /path/to/restore/spm-cloud.db \
      --objects /path/to/restore/objects

Stop the service before creating an offline backup so the database and object directory are from the same point in time:

    python tools/backup_restore.py backup \
      --database DATA/spm-cloud.db \
      --objects OBJECTS \
      --destination NEW_BACKUP

The tool checks SHA-256, object sizes, path overlap, and symbolic links. Restore refuses to overwrite existing destinations unless --force is supplied. Keep the service stopped until both database and object data are restored. Start it, verify login, permissions, and model downloads, and keep the original backup until checks pass.

### Automatic updates and rollback

Back up config, token, database, model objects, and Docker named volumes before an update. The WebUI can check stable releases. Native updaters verify SHA-256 and the binary's reported version, wait for active uploads and model processing to finish, create a local backup, replace the program, and restart. Automatic updates are disabled by default. The install directory must be writable; Unix systems need tar with xz support.

Update Docker images through the deployment platform. The repository includes an optional Linux systemd timer for Docker Compose updates on amd64/arm64. It requires Docker Compose, curl, jq, gzip, sha256sum, and flock. If the checkout is not under /opt/spm-cloud, update the Compose path in the unit. Run the updater service manually once and inspect its logs before enabling the timer. On failure, the script attempts to restore the previous image.

The Compose updater assumes the repository and Compose .env are under /opt/spm-cloud. Install its units there:

    sudo apt install -y curl jq gzip coreutils util-linux
    sudo cp deploy/spm-cloud-json-auto-update.service /etc/systemd/system/spm-cloud-auto-update.service
    sudo cp deploy/spm-cloud-json-auto-update.timer /etc/systemd/system/spm-cloud-auto-update.timer
    sudo systemctl daemon-reload
    sudo systemctl enable --now spm-cloud-auto-update.timer
    sudo systemctl start spm-cloud-auto-update.service
    sudo systemctl status spm-cloud-auto-update.timer

If paths differ, edit SPM_CLOUD_COMPOSE_DIR and the script path in the service. The timer applies the latest release on its first run too; check the service logs and health before leaving it enabled.

The native Linux updater downloads the matching prebuilt release archive and checksum. If the restarted service fails its health check, it restores the previous executable. Before unattended use, confirm the service name, binary path, health URL, and updater logs in journalctl. Restoring an old executable does not reverse database migrations; if the old binary cannot read the new schema, restore the database backup made before upgrading as well.

Example installation for the native updater; the source checkout can be anywhere:

    sudo install -d -m 755 /usr/local/libexec
    release=$(curl --fail --silent --show-error https://api.github.com/repos/sdf123098/spm-cloud/releases/latest | jq -er '.tag_name')
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

Defaults are /usr/local/bin/spm-cloud, spm-cloud.service, and http://127.0.0.1:8787/health. Override them in /etc/spm-cloud-auto-update.env with SPM_CLOUD_BINARY, SPM_CLOUD_SYSTEMD_SERVICE, or SPM_CLOUD_AUTO_UPDATE_HEALTH_URL. Review journalctl -u spm-cloud-native-update.service after the first run.

## 7. Troubleshooting

| Symptom | Checks |
|---|---|
| Blank token field or unauthenticated | The browser form cannot read a remote file. Read the token on the server and paste the full line. Check the actual config path. |
| Lost owner password | Sign in with the initial token and reset it. If both credentials are lost, stop the service and restore protected credential backups; do not edit the account database manually. |
| WebUI cannot save | Confirm startup uses --config PATH, the JSON is valid, and the service account can write to the config directory. Admin accounts cannot write server config. |
| HTTPS, certificate, or 502 failure | Check DNS, inbound ports 80/443, firewall, proxy upstream, and certificate renewal. Backend port 8787 should only be reachable by the local proxy. |
| Upload fails | Check service/proxy logs, model size, proxy body limit, and disk space. The default per-model limit is 128 MiB. |
| Redis is unavailable | Check the URL file permissions, network, and Redis health. The service falls back to local memory; the persistent store remains authoritative. |
| MySQL will not connect or migrate | Check database name, account permissions, TLS, and URL escaping. First import requires a valid SQLite source and an empty MySQL target. Restore a backup before retrying. |
| Content detection fails | Check detection and blocking separately, scanner reachability, YSMParser, and model-processing budgets. Rescan existing models. |
| Data issue after update | Stop the updater and service. Keep logs and old files. Restore the pre-upgrade consistent database/object backup, then start with a compatible program. |

## 8. API, capabilities, and client compatibility

Players and mods connect to the HTTPS root. Instance discovery and the model catalog are under /v1; player self-service is /account/; administration is /admin/; health is /health. The shared contract is Protobuf spm.cloud.v1. The real-time endpoint is /v1/realtime over WebSocket. Ordinary messages default to a 64 KiB limit; model files use HTTP upload/download and never travel over WebSocket. See docs/PROTOCOL_V1.md for the protocol contract.

Model revisions retain original bytes, SHA-256, length, and a controlled object path. Downloads support ETags and one byte range. Appearance writes use expected_revision to detect concurrent changes. Access checks account, scope, target, and model visibility. Private and blocked models are excluded from regular catalogs.

Player display state, vehicle bindings, and entity motion timelines have separate features switches. Enable only capabilities supported by the clients you deploy. Projectile snapshots remain reserved and are not a ready client feature. For capability conditions, permissions, expiry, and wire messages, see PROTOCOL_V1.md and config.schema.json.

## 9. Build, test, and contribute

Build the Rust service with Rust 1.88 or newer. The preview bundle is checked in, so ordinary Cargo builds do not need Node.js. To change web/preview.js, install Node.js 22 and rebuild the bundle:

    cd web
    npm ci
    npm run build
    cd ..
    cargo build --locked

Run the normal checks:

    cargo fmt --all -- --check
    cargo check --all-targets --all-features --locked
    cargo test --all-targets --all-features --locked
    cargo clippy --all-targets --all-features --locked -- -D warnings
    node --test web/preview.test.cjs

GitHub Release automation builds cross-platform archives, installers, checksums, and Docker release packages from version tags. To verify the complete HTTP service contract locally, build the release binary and run tools/service_contract.py with a test model file. Do not run destructive tests against a production instance.

Offline authentication fixtures are in fixtures/profile-proofs.json. They contain only synthetic public keys, a certificate, and public signed payloads, with no private keys or game credentials. Tests pass the fixture directory to pure verification functions. The production service always uses the repository's fixed Mojang trust roots and official HTTPS public-key endpoint; no environment variable or request field can replace production trust anchors.

## 10. License and project references

The project is licensed under MIT. Protocol details are in docs/PROTOCOL_V1.md. Editable config and its field constraints are in config.example.json and config.schema.json. Compose and reverse-proxy templates are in the repository root. Source code, issue tracking, and official build downloads are available from the GitHub project and Releases pages.
