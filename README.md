# SPM Cloud

**A self-hosted Rust backend for [SparkleMorpher](https://github.com/sdf123098/Sparkle-Morpher).** It shares the Cloud client protocol and supports model uploads/downloads, visibility, verified game identities, player appearances, textures and wheel actions. Players connect by selecting the same Cloud instance in the mod.

> **English** | [中文](README_zh.md)

The backend runs independently of Minecraft. You can deploy it beside a Minecraft server or on a separate public server. Players need a current SparkleMorpher build and the HTTPS address of your instance.

## Choose an installation method

| Method | Best for | Initial installation | Updates |
|---|---|---|---|
| Docker Compose | Most Linux servers; managed data volumes and automatic restarts | Clone this repository and build the container once with Docker Compose; Rust is not needed on the host | Optional Linux systemd updater downloads checksum-verified, prebuilt Docker images for amd64/arm64 |
| Native Linux | Ubuntu/Debian servers that prefer a systemd service | Download the latest stable, prebuilt Linux installer; supports x86_64 and ARM64 | Optional systemd timer downloads a prebuilt package, verifies it, restarts the service and rolls back on failed health checks |
| Native Windows | Windows servers without Docker | Run the latest stable PowerShell installer; prebuilt x64 executable | Optional Task Scheduler job downloads the verified prebuilt package and rolls back if health checks fail |
| macOS package | Local or manual runs on Intel or Apple Silicon | Download the matching archive from [GitHub Releases](https://github.com/sdf123098/spm-cloud/releases/latest) | Manual replacement; the service updater tutorials below cover Linux and Windows |

All release links below follow GitHub's **latest stable release**; commands do not contain a version number. For a tagged/older release, select a specific version on the [Releases page](https://github.com/sdf123098/spm-cloud/releases). Default model upload size is **128 MiB**. JSON runtime settings can be hot-reloaded; see [JSON configuration and deployment](deploy/ADMIN_JSON.md).

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

Run on the Docker host:

```bash
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
cp .env.example .env
chmod 600 .env
openssl rand -hex 32
nano .env
```

Set your domain and paste the generated random value into `SPM_CLOUD_ACCESS_TOKEN`:

```dotenv
SPM_CLOUD_HOST=cloud.example.com
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_ACCESS_TOKEN=replace-with-your-generated-secret
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_ALLOW_SELF_REGISTRATION=true
SPM_CLOUD_MAX_ASSET_BYTES=134217728
```

`SPM_CLOUD_HOST` is the hostname without `https://`; `SPM_CLOUD_ORIGIN` is the public HTTPS origin. Keep the operator token private. It is not a player's password. The 128 MiB upload limit is the default, so the final line can be omitted.

### 2. Start and verify

```bash
sudo docker compose config --quiet
sudo docker compose up -d --build
sudo docker compose ps
sudo docker compose logs --tail=100 spm-cloud caddy
curl -fsS https://cloud.example.com/health
curl -fsS https://cloud.example.com/v1/instance
```

Confirm `/v1/instance` returns your instance ID and capabilities. Compose exposes HTTPS through Caddy; it does not publish port 8787. After changing `.env`, recreate the container so it receives the new environment:

```bash
sudo docker compose up -d --force-recreate spm-cloud
```

A plain `docker compose restart` does not update container environment variables.

### 3. Optional automatic updates

The updater checks GitHub Releases every 30 minutes and downloads the matching amd64/arm64 Docker archive. It verifies the SHA-256 checksum, loads the image and updates only the backend service. Data volumes and Caddy remain in place. The supplied systemd units expect the repository at `/opt/spm-cloud`:

```bash
sudo apt install -y curl jq gzip coreutils util-linux
sudo cp deploy/spm-cloud-auto-update.service deploy/spm-cloud-auto-update.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-auto-update.timer
sudo systemctl start spm-cloud-auto-update.service
sudo systemctl list-timers spm-cloud-auto-update.timer
```

If the checkout is elsewhere, change `/opt/spm-cloud` in the service and timer before enabling them. For manual updates, back up first, then run `git pull --ff-only` followed by `sudo SPM_CLOUD_COMPOSE_DIR="$PWD" bash deploy/docker-auto-update.sh` from the repository directory. The updater applies the latest prebuilt image.

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

### 2. Create the service account and environment file

Run user creation only on a first install. Keep the data and object directories persistent across upgrades:

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
sudo install -m 600 /dev/null /etc/spm-cloud.env
openssl rand -hex 32
sudo nano /etc/spm-cloud.env
```

Enter the settings below, replacing the hostname, instance ID and secret:

```dotenv
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_BIND=127.0.0.1:8787
SPM_CLOUD_ACCESS_TOKEN=replace-with-your-generated-secret
SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data
SPM_CLOUD_DATABASE=/var/lib/spm-cloud/data/spm-cloud.db
SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1
SPM_CLOUD_MAX_ASSET_BYTES=134217728
```

The 128 MiB limit is the default. Keep `SPM_CLOUD_ACCESS_TOKEN` private and stable. The backend does not load `.env` on its own; the systemd unit below reads `/etc/spm-cloud.env`.

### 3. Install and start the systemd service

Fetch the service unit that matches the current stable release, then enable it:

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

Check logs with `sudo journalctl -u spm-cloud -n 100 --no-pager`. After editing `/etc/spm-cloud.env`, restart with `sudo systemctl restart spm-cloud`.

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

DNS must point to the server and ports 80/443 must be reachable for certificate issuance. Caddy handles HTTPS and WebSockets. The proxy header works with `SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1` above.

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

Keep the executable under `$env:USERPROFILE\.cargo\bin` and mutable data in a separate directory, for example `C:\SPMCloud`. Save a private startup script as `C:\SPMCloud\start-cloud.ps1`:

```powershell
Set-Location C:\SPMCloud
$env:SPM_CLOUD_INSTANCE_ID = 'my-server-cloud'
$env:SPM_CLOUD_ORIGIN = 'https://cloud.example.com'
$env:SPM_CLOUD_BIND = '127.0.0.1:8787'
$env:SPM_CLOUD_DATA_DIR = 'C:\SPMCloud\data'
$env:SPM_CLOUD_OBJECT_DIR = 'C:\SPMCloud\objects'
$env:SPM_CLOUD_ACCESS_TOKEN = 'replace-with-your-saved-random-secret'
$env:SPM_CLOUD_BOOTSTRAP_ACCOUNT = 'account_local'
$env:SPM_CLOUD_ALLOW_SELF_REGISTRATION = 'true'
$env:SPM_CLOUD_TRUSTED_PROXY_IPS = '127.0.0.1'
& "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
```

Generate a secret once with `-join (1..4 | ForEach-Object { [Guid]::NewGuid().ToString('N') })`, save it in the script, and keep that file private. A `.env` file alone does not configure a native Windows process.

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

**JSON is optional.** The Docker and native installation tutorials above use environment variables; those deployments do not need a JSON file. The Rust executable does not read `.env` itself, so Compose or your service/startup script must pass environment variables to it.

Choose administrator JSON when you want validated, centralized settings and runtime hot reload. You do not need to write it from scratch: `spm-cloud --init-config <path>` creates a starter file and a unique bootstrap token; edit only the instance-specific values such as HTTPS origin and storage paths. The token and paths are unique to each deployment, so a shared, prefilled config would be unsafe and usually point at the wrong data. An [editor schema](config.schema.json), [example](config.example.json) and full [deployment guide](deploy/ADMIN_JSON.md) are available. JSON is checked every two seconds. Runtime settings such as upload/request limits, registration policy, identity providers and visual runtime settings hot-reload without restarting the process. Listener address, instance identity/origin, storage paths and logging settings require a restart. Explicit environment variables override matching JSON values.

The default `limits.max_asset_bytes` / `SPM_CLOUD_MAX_ASSET_BYTES` is `134217728` bytes (128 MiB). To accept larger models, raise the backend limit and the reverse proxy's request-body limit together. A JSON limit change hot-reloads; an environment change requires a container recreation or service restart.

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

For one external authentication endpoint, add this environment variable to `.env`, `/etc/spm-cloud.env` or the Windows startup script:

```dotenv
SPM_CLOUD_HAS_JOINED_URL=https://auth.example.com/all-in-one/hasJoined
```

Use the complete HTTPS `hasJoined` URL. The backend appends `username` and `serverId`; the service must return the verified Yggdrasil player ID and name. For multiple providers or stable provider IDs, configure `SPM_CLOUD_IDENTITY_PROVIDERS` or use JSON. Keep provider IDs when changing the service: they identify existing account bindings. Full details are in the [JSON deployment guide](deploy/ADMIN_JSON.md).

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
| Native Linux service exits | `sudo journalctl -u spm-cloud -n 100`; origin syntax, environment file, file ownership and storage permissions |
| Port already in use | Identify what uses 8787/80/443 before changing the listener or proxy |
| Player cannot log in or bind | Same Cloud selected; account session works; game identity binding completed; external provider matches the launcher |
| Other players cannot see a model/action | Both players use the same instance; identity is verified; model is public; client and backend are current |
| Upload rejected | Backend default is 128 MiB; check `limits.max_asset_bytes` or `SPM_CLOUD_MAX_ASSET_BYTES` and the proxy body-size cap |
| JSON edit has no effect | Process started with the intended `--config`; effective environment variables may override JSON; restart-only fields need a restart |
| `.env` edit has no effect | Docker: recreate the container. Native: ensure the service/startup script loads the file, then restart the process |

One instance uses SQLite and a local object directory. Do not point multiple backend processes at the same SQLite file. Validate the deployment with two real players; `/health` only confirms reachability.

## API and development

The Rust backend uses Axum, SQLite WAL and local object storage. The official adapter uses Cloudflare Worker, D1, R2 and Durable Objects. The clients share a protocol; deployment and storage are separate.

| Feature | Endpoint |
|---|---|
| Health and discovery | `GET /health`, `GET /v1/instance` |
| Create account / password login | `POST /v1/accounts`, `POST /v1/sessions` |
| Refresh / log out | `POST /v1/sessions/refresh`, `DELETE /v1/sessions/current` |
| Game login and identity binding | `/v1/auth/login-challenges` and `/v1/auth/challenges` |
| Providers and identities | `/v1/identity-providers`, `/v1/identities` |
| Model catalog and upload | `GET/POST /v1/assets` |
| Player appearance/actions | `PUT /v1/players/me/appearance`, `POST /v1/players/appearances/query` |

When self-registration is disabled, an operator can create an account using the bearer token:

```bash
curl -fsS https://cloud.example.com/v1/accounts \
  -H "Authorization: Bearer $SPM_CLOUD_ACCESS_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"account_id":"alice","password":"replace-with-initial-password"}'
```

Rust development requires Rust 1.87 or newer. Tests additionally need Node.js 22+ for test fixtures; production runtime does not. Build and CI details are in [platform CI](.github/workflows/platforms.yml); Worker deployment is documented in [cloudflare/README.md](cloudflare/README.md).

**License:** MIT.
