# SPM Cloud 2.2 Guide

**SPM Cloud is the self-hosted backend for the SparkleMorpher mod.** The owner runs the service and manages models. Players add the same Cloud address in the mod to share public models, appearances, and actions.

> [中文](README_zh.md) | **English**
>
> If the “Initial access token” box is blank, that is normal. The token is saved on the Cloud server. Copy it from the server file and paste it into the page.

## Three credentials

- **Initial access token:** the owner’s first sign-in credential for the admin page. `spm-cloud --init-config` creates it in a separate text file. It is not a Minecraft token or player password.
- **Cloud account and password:** used on one Cloud instance to sign in and own private models. Accounts are separate across instances.
- **Game account credentials:** used to verify a Minecraft or owner-enabled third-party identity. Enter credentials only on the appropriate official or third-party login page.

The initial token is not sent to your browser. A remote WebUI cannot read files on the server. Sign in to the server, copy the token from its file, and paste it into the page.

## 1. Install Cloud

### Recommended: Docker Compose on Linux

You need a Linux server, a domain pointing to it, inbound TCP ports 80/443, Docker Engine, and the Compose plugin:

```bash
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
mkdir -p deploy/runtime
docker compose -f docker-compose.json.yml build spm-cloud
sudo chown 10001:10001 deploy/runtime
sudo docker run --rm --user 10001:10001 \
  -v "$PWD/deploy/runtime:/etc/spm-cloud" spm-cloud:local \
  --init-config /etc/spm-cloud/config.json
```

This creates `deploy/runtime/config.json` and `deploy/runtime/secrets/bootstrap-token.txt` in the repository directory. Edit the JSON, replace its example domain with your HTTPS domain, and set a unique instance ID. Replace `cloud.example.com` in `Caddyfile.compose` with the same domain, then start:

```bash
sudo nano deploy/runtime/config.json
sudo nano Caddyfile.compose
sudo docker compose -f docker-compose.json.yml up -d
```

Check status with `sudo docker compose -f docker-compose.json.yml ps`. Give players the HTTPS root address, such as `https://cloud.example.com`, without a port, `/admin/`, or `/v1`. Do not use `docker compose down -v` for routine upgrades; it deletes volumes.

### Native Linux or Windows

Follow the [deployment guide](deploy/ADMIN_JSON.md) for complete instructions. The Linux default config is `/etc/spm-cloud/config.json`; initialize it as the service account:

```bash
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
```

On Windows, install the prebuilt binary with the PowerShell installer and initialize in PowerShell (example data path `C:\SPMCloud`):

```powershell
$env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
$exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
$config = "C:\SPMCloud\config.json"
New-Item -ItemType Directory -Force C:\SPMCloud | Out-Null
Set-Location C:\SPMCloud
& $exe --init-config $config
```

The example token path is `C:\SPMCloud\secrets\bootstrap-token.txt`. Edit the domain, instance ID, and storage paths, then run `& $exe --config C:\SPMCloud\config.json`. For a persistent service, automatic startup, and HTTPS proxy, see the [deployment guide](deploy/ADMIN_JSON.md). Do not expose backend port 8787 to the internet.

## 2. Find and use the initial token

The token file is plain text with one long line beginning `spm_bootstrap_`.

| Deployment | Default token file | How to read it |
|---|---|---|
| Docker Compose | Repository directory: `deploy/runtime/secrets/bootstrap-token.txt` | From the repository directory, run `sudo cat deploy/runtime/secrets/bootstrap-token.txt` |
| Native Linux | `/etc/spm-cloud/secrets/bootstrap-token.txt` | Run `sudo cat /etc/spm-cloud/secrets/bootstrap-token.txt` |
| Windows example | `C:\SPMCloud\secrets\bootstrap-token.txt` | PowerShell: `Get-Content 'C:\SPMCloud\secrets\bootstrap-token.txt'` |
| Prebuilt package launcher | `secrets/bootstrap-token.txt` under the config directory | Open the text file |

With a custom config path, the default token remains in `secrets/bootstrap-token.txt` beside that config. If `auth.bootstrap_access_token_file` was changed, use that path; relative paths start from the config directory. Inside Docker, `/etc/spm-cloud/secrets/bootstrap-token.txt` maps to `secrets/bootstrap-token.txt` in the host-mounted config directory.

1. Open `https://your-domain/admin/`.
2. Copy the complete line from the token file into “Initial access token” and sign in.
3. Open “Accounts & permissions.” On the owner account, choose “Set owner password.”
4. Later, sign in with the owner account ID and password. The initial token remains valid, so keep protecting it.

The token is stored in a server file; initialization does not print it, and the remote page cannot fill it in. Do not put it in JSON, command-line arguments, chat, screenshots, or public logs. Linux initialization restricts file permissions. On Windows, limit access to the service account and owner. Include config and token in a protected offline backup.

## 3. Use the WebUI

The WebUI is built into Cloud; no separate website or Node.js install is needed.

- `/admin/`: owner and administrators created and distributed by the owner.
- `/account/`: players manage their own Cloud account and links.

The owner can change settings, manage accounts, and manage models. The owner creates admin accounts and securely gives each person their account and initial password. Admins can manage models and view settings, but cannot change server config or create accounts. Regular players have no admin access. Password resets, account disablement, and role changes invalidate active sessions.

The sidebar stays in place while settings and model lists scroll independently with the mouse wheel. The model library supports search, pages, grid or 3D cards, multi-select, and bulk actions. Click a model name for details. Select models to change visibility, rescan, manually label, delete, or restore. Deletion can be reversed while the original file is retained; back up before permanent cleanup.

Settings group everyday options into “Models & content safety,” “Accounts & permissions,” “Service access,” and “Maintenance & cleanup.” Server paths, proxy IPs, and protocol details are advanced settings. Saving requires startup with `--config PATH` and write access to the config directory. Some options need a restart.

The JSON editor validates before saving. If a bad config prevents startup, restore a backup and validate with `spm-cloud --config /path/to/config.json --check-config` (in Docker, run it through Compose). Do not run `--init-config` again on an existing instance; it will not overwrite files or reset passwords.

Uploads are **public by default**. A private model belongs to a Cloud account and is available only to its owner; select the owner during upload. The picker supports multiple files. Even with remote Cloud, the picker opens on your computer, and only selected files are uploaded.

### R18 detection and blocking are separate

Configure these independently under “Models & content safety”:

- **Detection** checks model metadata and can optionally use NudeNet to inspect readable textures. It labels content R18, clean, or unclassified.
- **Blocking** prevents R18 models from regular player listings, downloads, and use.

Detection labels content but does not block it. Blocking alone has no detection result to act on. Both are off by default. Rescan existing models after enabling detection. Texture inspection needs a separate [NudeNet](https://github.com/notAI-tech/NudeNet) service. For this repository's Docker deployment, start it with `docker compose -f docker-compose.json.yml -f docker-compose.moderation.yml up -d`, then set the scanner URL to `http://nudenet:8080/infer` in settings. Encrypted `.ysm` files also need [YSMParser](https://github.com/OpenYSM/YSMParser). Detection can miss or misclassify content; manual review is available. You can separately block unclassified content. More settings are in the [admin deployment guide](deploy/ADMIN_JSON.md).

## 4. How players connect

Give players the HTTPS root address, for example `https://cloud.example.com`. They need a compatible SparkleMorpher version. In the mod, add the address, select the instance, and register or sign in. Players can link accounts they own at `/account/` without the owner doing it for them. Admin accounts are created and distributed by the owner. Microsoft web login requires the owner’s Microsoft application and Minecraft service approval. Until configured, use in-mod linking or an enabled third-party identity service.

## 5. Troubleshooting and recovery

- **Blank token box or unauthenticated:** it is a sign-in form and does not read the server file. Read the token on the server and paste the full line. Check that you are on the right instance. If missing, check the actual `--config` path and `auth.bootstrap_access_token_file`; do not delete the database or rerun initialization.
- **Forgot the owner password:** use the initial token to sign in and set a new password. If both are lost, stop the service and locate a secure backup. Do not delete or hand-edit the account database; follow the [deployment guide](deploy/ADMIN_JSON.md).
- **Cannot save settings:** confirm startup uses `--config` and the service account can write the config directory. Admins cannot change config. Listener, storage, public origin, and logging changes need restart. If WebUI was disabled, re-enable it in server JSON.
- **Site or certificate unavailable:** check DNS, inbound TCP 80/443, firewall, Caddy, and that the proxy domain matches the public origin. Port 8787 should only be reachable by the local proxy.
- **502 or upload fails:** check Cloud health and proxy upstream. The default per-model limit is 128 MiB; the proxy must allow at least that much.
- **Detection fails or models remain downloadable:** check detection and blocking separately, ensure Cloud can reach NudeNet, rescan old models, and verify players use the same instance.
- **Deleted model or upgrade trouble:** ordinary deletions can be restored; permanent cleanup cannot be undone in the WebUI. Back up config (including token), database, model objects, and Docker volumes before upgrades. Do not use `docker compose down -v` for routine upgrades.

## 6. Backups and more information

Before upgrading, back up the config directory, database, and model objects. Keep Docker named volumes. For native installs, stop Cloud before copying SQLite and its WAL files. Restore with a compatible version. The [admin deployment guide](deploy/ADMIN_JSON.md) covers permissions, proxies, scanning, upgrades, and rollback. Developers can see the [source and API](https://github.com/sdf123098/spm-cloud), [Compose config](docker-compose.json.yml), and [releases](https://github.com/sdf123098/spm-cloud/releases). License: MIT.
