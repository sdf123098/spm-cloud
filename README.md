# SPM Cloud

**Self-hosted Rust release: 2.1.0.** JSON runtime settings reload automatically without restarting the service; the default per-model upload limit is 128 MiB. Docker Compose installations can enable periodic GitHub release updates; see [JSON deployment, migration and rollback](deploy/ADMIN_JSON.md).

> **English** | [中文](README_zh.md)

A standalone Rust Cloud backend for [Sparkle's Morpher](https://github.com/sdf123098/Sparkle-Morpher). Self-host model uploads, downloads, public/private visibility, verified game identities, player models, textures, model settings, wheel actions, stop events and idle controller states using the same client protocol as the official Cloud.

The Rust service runs independently of Minecraft. Players with updated SparkleMorpher clients can share their public models and actions on ordinary Minecraft servers when they select the same Cloud and bind their game identities.

Source: [sdf123098/spm-cloud](https://github.com/sdf123098/spm-cloud). The official Cloudflare Worker source is in [cloudflare/](cloudflare/README.md). Rust startup variables below configure the self-hosted service; an existing Worker is updated through its own deployment workflow.

## Configuration files

Version 2.1.0 supports strict administrator JSON with [editor schema](config.schema.json) and [example](config.example.json). Initialize a configuration and persistent bootstrap token with `spm-cloud --init-config /etc/spm-cloud/config.json`, edit the public HTTPS origin and storage paths, then run `spm-cloud --config /etc/spm-cloud/config.json --check-config`. Start with the same `--config` argument. `--print-effective-config` reports merged values and their sources with credentials redacted; both inspection commands exit before database initialization or listening. A selected JSON file is checked every two seconds; runtime settings such as the 128 MiB default upload limit apply without restarting `spm-cloud`. Listener, identity, storage paths and logging changes require a restart.

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

Docker builds the Linux backend; Rust is not required on the host. Install Docker Engine and the Compose plugin using the official [Ubuntu](https://docs.docker.com/engine/install/ubuntu/) or [Debian](https://docs.docker.com/engine/install/debian/) instructions. Windows/macOS can use Docker Desktop in Linux container mode.

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

## 3. Native Linux

Build with Rust **1.87 or newer** and a C compiler. Runtime does not require Rust, Node.js or a separately installed SQLite/OpenSSL. Build for your server's actual OS and CPU; a Windows executable cannot run on Linux.

### Build and install

```bash
sudo apt update
sudo apt install -y build-essential ca-certificates curl git openssl nano
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
sh rustup-init.sh -y --profile minimal
. "$HOME/.cargo/env"
rustc --version
mkdir -p "$HOME/spm-build"
cd "$HOME/spm-build"
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
cargo build --release --locked
sudo install -m 755 target/release/spm-cloud /usr/local/bin/spm-cloud
```

Skip rustup installation when already installed. Use the [official Rust installation guide](https://rust-lang.org/tools/install/) when another platform/toolchain is needed. Low-memory machines can build with `-j 1`.

On a first installation, create the dedicated account and directories; skip account creation if it already exists:

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
sudo install -m 600 .env.native.example /etc/spm-cloud.env
openssl rand -hex 32
sudo nano /etc/spm-cloud.env
```

Set your origin, ID and generated secret. Relative `data` / `objects` paths in the template resolve under the service working directory `/var/lib/spm-cloud`; absolute paths are also supported.

### systemd service

```bash
sudo install -m 644 deploy/spm-cloud.service /etc/systemd/system/spm-cloud.service
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud
sudo systemctl status spm-cloud --no-pager
curl -fsS http://127.0.0.1:8787/health
```

The unit loads `/etc/spm-cloud.env`. After changing it, run `sudo systemctl restart spm-cloud`. Diagnose startup failures with `sudo journalctl -u spm-cloud -n 100 --no-pager`.

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

Add the site from [deploy/Caddyfile.native](deploy/Caddyfile.native), replacing the domain and preserving existing sites:

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

Use a Windows binary package supplied by the project, or build with `cargo build --release --locked` and the C toolchain required by your Rust target. Put the executable in a directory such as `C:/SPMCloud`.

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
.\spm-cloud.exe
```

Generate a secret once with `-join (1..4 | ForEach-Object { [Guid]::NewGuid().ToString('N') })`, then save it in that script and reuse it. Keep the script private. A `.env` file alone has no effect on native Windows.

In another terminal check `Invoke-RestMethod http://127.0.0.1:8787/health`. Download Windows Caddy from [caddyserver.com](https://caddyserver.com/download), copy and edit `deploy/Caddyfile.native`, and run:

```powershell
.\caddy.exe run --config .\Caddyfile.native --adapter caddyfile
Invoke-RestMethod https://cloud.example.com/v1/instance
```

Keep both processes running. For unattended operation, configure Task Scheduler to start each program with its working directory and restart behavior, or use Docker Compose.

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
sudo docker compose up -d --build
```

Copy the backup off the server. Preserve existing volume names; `docker compose down -v` deletes persistent volumes. Restore archives to new volumes and update the two volume names in `.env` to retain the originals for comparison.

### Native

On Linux, stop the service and archive `/var/lib/spm-cloud`, `/etc/spm-cloud.env`, the systemd unit and Caddy configuration. Restrict access to the archive because it contains the operator secret. Then start the service again. Build the replacement before stopping the service to install it:

```bash
backup_file="$HOME/spm-cloud-backup-$(date +%Y%m%d-%H%M%S).tgz"
sudo systemctl stop spm-cloud
sudo tar -czf "$backup_file" -C / var/lib/spm-cloud etc/spm-cloud.env etc/systemd/system/spm-cloud.service etc/caddy/Caddyfile
sudo chmod 600 "$backup_file"
sudo systemctl start spm-cloud
```

```bash
git pull --ff-only
cargo build --release --locked
sudo systemctl stop spm-cloud
sudo cp /usr/local/bin/spm-cloud /usr/local/bin/spm-cloud.previous
sudo install -m 755 target/release/spm-cloud /usr/local/bin/spm-cloud
sudo systemctl start spm-cloud
curl -fsS https://cloud.example.com/v1/instance
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
| Large upload rejected | Asset limit and the proxy's body-size limits |
| Configuration edits have no effect | Recreate Compose containers; restart native processes with the new environment |

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
