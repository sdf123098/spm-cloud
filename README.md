# SPM Cloud

**Self-hosted Rust backend.** [Latest stable release and prebuilt downloads](https://github.com/sdf123098/spm-cloud/releases/latest) provide Linux, Windows and macOS packages, plus amd64/arm64 Docker images. Install commands below always follow the latest stable release; no version number needs editing. JSON runtime settings reload without restarting the service, and the default per-model upload limit is 128 MiB. See [JSON deployment, migration and automatic updates](deploy/ADMIN_JSON.md).

> **English** | [中文](README_zh.md)

A standalone Rust Cloud backend for [Sparkle's Morpher](https://github.com/sdf123098/Sparkle-Morpher). Self-host model uploads, downloads, public/private visibility, verified game identities, player models, textures, model settings, wheel actions, stop events and idle controller states using the same client protocol as the official Cloud.

The Rust service runs independently of Minecraft. Players with updated SparkleMorpher clients can share their public models and actions on ordinary Minecraft servers when they select the same Cloud and bind their game identities.

Source: [sdf123098/spm-cloud](https://github.com/sdf123098/spm-cloud). The official Cloudflare Worker source is in [cloudflare/](cloudflare/README.md). Rust startup variables below configure the self-hosted service; an existing Worker is updated through its own deployment workflow.

## Configuration files

The Rust backend supports strict administrator JSON with an [editor schema](config.schema.json) and [example](config.example.json). Initialize a configuration and persistent bootstrap token with `spm-cloud --init-config /etc/spm-cloud/config.json`, edit the public HTTPS origin and storage paths, then run `spm-cloud --config /etc/spm-cloud/config.json --check-config`. Start with the same `--config` argument. `--print-effective-config` reports merged values and their sources with credentials redacted; both inspection commands exit before database initialization or listening. A selected JSON file is checked every two seconds; runtime settings such as the 128 MiB default upload limit apply without restarting `spm-cloud`. Listener, identity, storage paths and logging changes require a restart.

Selection is `--config` > `SPM_CLOUD_CONFIG` > an existing working-directory `config.json`. Without a selected file, legacy environment startup remains supported. Explicit environment values override JSON; JSON paths are relative to its directory, environment paths to the working directory. Protect the generated `secrets/bootstrap-token.txt` with service-account permissions. Bootstrap password settings initialize missing credentials and do not reset existing passwords on restart. Player display state, explicit vehicle bindings and bound entity motion are opt-in and default off; projectile snapshots remain unavailable. New clients negotiate entity query limits; keep 64 when serving old clients. See [JSON deployment, migration and rollback](deploy/ADMIN_JSON.md).

| Deployment | Configuration | How it is loaded |
|---|---|---|
| Docker Compose | Copy [.env.example](.env.example) to `.env` | Compose passes the supported values to the backend and Caddy |
| Native Linux | Copy [.env.native.example](.env.native.example) to `/etc/spm-cloud.env` | [deploy/spm-cloud.service](deploy/spm-cloud.service) loads it with systemd |
| Native Windows | Persistent PowerShell startup script | Set `$env:SPM_CLOUD_...` before starting the executable |
| Native HTTPS proxy | [deploy/Caddyfile.native](deploy/Caddyfile.native) | Caddy reads the edited file |

The Rust executable does **not** read a `.env` file by itself. Choose one deployment method. Instance IDs, origins, provider IDs, storage locations and operator secrets should be preserved across upgrades.

## 1. Prerequisites

| Item | Requirement |
|---|---|
| Server | Publicly reachable; the native Linux instructions use Ubuntu/Debian with sudo |
| Domain | Replace `cloud.example.com` with your domain |
| DNS | Point the A record at the public IPv4; add AAAA only if IPv6 is reachable |
| Ports | Allow TCP 80 and 443 through the firewall, security group and router |
| Existing websites | Keep their configuration; add a Cloud site to the existing HTTPS proxy |
| Players | Install the updated mod for the correct Minecraft version and loader |

Caddy issues and renews HTTPS certificates after DNS and connectivity are ready. See [automatic HTTPS](https://caddyserver.com/docs/automatic-https).

Give players the bare HTTPS origin, such as `https://cloud.example.com`. Do not append `/v1`, `/health` or the internal port 8787. The backend uses internal HTTP on 8787; the proxy provides HTTPS/WSS.

## 2. Docker Compose

The Compose template builds the Linux backend from the checked-out source on first install; Rust is not required on the host. Later automatic updates use prebuilt, checksummed Docker image archives from GitHub Releases. Install Docker Engine and the Compose plugin using the official [Ubuntu](https://docs.docker.com/engine/install/ubuntu/) or [Debian](https://docs.docker.com/engine/install/debian/) instructions. Windows/macOS can use Docker Desktop in Linux container mode.

### Download and configure

```bash
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
cp .env.example .env
chmod 600 .env
openssl rand -hex 32
nano .env
```

Replace the domain and paste the generated secret into `SPM_CLOUD_ACCESS_TOKEN`:

```dotenv
SPM_CLOUD_HOST=cloud.example.com
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_ACCESS_TOKEN=replace-with-your-generated-secret
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_ALLOW_SELF_REGISTRATION=true
```

`SPM_CLOUD_HOST` is a domain without `https://`. `SPM_CLOUD_ORIGIN` is the public HTTPS origin. IDs start with a letter or number and contain only letters, numbers, dots, underscores and hyphens, up to 128 characters.

The operator secret is not a player password. Leave `SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` unset unless enabling password login for the bootstrap account; it requires a real Argon2id hash.

### Start and verify

```bash
sudo docker compose config --quiet
sudo docker compose up -d --build
sudo docker compose ps
sudo docker compose logs --tail=100 spm-cloud caddy
curl -fsS https://cloud.example.com/health
curl -fsS https://cloud.example.com/v1/instance
```

Check your `instance_id`, the `player_motion_v1` and `game_identity_auth_v1` capabilities, and `auth.self_registration`. The backend port is not exposed publicly by Compose.

After editing `.env`, run `sudo docker compose up -d --force-recreate`; a simple `restart` does not replace container environment variables. The Docker Caddy upstream is `spm-cloud:8787`.

### Optional automatic updates

The release workflow publishes checksummed prebuilt amd64 and arm64 Docker images. The supplied systemd timer downloads and verifies the matching image every 30 minutes; it does not build source code on the server. Install it from `/opt/spm-cloud` using the steps in [automatic updates](deploy/ADMIN_JSON.md).

## 3. Native Linux

Install the prebuilt package for the server's architecture; the release includes x86_64 and ARM64 Linux builds, so the server does not need a Rust toolchain. Runtime does not require Node.js or a separately installed SQLite/OpenSSL.

### Install the release binary

```bash
sudo apt update
sudo apt install -y ca-certificates curl jq nano
curl --fail --location --proto '=https' --tlsv1.2 \
  "https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh" \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
spm-cloud --version
```

The installer selects the matching Linux build and installs `spm-cloud` under `/usr/local/bin`. The optional systemd updater described below downloads and verifies the prebuilt release archive before replacing it.

On a first installation, create the dedicated account and directories; skip account creation if it already exists:

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
sudo install -m 600 /dev/null /etc/spm-cloud.env
openssl rand -hex 32
sudo nano /etc/spm-cloud.env
```

Enter the following settings, replacing the domain, instance ID and generated secret. The Rust executable does not read `.env` automatically; the systemd unit loads this file as environment variables.

```dotenv
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_BIND=127.0.0.1:8787
SPM_CLOUD_ACCESS_TOKEN=replace-with-your-generated-secret
SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data
SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
SPM_CLOUD_DATABASE=/var/lib/spm-cloud/data/spm-cloud.db
SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1
SPM_CLOUD_MAX_ASSET_BYTES=134217728
```

Keep the access token private. The 128 MiB upload limit is the default; the explicit line above documents the corresponding byte value and can be omitted.

### systemd service

```bash
release_tag="$(curl --fail --silent --show-error https://api.github.com/repos/sdf123098/spm-cloud/releases/latest | jq -er '.tag_name')"
deploy_url="https://raw.githubusercontent.com/sdf123098/spm-cloud/${release_tag}/deploy"
curl --fail --location "$deploy_url/spm-cloud.service" --output /tmp/spm-cloud.service
sudo install -m 644 /tmp/spm-cloud.service /etc/systemd/system/spm-cloud.service
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud
sudo systemctl status spm-cloud --no-pager
curl -fsS http://127.0.0.1:8787/health
```

The unit loads `/etc/spm-cloud.env`. After changing it, run `sudo systemctl restart spm-cloud`. Diagnose startup failures with `sudo journalctl -u spm-cloud -n 100 --no-pager`.

### Enable automatic native updates

The optional systemd timer checks GitHub every 30 minutes. It downloads the matching prebuilt Linux archive, verifies its SHA-256 checksum and version, atomically replaces the binary, restarts the service and checks `/health`. If startup health fails, it restores the previous executable. Install it after the backend is running:

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

If the backend does not listen on `127.0.0.1:8787`, set `SPM_CLOUD_AUTO_UPDATE_HEALTH_URL` in `/etc/spm-cloud-auto-update.env` to its local health URL. Check update runs with `sudo journalctl -u spm-cloud-native-update.service`.

### HTTPS with Caddy

Install Caddy using its [official installation instructions](https://caddyserver.com/docs/install). On Debian/Ubuntu:

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install -y caddy
sudo nano /etc/caddy/Caddyfile
```

Add this site block to the existing Caddyfile, replacing the domain and preserving existing sites:

```caddyfile
cloud.example.com {
    reverse_proxy 127.0.0.1:8787 {
        header_up X-SPM-Client-IP {http.request.remote.host}
    }
}
```

```bash
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl enable --now caddy
sudo systemctl reload caddy
curl -fsS https://cloud.example.com/v1/instance
```

Caddy proxies WebSockets automatically. Its overwritten `X-SPM-Client-IP` header and `SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1` let the backend rate-limit by the real player address. Native deployment uses `127.0.0.1:8787`, not the Docker service name.

## 4. Native Windows

Install the prebuilt Windows x64 package from the [GitHub Release](https://github.com/sdf123098/spm-cloud/releases/latest). The PowerShell installer places `spm-cloud.exe` in the user's Cargo bin directory; no Rust toolchain is required:

```powershell
$env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
```

Use the installed executable from `$env:USERPROFILE\.cargo\bin\spm-cloud.exe`; keep mutable data in a separate directory such as `C:/SPMCloud`.

Create a persistent startup script using this PowerShell configuration:

```powershell
Set-Location C:/SPMCloud
$env:SPM_CLOUD_INSTANCE_ID = 'my-server-cloud'
$env:SPM_CLOUD_ORIGIN = 'https://cloud.example.com'
$env:SPM_CLOUD_BIND = '127.0.0.1:8787'
$env:SPM_CLOUD_DATA_DIR = 'C:/SPMCloud/data'
$env:SPM_CLOUD_OBJECT_DIR = 'C:/SPMCloud/objects'
$env:SPM_CLOUD_ACCESS_TOKEN = 'replace-with-your-saved-random-secret'
$env:SPM_CLOUD_BOOTSTRAP_ACCOUNT = 'account_local'
$env:SPM_CLOUD_ALLOW_SELF_REGISTRATION = 'true'
$env:SPM_CLOUD_TRUSTED_PROXY_IPS = '127.0.0.1'
& "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
```

Generate a secret once with `-join (1..4 | ForEach-Object { [Guid]::NewGuid().ToString('N') })`, then save it in that script and reuse it. Keep the script private. A `.env` file alone has no effect on native Windows.

In another terminal check `Invoke-RestMethod http://127.0.0.1:8787/health`. Download Windows Caddy from [caddyserver.com](https://caddyserver.com/download), copy and edit `deploy/Caddyfile.native`, and run:

```powershell
.\caddy.exe run --config .\Caddyfile.native --adapter caddyfile
Invoke-RestMethod https://cloud.example.com/v1/instance
```

Keep both processes running. For unattended operation, configure Task Scheduler to start each program with its working directory and restart behavior, or use Docker Compose.

For unattended updates, run the backend from a Task Scheduler task named `SPM Cloud` under the same Windows account that installed the package. Download the updater script from the current stable release and schedule it daily under that account:

```powershell
$release = Invoke-RestMethod https://api.github.com/repos/sdf123098/spm-cloud/releases/latest
$url = "https://raw.githubusercontent.com/sdf123098/spm-cloud/$($release.tag_name)/deploy/windows-auto-update.ps1"
$updater = "$env:ProgramData\SPMCloud\windows-auto-update.ps1"
New-Item -ItemType Directory -Force (Split-Path $updater) | Out-Null
Invoke-WebRequest $url -OutFile $updater
```

The script downloads the prebuilt Windows package, verifies its SHA-256 and version, stops the backend task, replaces the executable, then starts the task and checks `/health`. If the check fails it restores the old executable. If the backend is running interactively, stop it before invoking the updater.

## External login: custom Yggdrasil and all-in-one

### One full hasJoined URL

Add this to the Docker `.env` or Linux `/etc/spm-cloud.env`:

```dotenv
SPM_CLOUD_HAS_JOINED_URL=https://auth.example.com/all-in-one/hasJoined
```

This creates/enables provider `custom`. Use the complete HTTPS endpoint; no root/path splitting is needed. An all-in-one gateway can verify all the accounts it supports through this one configuration.

Cloud appends `username` and `serverId`. The service must return the verified Yggdrasil player `id` and `name`, matching the challenge. The game's access token stays with the launcher's configured Session Service and is never sent to Cloud.

On Windows, set `$env:SPM_CLOUD_HAS_JOINED_URL = 'https://auth.example.com/all-in-one/hasJoined'` before starting the executable. Recreate Docker containers or restart the native service to apply changes.

### Multiple services, existing IDs and disabling

Use a JSON array instead of the shortcut; the two non-empty options are mutually exclusive:

```dotenv
SPM_CLOUD_IDENTITY_PROVIDERS='[{"provider_id":"my-auth","display_name":"My Auth","has_joined_url":"https://auth.example.com/all-in-one/hasJoined","enabled":true}]'
```

Standard services can instead specify `base_url` and optional `session_path`. The default path is `/sessionserver/session/minecraft/hasJoined` appended to the root. For example:

```json
{"provider_id":"skin-site","display_name":"Skin Site","base_url":"https://auth.example.com/api/yggdrasil","enabled":true}
```

Provider IDs namespace existing identities: keep an existing ID when updating its configuration. Gateway accounts must not collide on UUID. Set `enabled:false` to disable a provider; deleting an environment variable does not remove its persisted database configuration. The reserved `official` provider cannot be replaced.

Both Rust and Worker support the operator-authenticated `POST /v1/identity-providers` with one such JSON object. `has_joined_url` is mutually exclusive with `base_url` / `session_path`. Verification URLs require a public HTTPS hostname, with no non-default port, credentials, query or fragment. Players cannot add trusted services themselves.

The official Cloud operator configures providers for the official instance. Self-hosted settings do not automatically register that service on the official Cloud. The two backends share source-level support; updating a running Rust service or a running Worker requires its own release step.

## 5. Player setup and model sharing

1. Update SparkleMorpher. Open **Instance Management → Add Instance**, enter the Cloud HTTPS origin and optionally a name. The instance ID is discovered automatically; the Cloud tab's **+** opens the same form.
2. Select the instance and open **Account Management**. Connect/create an account using the game identity, or use password login/registration. Accounts belong to that Cloud instance.
3. After password login, click **Bind current game identity**. A connected Cloud account alone does not prove the game identity is bound.
4. Once bound, the client automatically restores login. It retries failures at intervals and resumes after selecting another Cloud; manual logout disables automatic restoration until an instance is selected again. Automatic restoration does not register new accounts.
5. Upload a model, make it **public**, select it and check models, textures, wheel playback/stop, model settings and idle states with another player using the same Cloud.

Community **All Models** lists accessible models without a search term. Recent, Favorites, My Models and Public Models are also available; the separate Public Models view requires a search. Only owners can change visibility.

Official Minecraft accounts use certificate/session proofs; third-party accounts require a trusted enabled provider corresponding to the launcher. An offline name/UUID cannot become a verified global identity by self-reporting. Scope-specific offline approval is separate from ordinary player identity verification.

Private models do not publish appearance or actions to other players. Selecting a different Cloud, an unverified identity or an unavailable model prevents sharing. With registration disabled, an operator must first create the account through `POST /v1/accounts`.

## Configuration reference

Defaults below describe the Rust process; Compose overrides bind, storage and proxy settings for its container network.

| Variable | Default | Purpose |
|---|---|---|
| `SPM_CLOUD_INSTANCE_ID` | `local-dev` | Stable instance ID; the templates use `self-hosted` |
| `SPM_CLOUD_ORIGIN` | `https://localhost` | Bare public HTTPS origin |
| `SPM_CLOUD_BIND` | `127.0.0.1:8787` | HTTP listener; Compose fixes `0.0.0.0:8787` |
| `SPM_CLOUD_DATA_DIR` | `data` | Data directory |
| `SPM_CLOUD_DATABASE` | `<DATA_DIR>/spm-cloud.db` | SQLite database |
| `SPM_CLOUD_OBJECT_DIR` | `<DATA_DIR>/objects` | Model object directory; templates use `objects` |
| `SPM_CLOUD_ACCESS_TOKEN` | Unset | Operator bearer secret |
| `SPM_CLOUD_BOOTSTRAP_ACCOUNT` | `account_local` | Bootstrap account ID |
| `SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` | Unset | Optional Argon2id password hash |
| `SPM_CLOUD_ALLOW_SELF_REGISTRATION` | `true` | `true` or `false` |
| `SPM_CLOUD_TRUSTED_PROXY_IPS` | Empty | Comma-separated trusted proxy IPs |
| `SPM_CLOUD_HAS_JOINED_URL` | Unset | One complete endpoint; provider `custom` |
| `SPM_CLOUD_IDENTITY_PROVIDERS` | Unset | Operator provider JSON array |
| `SPM_CLOUD_MAX_ASSET_BYTES` | `134217728` | Asset bytes, allowed 1 through 4 GiB |
| `SPM_CLOUD_MAX_MESSAGE_BYTES` | `65536` | Protocol message bytes, allowed 1 KiB through 1 MiB |

Compose-only variables are `SPM_CLOUD_HOST`, `SPM_CLOUD_DATA_VOLUME`, `SPM_CLOUD_OBJECTS_VOLUME`, `SPM_CLOUD_NETWORK_SUBNET` and `SPM_CLOUD_PROXY_IP`. If the subnet conflicts with another network, change subnet and proxy IP together.

## 6. Backup and upgrade

Back up the database, model objects and original configuration together. Stop the backend before a file-level backup and include the entire database directory, including any SQLite WAL files.

### Docker

Use a new backup directory each time, substituting your existing volume names if customized:

```bash
mkdir -p backup
chmod 700 backup
sudo docker compose stop spm-cloud
sudo docker run --rm -v spm-cloud-data:/from:ro -v "$PWD/backup:/to" alpine sh -c 'tar czf /to/data.tgz -C /from .'
sudo docker run --rm -v spm-cloud-objects:/from:ro -v "$PWD/backup:/to" alpine sh -c 'tar czf /to/objects.tgz -C /from .'
cp .env backup/instance.env
chmod 600 backup/instance.env
sudo docker compose start spm-cloud
git pull --ff-only
sudo SPM_CLOUD_COMPOSE_DIR="$PWD" bash deploy/docker-auto-update.sh
```

Copy the backup off the server. Preserve existing volume names; `docker compose down -v` deletes persistent volumes. Restore archives to new volumes and update the two volume names in `.env` to retain the originals for comparison.

### Native

On Linux, stop the service and archive `/var/lib/spm-cloud`, `/etc/spm-cloud.env`, the systemd unit and Caddy configuration. Restrict access to the archive because it contains the operator secret. Then start the service again. The release installer downloads the latest prebuilt package; no source build is required:

```bash
backup_file="$HOME/spm-cloud-backup-$(date +%Y%m%d-%H%M%S).tgz"
sudo systemctl stop spm-cloud
sudo tar -czf "$backup_file" -C / var/lib/spm-cloud etc/spm-cloud.env etc/systemd/system/spm-cloud.service etc/caddy/Caddyfile
sudo chmod 600 "$backup_file"
sudo systemctl start spm-cloud
```

```bash
curl --fail --location --proto '=https' --tlsv1.2 \
  https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
sudo systemctl restart spm-cloud
curl -fsS https://cloud.example.com/health
```

On Windows stop the executable, back up `data`, `objects` and the startup configuration, replace the binary and restart with the same settings. Normal upgrades migrate the database incrementally. Restore matching storage/configuration and a compatible binary; never mix an old WAL with a different database.

## 7. Troubleshooting

| Symptom | Check |
|---|---|
| HTTPS/certificate failure | DNS, reachable ports 80/443, occupied ports and Caddy logs |
| Caddy 502 | Backend running; native upstream `127.0.0.1:8787`, Docker upstream `spm-cloud:8787` |
| Native service exits | Origin syntax, directory permissions, EnvironmentFile and journal logs |
| Third-party login fails | Enabled provider, exact hasJoined URL, matching launcher and first identity binding |
| Identity already linked | Log into the original account; avoid repeatedly creating new accounts |
| Registration denied | `auth.self_registration`; explicit disabled registration requires operator-created accounts |
| Other players cannot see the model/action | Same Cloud, updated clients/backend, verified identities and a public model |
| Empty Public Models results | Enter a search term; use All Models on a community instance for the accessible catalog |
| Large upload rejected | The default is 128 MiB. Check `SPM_CLOUD_MAX_ASSET_BYTES` and any proxy body-size limits; JSON limit changes hot reload, while environment changes require container recreation or a service restart |
| Configuration edits have no effect | Confirm the process started with the intended `--config` file. Runtime JSON settings hot reload; environment settings and restart-only JSON fields require a restart |

One Rust instance uses SQLite and a local object directory. Do not run multiple backends against the same SQLite file. Validate with two real players, including rejoining the world; a successful health response only verifies service reachability.

## Development and API

The Rust service uses Axum, SQLite WAL and local objects. The official adapter uses Worker, D1, R2 and Durable Objects. The protocol is shared; storage and deployment configuration are separate.

| Area | Endpoints |
|---|---|
| Health/discovery | `GET /health`, `GET /v1/instance` |
| Registration/password login | `POST /v1/accounts`, `POST /v1/sessions` |
| Refresh/logout | `POST /v1/sessions/refresh`, `DELETE /v1/sessions/current` |
| Game login | `POST /v1/auth/login-challenges` and its `/complete` |
| Identity binding | `POST /v1/auth/challenges` and its `/complete` |
| Identities/providers | `/v1/identities`, `/v1/identity-providers` |
| Model catalog/uploads | `GET/POST /v1/assets` |
| Player appearance/actions | `PUT /v1/players/me/appearance`, `POST /v1/players/appearances/query` |

Operator example for creating an account when registration is closed; set the operator secret in your terminal first:

```bash
curl -fsS https://cloud.example.com/v1/accounts -H "Authorization: Bearer $SPM_CLOUD_ACCESS_TOKEN" -H 'Content-Type: application/json' --data '{"account_id":"alice","password":"replace-with-initial-password"}'
```

An account password creates a Cloud session; the game identity still requires a challenge. Player `motion` shares the appearance revision transaction and carries wheel events, Molang settings/expressions and controller state. Omitted/cleared motion or a cleared model removes old actions. Private, expired, unverified or unavailable appearances are hidden.

Scope-local `STRICT_APPROVAL`, `CLAIM_CODE`, `FIRST_CLAIM` and `DISABLED` offline policies do not replace ordinary player verification.

### Build and checks

Tests additionally require Node.js 22+ for synthetic RSA fixtures; runtime does not. SQLite is bundled, TLS uses Rustls and protoc is provided by build dependencies.

```text
cargo fmt --check
cargo check --all-targets --all-features --locked
cargo test --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
```

Explicit-target binaries are under `target/<target>/release`; native builds normally use `target/release`. Developer-specific GNU/MSYS2 settings are ignored by Git and are not required on other devices.

[Cross-platform CI](.github/workflows/platforms.yml) checks Ubuntu, Windows and macOS builds and HTTP contracts, the Rust minimum version and a Linux container build. Local Windows checks do not establish that native systemd, macOS or Docker have been run locally.

Official certificate trust is built in or fetched from fixed official HTTPS endpoints. No environment/request option replaces production trust roots. See [Worker deployment](cloudflare/README.md) and [test fixtures](tests/README.md). `tools/migrate_legacy.py` audits legacy catalogs without importing files, deleting originals or granting access.

**License:** MIT.
