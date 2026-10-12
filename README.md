# SPM Cloud

**A self-hosted Rust backend for [SparkleMorpher](https://github.com/sdf123098/Sparkle-Morpher).** It shares the Cloud client protocol and supports model uploads/downloads, visibility, verified game identities, player appearances, textures and wheel actions. Players connect by selecting the same Cloud instance in the mod.

> **English** | [中文](README_zh.md)

The backend runs independently of Minecraft. You can deploy it beside a Minecraft server or on a separate public server. Players need a current SparkleMorpher build and the HTTPS address of your instance.

## Embedded administration and content moderation

Visit `https://your-host/admin/`. The console is embedded in the Rust binary. The bootstrap owner signs in using the token referenced by `auth.bootstrap_access_token_file`, sets an owner password if desired, and creates/distributes administrator accounts. Ordinary self-registration grants no console access. Administrators manage all models and read configuration/activity; only the owner edits configuration or distributes accounts. Account disabling, password resets and role changes revoke existing sessions.

The console supports model upload, search/pagination, visibility, reversible deletion, re-scanning, manual content verdicts, and all JSON settings through forms or an advanced JSON editor. Configuration saves are validated and detect concurrent edits. Start with `--config PATH` and give the service write access to the configuration directory for saving. `web_ui.enabled=false` disables the console and its management API.

The panel layout follows 1Panel's navigation and compact panels. The model library switches between a compact grid and depth-styled cards with pointer tilt; the browser remembers the selection and respects reduced motion. All 48 original HarmonyOS Sans fonts (SC, TC, Latin, condensed, italic and Arabic variants) and pinned Lucide SVG icons are embedded for offline use. Fonts load on demand and use versioned URLs with a seven-day cache. Original fonts add about 75.4 MiB to the binary. Their licenses differ from the application's MIT license: see the console's `/admin/credits`, `web/credits.txt`, and the source asset manifests.

The content area scrolls independently while navigation stays in place. Compact model cards open a detail dialog for individual actions. Select models or all currently loaded results for batch visibility, re-scanning, marking, deletion and restoration; successful items are deselected and failures remain selected for retry. Search/refresh retains selection only for visible results, and the selection survives view switching. Batch marking shares the same content-hash scope as individual marking.

`moderation.detection_enabled` and `moderation.block_r18` are independent. Optional texture recognition uses a self-hosted [NudeNet](https://github.com/notAI-tech/NudeNet) `/infer` endpoint; blocked revisions are excluded from discovery, downloads, player/entity selection and exact resource authorization. `block_unclassified` additionally rejects unreadable, unscanned, disabled, over-budget or failed classifications. Recognition and blocking default to off for compatibility. Re-scan existing models after enabling recognition. Manual verdicts are content-addressed and survive automated re-scans.

For a native deployment (the source checkout paths below have no `tools/` prefix in prebuilt archives), create a Python virtual environment, install `tools/requirements-moderation.txt`, and run `tools/nudenet_service.py --port 8080`. Set `moderation.scanner_url` to `http://127.0.0.1:8080/infer`, enable both recognition flags, and choose `block_r18` separately. The supplied adapter reads PNG/JPEG/WebP/AVIF by content, retains no images, and uses NudeNet 320n. ZIP, bbmodel and embedded glTF/GLB textures are supported; encrypted `.ysm` requires an independently installed [YSMParser](https://github.com/OpenYSM/YSMParser) executable configured with `moderation.ysm_parser_path` (relative paths resolve from the JSON directory). Animated images and incomplete reads remain unclassified. Stylized textures, UV atlases and low-resolution images can produce false positives or misses; retain human review.

For Docker, add `docker-compose.moderation.yml` to the JSON Compose deployment and set the scanner URL to `http://nudenet:8080/infer`. The official scanner image's image-format support depends on its dependencies; use the supplied Python adapter when AVIF support is required. See the [Chinese walkthrough](README_zh.md) and [deployment guide](deploy/ADMIN_JSON.md) for commands and directory permissions.

## Choose an installation method

| Method | Best for | Initial installation | Updates |
|---|---|---|---|
| Docker Compose | Most Linux servers; managed data volumes and automatic restarts | Clone this repository and build the container once with Docker Compose; Rust is not needed on the host | Optional Linux systemd updater downloads checksum-verified, prebuilt Docker images for amd64/arm64 |
| Native Linux | Ubuntu/Debian servers that prefer a systemd service | Download the latest stable, prebuilt Linux installer; supports x86_64 and ARM64 | Optional systemd timer downloads a prebuilt package, verifies it, restarts the service and rolls back on failed health checks |
| Native Windows | Windows servers without Docker | Run the latest stable PowerShell installer; prebuilt x64 executable | Optional Task Scheduler job downloads the verified prebuilt package and rolls back if health checks fail |
| macOS package | Local or manual runs on Intel or Apple Silicon | Download the matching archive from [GitHub Releases](https://github.com/sdf123098/spm-cloud/releases/latest) | Manual replacement; the service updater tutorials below cover Linux and Windows |

All release links below follow GitHub's **latest stable release**; commands do not contain a version number. For a tagged/older release, select a specific version on the [Releases page](https://github.com/sdf123098/spm-cloud/releases). Default model upload size is **128 MiB** (134217728 bytes), and the reverse proxy must allow at least the same request size. New installations use JSON as the required backend configuration; environment-only startup remains available for existing deployments during migration. Prebuilt archives ship `config.example.json`, `config.schema.json`, and `nginx-cloud.conf.example`; initialize an instance-specific config and token on the target machine. See [JSON configuration and deployment](deploy/ADMIN_JSON.md).

## Before you install

| Requirement | What to prepare |
|---|---|
| Domain | For example, `cloud.example.com`, pointed at the server's public IP address |
| HTTPS | Allow inbound TCP 80 and 443; use Caddy or add a site to your existing reverse proxy |
| Backend port | Keep port 8787 private. The proxy connects to it over HTTP; players use the HTTPS origin only |
| Instance identity | Choose a stable instance ID and keep it, the data directory and the operator secret when upgrading |
| Client | Players install the matching SparkleMorpher version and select this same Cloud instance |

Give players the bare origin, for example `https://cloud.example.com`. Do not add `/v1`, `/health` or `:8787`.

## Docker Compose

The initial Compose deployment builds the container from the checked-out repository. Later automatic updates use prebuilt Release images, so the server does not rebuild the Rust backend for each update. Install Docker Engine and the Compose plugin using Docker's instructions for [Ubuntu](https://docs.docker.com/engine/install/ubuntu/) or [Debian](https://docs.docker.com/engine/install/debian/). Docker Desktop can run the same Compose setup in Linux container mode.

### 1. Download and configure

Run on the Docker host. The JSON Compose file is the default for new installations; the legacy `docker-compose.yml` remains available for existing ENV deployments.

```bash
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
mkdir -p deploy/runtime
docker compose -f docker-compose.json.yml build spm-cloud
sudo chown 10001:10001 deploy/runtime
sudo docker run --rm --user 10001:10001 \
  -v "$PWD/deploy/runtime:/etc/spm-cloud" spm-cloud:local \
  --init-config /etc/spm-cloud/config.json
sudo nano deploy/runtime/config.json
```

Edit `instance.instance_id`, `instance.origin`, `instance.bind`, `instance.trusted_proxy_ips`, and `storage` in the generated JSON. For this Compose network, use `0.0.0.0:8787`, the proxy address `172.30.10.3`, and persistent container paths `/var/lib/spm-cloud/data`, `/var/lib/spm-cloud/data/spm-cloud.db`, `/var/lib/spm-cloud/objects`. Set the public origin to your HTTPS domain. Also replace `cloud.example.com` in `Caddyfile.compose` with the same hostname. The generated bootstrap token is in `deploy/runtime/secrets/bootstrap-token.txt`; keep it private.

`limits.max_asset_bytes` defaults to 134217728 bytes (128 MiB). The example config, schema and Nginx proxy snippet are included in the image at `/usr/share/doc/spm-cloud/` and in the native prebuilt archives.

### 2. Start and verify

```bash
sudo docker compose -f docker-compose.json.yml config --quiet
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo docker compose -f docker-compose.json.yml up -d
sudo docker compose ps
sudo docker compose logs --tail=100 spm-cloud caddy
curl -fsS https://cloud.example.com/health
curl -fsS https://cloud.example.com/v1/instance
```

Confirm `/v1/instance` returns your instance ID and capabilities. Compose exposes HTTPS through Caddy; it does not publish port 8787. Runtime JSON settings hot-reload every two seconds; changes to bind address, instance identity, storage, or logging require recreating the backend container.

```bash
sudo docker compose -f docker-compose.json.yml up -d --force-recreate spm-cloud
```

If you customize `SPM_CLOUD_PROXY_IP` or `SPM_CLOUD_NETWORK_SUBNET` in Compose's `.env`, also update `instance.trusted_proxy_ips` in the JSON to match the proxy address.

### 3. Optional automatic updates

The updater checks GitHub Releases every 30 minutes and downloads the matching amd64/arm64 Docker archive. It verifies the SHA-256 checksum, loads the image and updates only the backend service. Data volumes and Caddy remain in place. For new JSON deployments, use the JSON-specific systemd units:

```bash
sudo apt install -y curl jq gzip coreutils util-linux
sudo cp deploy/spm-cloud-json-auto-update.service /etc/systemd/system/spm-cloud-auto-update.service
sudo cp deploy/spm-cloud-json-auto-update.timer /etc/systemd/system/spm-cloud-auto-update.timer
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-auto-update.timer
sudo systemctl start spm-cloud-auto-update.service
sudo systemctl list-timers spm-cloud-auto-update.timer
```

If the checkout is elsewhere, change `/opt/spm-cloud` in the service before enabling the timer. For manual updates, back up first, then run `git pull --ff-only` followed by `sudo SPM_CLOUD_COMPOSE_DIR="$PWD" SPM_CLOUD_COMPOSE_FILE=docker-compose.json.yml bash deploy/docker-auto-update.sh` from the repository directory. The updater applies the latest prebuilt image.

## Native Linux

This path uses a prebuilt release package and systemd. No Rust toolchain, Node.js, SQLite package or OpenSSL package is needed at runtime. The examples use Ubuntu/Debian and install the binary under `/usr/local/bin`.

### 1. Install the latest stable binary

```bash
sudo apt update
sudo apt install -y ca-certificates curl jq nano
curl --fail --location --proto '=https' --tlsv1.2 \
  https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
spm-cloud --version
```

The installer selects the matching x86_64 or ARM64 package. It does not create a user or configure HTTPS.

### 2. Create the service account and JSON configuration

Run user creation only on a first install. Keep the data and object directories persistent across upgrades:

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects /etc/spm-cloud
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
sudo -u spm-cloud nano /etc/spm-cloud/config.json
```

Set the public HTTPS origin and a stable instance ID. Use these storage paths so mutable data stays under `/var/lib`:

```json
{
"instance": {
  "instance_id": "my-server-cloud",
  "origin": "https://cloud.example.com",
  "bind": "127.0.0.1:8787",
  "trusted_proxy_ips": ["127.0.0.1"]
},
"storage": {
  "data_dir": "/var/lib/spm-cloud/data",
  "database_path": "/var/lib/spm-cloud/data/spm-cloud.db",
  "object_dir": "/var/lib/spm-cloud/objects"
}
}
```

The initialized bootstrap token is in `/etc/spm-cloud/secrets/bootstrap-token.txt`. Keep it private and preserve it across upgrades. The default upload limit is 128 MiB.

### 3. Install and start the systemd service

Fetch the service unit that matches the current stable release, then enable it:

```bash
release_tag="$(curl --fail --silent --show-error https://api.github.com/repos/sdf123098/spm-cloud/releases/latest | jq -er '.tag_name')"
deploy_url="https://raw.githubusercontent.com/sdf123098/spm-cloud/${release_tag}/deploy"
curl --fail --location "$deploy_url/spm-cloud-json.service" --output /tmp/spm-cloud-json.service
sudo install -m 644 /tmp/spm-cloud-json.service /etc/systemd/system/spm-cloud.service
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud
sudo systemctl status spm-cloud --no-pager
curl -fsS http://127.0.0.1:8787/health
```

Check the file before enabling the service with `sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --check-config`. Check logs with `sudo journalctl -u spm-cloud -n 100 --no-pager`. JSON runtime settings hot-reload; listener, storage and logging changes require `sudo systemctl restart spm-cloud`.

### 4. Configure HTTPS

Install Caddy using its [official instructions](https://caddyserver.com/docs/install). Add this site to `/etc/caddy/Caddyfile`, replacing the hostname and preserving any existing site blocks:

```caddyfile
cloud.example.com {
    reverse_proxy 127.0.0.1:8787 {
        header_up X-SPM-Client-IP {http.request.remote.host}
    }
}
```

Validate and reload Caddy:

```bash
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl enable --now caddy
sudo systemctl reload caddy
curl -fsS https://cloud.example.com/v1/instance
```

DNS must point to the server and ports 80/443 must be reachable for certificate issuance. Caddy handles HTTPS and WebSockets. The proxy header works with `instance.trusted_proxy_ips` set to `127.0.0.1` above.

If you use Nginx instead of Caddy, the prebuilt archive includes `nginx-cloud.conf.example`. Include its contents inside the HTTPS `server` block for this hostname. It sets `client_max_body_size 128m` and forwards HTTP/WebSocket requests to the backend. Keep the Nginx limit at least as high as `limits.max_asset_bytes`; otherwise Nginx will return 413 before the request reaches SPM Cloud. After editing Nginx, run `sudo nginx -t` and reload it.

### 5. Optional automatic updates

The systemd timer checks GitHub every 30 minutes. It downloads the matching prebuilt Linux archive, verifies its checksum and version, replaces the binary, restarts the service and checks `/health`. It restores the previous binary if the health check fails. Enable it after the backend is healthy:

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

Review update logs with `sudo journalctl -u spm-cloud-native-update.service`. Back up the database and objects before unattended updates; an application database migration may not be reversible by restoring only the executable.

## Native Windows

The PowerShell installer downloads the latest stable, prebuilt Windows x64 package. Run it as the Windows account that will own the backend process:

```powershell
$env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
```

Keep the executable under `$env:USERPROFILE\.cargo\bin` and mutable data in a separate directory, for example `C:\SPMCloud`. Initialize JSON and edit the HTTPS origin and instance ID:

```powershell
Set-Location C:\SPMCloud
$exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
$config = '.\config.json'
& $exe --init-config $config
# Edit config.json: set instance.origin, instance.instance_id and storage paths
& $exe --config $config --check-config
```

Save the first three commands as a one-time setup, then create `C:\SPMCloud\start-cloud.ps1` with these persistent startup commands:

```powershell
Set-Location C:\SPMCloud
& "$env:USERPROFILE\.cargo\bin\spm-cloud.exe" --config .\config.json
```

The bootstrap token is written to `C:\SPMCloud\secrets\bootstrap-token.txt`. Keep it private and preserve it across upgrades. Relative config and storage paths are resolved from the JSON file location. A `.env` file alone does not configure a native Windows process.

For HTTPS, download Caddy from [caddyserver.com](https://caddyserver.com/download). Put `caddy.exe` in `C:\SPMCloud`, create a `Caddyfile` with the same site block as the Linux section, then run the backend script and Caddy in separate terminals. Confirm `http://127.0.0.1:8787/health` locally and `https://cloud.example.com/v1/instance` through the proxy. Configure Task Scheduler to start both at boot for unattended operation.

### Optional automatic updates

Run the backend from a Task Scheduler task named `SPM Cloud` under the installer account. Download the latest updater script:

```powershell
$release = Invoke-RestMethod https://api.github.com/repos/sdf123098/spm-cloud/releases/latest
$url = "https://raw.githubusercontent.com/sdf123098/spm-cloud/$($release.tag_name)/deploy/windows-auto-update.ps1"
$updater = "$env:ProgramData\SPMCloud\windows-auto-update.ps1"
New-Item -ItemType Directory -Force (Split-Path $updater) | Out-Null
Invoke-WebRequest $url -OutFile $updater
```

Create a daily Task Scheduler job that runs `powershell.exe` with `-NoProfile -ExecutionPolicy Bypass -File "C:\ProgramData\SPMCloud\windows-auto-update.ps1"`, using the same account as the backend task. The updater verifies the package checksum and version, stops the backend task, replaces the executable, restarts it and checks `/health`; it restores the old executable if health does not recover. Stop an interactively running backend before manually invoking the updater.

## Configuration and JSON hot reload

**New installations use JSON as the required backend configuration.** The Rust executable does not read `.env` itself. Existing installations that started with environment variables can keep using that legacy mode during migration; once a JSON file is selected, its values are authoritative and environment variables do not override them.

`spm-cloud --init-config <path>` creates a starter file and a unique bootstrap token; edit instance-specific values such as HTTPS origin and storage paths. Prebuilt archives include `config.example.json`, `config.schema.json` and `nginx-cloud.conf.example`; the Docker image also contains these under `/usr/share/doc/spm-cloud/`. The token and paths are unique to each deployment, so initialize them on the target machine instead of sharing a generated file. JSON is checked every two seconds. Runtime settings such as upload/request limits, registration policy, identity providers and visual runtime settings hot-reload without restarting the process. Listener address, instance identity/origin, storage paths and logging settings require a restart. The default `limits.max_asset_bytes` is 128 MiB.

To accept larger models, raise the JSON upload limit and the reverse proxy's request-body limit together.

For a Linux JSON service, the native path is `/etc/spm-cloud/config.json`. Create the directory for the service account, then initialize, edit and validate with the installed binary:

```bash
sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
# Edit the JSON: set the HTTPS origin and absolute storage paths
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --print-effective-config
```

On Windows, use a Windows path and run as the backend account:

```powershell
$exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
$config = 'C:\SPMCloud\config.json'
New-Item -ItemType Directory -Force (Split-Path $config) | Out-Null
& $exe --init-config $config
# Edit the JSON: set the HTTPS origin and storage paths
& $exe --config $config --check-config
& $exe --config $config --print-effective-config
```

Initialization creates `secrets/bootstrap-token.txt` beside the config. Keep the config and token readable by the service account and private from players. `--print-effective-config` redacts credentials. Do not move a live instance to a new data path unless you also move its existing database and object files.

## Connect players and accounts

1. Each player updates SparkleMorpher and opens the model panel's **Instance Management** tab.
2. Select **Add Instance** and enter the HTTPS origin, such as `https://cloud.example.com`. The mod reads the instance ID from the server.
3. Select the new instance, open account management, then register or log in. Accounts are separate for every Cloud instance.
4. After login, use **Bind Current Game Identity** to verify the game account. A Cloud account session alone does not complete game identity binding.
5. Upload a model, mark it public, then test with another player on the same instance. Private models are not shared.

Self-registration is enabled by default. If disabled, the operator must create player accounts through `POST /v1/accounts`; see [API notes](#api-and-development).

## Optional external Yggdrasil login

For a new JSON installation, add the provider under `auth.identity_providers` in `config.json`. For example, a single HTTPS `hasJoined` endpoint can be represented as:

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

Place this array inside the existing `auth` object. The backend appends `username` and `serverId`; the service must return the verified Yggdrasil player ID and name. Alternatively, use `base_url` with `session_path` (default `/sessionserver/session/minecraft/hasJoined`). Keep `provider_id` stable when changing the service because it identifies existing account bindings. Legacy ENV deployments can continue using `SPM_CLOUD_HAS_JOINED_URL` or `SPM_CLOUD_IDENTITY_PROVIDERS` until migrated. See the [JSON deployment guide](deploy/ADMIN_JSON.md).

## Backup and upgrade

Back up configuration and the entire data and object storage before upgrading. For SQLite, stop the service before copying the database so its WAL is included consistently. Keep backups private because they contain credentials and player data.

| Deployment | Update path | Data to preserve |
|---|---|---|
| Docker Compose | Use the optional systemd updater, or pull repository changes and run `deploy/docker-auto-update.sh` to load the latest prebuilt image | Docker data/object volumes, `.env`, Caddy data/config volumes |
| Linux native | Use the systemd timer, or run the latest shell installer and restart `spm-cloud` | `/var/lib/spm-cloud`, `/etc/spm-cloud.env` or JSON config/secrets, systemd overrides and Caddy config |
| Windows native | Use the scheduled updater or install the latest PowerShell package after stopping the process | `data`, `objects`, startup script, Caddy config and any JSON/token files |

Never run `docker compose down -v` when you intend to keep volumes. Normal upgrades retain accounts and model files; database schema changes may require a newer binary. To roll back, restore a matching data/config backup together with the compatible binary. See [backup, migration and rollback details](deploy/ADMIN_JSON.md).

## Troubleshooting

| Symptom | Check |
|---|---|
| HTTPS or certificate failure | DNS points to this server; ports 80/443 are open; Caddy logs and configuration |
| Caddy returns 502 | Backend health; native upstream is `127.0.0.1:8787`, Docker upstream is `spm-cloud:8787` |
| Native Linux service exits | `sudo journalctl -u spm-cloud -n 100`; origin syntax, JSON validation, file ownership and storage permissions |
| Port already in use | Identify what uses 8787/80/443 before changing the listener or proxy |
| Player cannot log in or bind | Same Cloud selected; account session works; game identity binding completed; external provider matches the launcher |
| Other players cannot see a model/action | Both players use the same instance; identity is verified; model is public; client and backend are current |
| Upload rejected | Backend default is 128 MiB; check `limits.max_asset_bytes` and the proxy body-size cap |
| JSON edit has no effect | Process started with the intended `--config`; listener, identity, storage and logging fields need a restart |
| Legacy `.env` edit has no effect | Docker: recreate the container. Native: ensure the service/startup script loads the file, then restart the process |

One instance uses SQLite and a local object directory. Do not point multiple backend processes at the same SQLite file. Validate the deployment with two real players; `/health` only confirms reachability.

## API and development

The official service at `https://micaftic.top` and self-hosted instances use the same Rust backend with Axum, SQLite WAL and local object storage. Cloudflare proxies HTTPS to the official origin using Full (strict). The previous `micafic.xyz` domain reaches the same origin; the original `workers.dev` address uses a compatibility forwarding Worker. The previous D1/R2 implementation remains in `cloudflare/` for historical reference and migration tools.

Set `SPM_CLOUD_COMPAT_ORIGINS` to a comma-separated allowlist of HTTPS origins when maintaining old client endpoints. Direct requests match the Host header; forwarded requests may select an alias only through `X-SPM-Compat-Origin` from a configured trusted proxy IP. All origins share one database, object directory and realtime state. Alias discovery and game challenges retain the corresponding origin expected by old clients; browser account/admin pages should redirect to the canonical origin.

For an offline D1 migration, use `tools/migrate_cloudflare.py --help`. Stop writes and export a final SQL snapshot, verify the SHA-256 object files, build a separate output database from a stopped Rust database snapshot, then back up and promote the database and objects before reopening traffic. The importer preserves identifiers, sessions and ACLs and converts Worker PBKDF2 credentials to an explicit legacy format. A successful password login upgrades that credential to Argon2 in the same transaction as session creation. Keep the source export and storage as rollback backups until migration acceptance.

| Feature | Endpoint |
|---|---|
| Health and discovery | `GET /health`, `GET /v1/instance` |
| Create account / password login | `POST /v1/accounts`, `POST /v1/sessions` |
| Refresh / log out | `POST /v1/sessions/refresh`, `DELETE /v1/sessions/current` |
| Game login and identity binding | `/v1/auth/login-challenges` and `/v1/auth/challenges` |
| Providers and identities | `/v1/identity-providers`, `/v1/identities` |
| Model catalog and upload | `GET/POST /v1/assets` |
| Player appearance/actions | `PUT /v1/players/me/appearance`, `POST /v1/players/appearances/query` |

When self-registration is disabled, an operator can create an account using the bootstrap token. The example reads the Linux native token path; for Docker, use `deploy/runtime/secrets/bootstrap-token.txt` instead.

```bash
bootstrap_token="$(sudo cat /etc/spm-cloud/secrets/bootstrap-token.txt)"
curl -fsS https://cloud.example.com/v1/accounts \
  -H "Authorization: Bearer $bootstrap_token" \
  -H 'Content-Type: application/json' \
  --data '{"account_id":"alice","password":"replace-with-initial-password"}'
```

Rust development requires Rust 1.87 or newer. Tests additionally need Node.js 22+ for test fixtures; production runtime does not. Build and CI details are in [platform CI](.github/workflows/platforms.yml); Worker deployment is documented in [cloudflare/README.md](cloudflare/README.md).

**License:** MIT.

### Model ownership and self-service account connections

Uploads default to PUBLIC. Private console uploads require an explicit Cloud owner. Administrators can assign owners individually or in bulk; transferring ownership clears existing sharing grants without changing model bytes or revisions.

Users manage their own connections at `/account/`; `/admin/` remains restricted to owner-distributed administrators. Connecting accounts requires proof of both accounts and unifies verified official/Yggdrasil identities while preserving identity IDs. Model ownership and administrative roles are not migrated. Game login in the mod issues the Cloud account session; browser login issues the associated WebUI account session.

Configure a personal-account-compatible Microsoft Entra Web application with `web_ui.microsoft_client_id`, `microsoft_client_secret_file`, and `microsoft_redirect_uri`. Register `/admin/oauth/microsoft/callback` on your public HTTPS origin (localhost HTTP is allowed for local tests). Keep the secret file outside source control and release archives. OAuth uses PKCE, browser-bound expiring state, and a one-time redemption ticket; Xbox and Minecraft profile verification must complete before accepting a game identity.

Trusted HTTPS third-party providers support standard Yggdrasil and Ely authentication, role selection, and join/hasJoined verification. Manual verification accepts service-validated access tokens; names or UUIDs alone never prove ownership. Existing mod-side game identity bindings are shared with the browser account center. See the Chinese README for configuration details.

Microsoft browser login also requires Minecraft Services access for the application. Entra registration alone does not grant this access; new applications can apply through the official AppID Review form at https://aka.ms/mce-reviewappid. If Microsoft and Xbox authentication succeed but Minecraft application authentication returns HTTP 403, check application approval. Failed Minecraft verification never creates a verified game identity. Existing mod-side identity verification remains an independent option.
