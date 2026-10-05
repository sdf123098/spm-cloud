# SPM Cloud

SparkleMorpher 的独立 Rust 自建 Cloud 后端，使用与官方 Cloud 相同的客户端协议、登录、注册和游戏身份绑定流程。提供模型上传下载、权限、公开/私密、玩家模型与贴图同步、轮盘动作、停止动作和待机状态同步。源码和模组版本保持 2.0.0。

本教程提供三种方式：Docker Compose、Linux 原生、Windows 原生。选一种做即可。Cloud 是独立程序，可以与 Minecraft 服务器部署在同一台机器，也可以放在另一台机器；它通过 HTTPS 为玩家同步模型、贴图、轮盘动作和待机状态。参与同步的玩家需要安装对应版本的 SparkleMorpher，并选择同一个 Cloud 实例。

源码仓库：[sdf123098/spm-cloud](https://github.com/sdf123098/spm-cloud)。模组仓库：[sdf123098/Sparkle-Morpher](https://github.com/sdf123098/Sparkle-Morpher)。

## 1. 先准备这些东西

| 项目 | 你需要做什么 |
|---|---|
| 服务器 | 具备公网访问能力；Linux 教程以 Ubuntu/Debian 为例，需要 sudo 权限 |
| 域名 | 例如 `cloud.example.com`；把下文的示例域名全部换成自己的 |
| DNS | 域名的 A 记录指向服务器公网 IPv4；只有 IPv6 也可达时才添加 AAAA |
| 端口 | 放行 TCP 80、443，包括云服务器安全组、防火墙及路由器端口映射 |
| 已有网站 | 如果 80/443 已被其他网站占用，使用现有 HTTPS 反向代理，或为 Cloud 单独部署一台机器 |
| 玩家客户端 | 安装本次更新后的模组；服主升级服务不会自动升级玩家模组 |

Caddy 会根据域名申请和续期 HTTPS 证书，要求 DNS 与端口先准备好。[Caddy HTTPS 官方说明](https://caddyserver.com/docs/automatic-https)

对外给玩家的地址是 `https://cloud.example.com`，不要填写 `/v1`、`/health` 或内网的 8787 端口。8787 是后端内部 HTTP 端口，由 Caddy 转成 HTTPS/WSS；不需要开放给公网。

## 2. 方式一：Docker Compose

适合希望程序自动重启、数据目录由 Docker 管理的服主。无需在主机安装 Rust。下面命令在 Linux 终端运行；Windows/macOS 使用 Docker Desktop 的 Linux 容器模式，也可运行项目中的同一 Compose 配置。

### 2.1 安装 Docker

已安装 Docker 的先运行 `sudo docker compose version`；能显示版本就跳过安装。首次安装请按 [Docker Ubuntu 安装教程](https://docs.docker.com/engine/install/ubuntu/) 或 [Debian 安装教程](https://docs.docker.com/engine/install/debian/) 完成 Docker Engine 与 Compose plugin 安装。不要在已有 Docker 服务的机器上随意卸载旧软件。

检查安装与服务：

```bash
sudo systemctl enable --now docker
sudo docker compose version
sudo docker run --rm hello-world
sudo apt update
sudo apt install -y git openssl nano curl
```

### 2.2 下载源码、填写配置

```bash
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
cp .env.example .env
chmod 600 .env
openssl rand -hex 32
```

最后一条会生成随机管理密钥，把生成的整串文字填入 `.env` 的 `SPM_CLOUD_ACCESS_TOKEN`。然后运行 `nano .env`，至少设置以下内容（nano 中按 Ctrl+O、Enter 保存，Ctrl+X 退出）：

```dotenv
SPM_CLOUD_HOST=cloud.example.com
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_ACCESS_TOKEN=填入刚才生成的随机密钥
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_ALLOW_SELF_REGISTRATION=true
```

`SPM_CLOUD_HOST` 是域名，不带 `https://`；`SPM_CLOUD_ORIGIN` 是玩家实际访问的 HTTPS 根地址。实例 ID 使用字母、数字、点、下划线、短横线，并以字母或数字开头。实例 ID 与地址确定后，升级时继续保留它们。

管理密钥只供服主管理接口使用，不是玩家密码，不要发给玩家。`SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` 留空即可；玩家默认可以自行注册。只有需要给管理员 bootstrap 账号启用密码登录时，才填写真正的 Argon2id 密码摘要，不要填明文密码或示例 `$argon2id...`。

### 2.3 启动

在刚才的 `spm-cloud` 目录运行：

```bash
sudo docker compose up -d --build
sudo docker compose ps
sudo docker compose logs --tail=100 spm-cloud caddy
```

第一次会下载镜像并编译程序，耗时较长。看到容器运行后验证：

```bash
curl -fsS https://cloud.example.com/health
curl -fsS https://cloud.example.com/v1/instance
```

第一条应返回 JSON，第二条的 `instance_id` 应是自己的 ID，`capabilities` 应包含 `player_motion_v1` 和 `game_identity_auth_v1`。`auth.self_registration` 为 `true` 时允许玩家自行注册。

修改 `.env` 后，用 `sudo docker compose up -d --force-recreate spm-cloud` 让新环境变量生效；只运行 `restart` 不会更新容器的环境变量。Docker 模式下继续使用仓库自带的 `Caddyfile`，其上游是容器服务 `spm-cloud:8787`。

## 3. 方式二：Linux 原生运行（不使用 Docker）

以下是 Ubuntu/Debian 的完整流程。编译时需要 Rust 1.87 或更新版本、C 编译器；正式运行不需要 Rust、Node.js 或手动安装 SQLite/OpenSSL。首次编译在服务器上完成，得到该服务器平台的程序；不能把 Windows 的 `.exe` 改名后拿到 Linux 运行。

### 3.1 安装编译工具与 Rust

```bash
sudo apt update
sudo apt install -y build-essential ca-certificates curl git openssl nano
```

如果尚未安装 Rust，按 [Rust 官方安装页](https://rust-lang.org/tools/install/) 安装 rustup，使用普通用户进行安装与编译：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
sh rustup-init.sh -y --profile minimal
. "$HOME/.cargo/env"
rustc --version
```

已安装 Rust 的确认版本至少为 1.87；过旧时可执行 `rustup update stable`。本教程在 Linux 使用该设备的原生工具链，不要求安装 Windows GNU 或 MSYS2。

### 3.2 编译并安装程序

```bash
mkdir -p "$HOME/spm-build"
cd "$HOME/spm-build"
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
cargo build --release --locked
sudo install -m 755 target/release/spm-cloud /usr/local/bin/spm-cloud
```

只有最后一步需要 sudo。ARM64 服务器也应在自己的目标环境编译。第一次编译需下载依赖，服务器内存不足时可以改用 `cargo build --release --locked -j 1`。

首次部署创建服务专用用户和数据目录：

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
```

如果服务用户已存在，不要重复创建。

### 3.3 创建环境配置

```bash
openssl rand -hex 32
sudo touch /etc/spm-cloud.env
sudo chown root:root /etc/spm-cloud.env
sudo chmod 600 /etc/spm-cloud.env
sudo nano /etc/spm-cloud.env
```

填入下列内容，替换域名与随机密钥：

```dotenv
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_BIND=127.0.0.1:8787
SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data
SPM_CLOUD_DATABASE=/var/lib/spm-cloud/data/spm-cloud.db
SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
SPM_CLOUD_ACCESS_TOKEN=填入刚才生成的随机密钥
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_ALLOW_SELF_REGISTRATION=true
SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1
```

这里没有 `SPM_CLOUD_HOST`，因为 Linux 原生的域名写在 Caddy 配置中。后端自己不会读取目录里的 `.env`；下一步由 systemd 读取 `/etc/spm-cloud.env` 并把配置传给程序。

### 3.4 配置 systemd 开机自启

执行 `sudo nano /etc/systemd/system/spm-cloud.service`，填入：

```ini
[Unit]
Description=SparkleMorpher Cloud
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=spm-cloud
Group=spm-cloud
WorkingDirectory=/var/lib/spm-cloud
EnvironmentFile=/etc/spm-cloud.env
ExecStart=/usr/local/bin/spm-cloud
Restart=on-failure
RestartSec=5
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/spm-cloud

[Install]
WantedBy=multi-user.target
```

保存并启动：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud
sudo systemctl status spm-cloud --no-pager
curl -fsS http://127.0.0.1:8787/health
```

状态应显示 `active (running)`；内网 HTTP 健康检查成功后，再做 HTTPS。查看程序日志用 `sudo journalctl -u spm-cloud -n 100 --no-pager`。以后修改环境配置，执行 `sudo systemctl restart spm-cloud`。

### 3.5 安装 Caddy 并配置 HTTPS

下面采用 [Caddy 官方 Debian/Ubuntu 安装方式](https://caddyserver.com/docs/install)。已有 Caddy 的跳过安装，并在现有配置中增加 Cloud 站点。

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl gnupg
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install -y caddy
sudo nano /etc/caddy/Caddyfile
```

给自己的域名加入这个站点块，保留已有网站配置：

```caddyfile
cloud.example.com {
    reverse_proxy 127.0.0.1:8787 {
        header_up X-SPM-Client-IP {http.request.remote.host}
    }
}
```

保存并检查：

```bash
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl enable --now caddy
sudo systemctl reload caddy
curl -fsS https://cloud.example.com/health
curl -fsS https://cloud.example.com/v1/instance
```

Caddy 会代理 WebSocket，不需要另装插件。`X-SPM-Client-IP` 由 Caddy 覆盖，配合后端只信任 `127.0.0.1`，用于按实际玩家 IP 限制认证请求。原生模式必须使用这里的 `127.0.0.1:8787` 上游，不能直接套用 Docker 版的 `spm-cloud:8787`。

## 4. 方式三：Windows 原生运行

向模组作者获取 Windows 程序包 `spm-cloud-windows-x64.zip` 并解压，例如放到 `C:\SPMCloud`。也可自行在 Windows 克隆源码并执行 `cargo build --release --locked`，但需要该设备 Rust 工具链对应的 C 编译环境。Windows、Linux、macOS 分别使用自己的程序。

在 PowerShell 中运行（替换域名）：

```powershell
Set-Location C:\SPMCloud
$env:SPM_CLOUD_INSTANCE_ID = 'my-server-cloud'
$env:SPM_CLOUD_ORIGIN = 'https://cloud.example.com'
$env:SPM_CLOUD_BIND = '127.0.0.1:8787'
$env:SPM_CLOUD_DATA_DIR = 'C:\SPMCloud\data'
$env:SPM_CLOUD_OBJECT_DIR = 'C:\SPMCloud\objects'
$env:SPM_CLOUD_ACCESS_TOKEN = -join ((1..4 | ForEach-Object { [Guid]::NewGuid().ToString('N') }))
$env:SPM_CLOUD_BOOTSTRAP_ACCOUNT = 'account_local'
$env:SPM_CLOUD_ALLOW_SELF_REGISTRATION = 'true'
$env:SPM_CLOUD_TRUSTED_PROXY_IPS = '127.0.0.1'
.\spm-cloud.exe
```

运行时窗口会持续显示日志。第一次生成的管理密钥要保存到仅服主可读的启动配置中；后续启动继续使用同一个值。可以把配置和启动命令存为本地 `start-cloud.ps1`，并把随机生成表达式换成已保存的固定密钥。这个启动文件包含密钥，不要随教程发给玩家。程序不会自动读取解压目录中的 `.env`。

另开 PowerShell 验证：`Invoke-RestMethod http://127.0.0.1:8787/health`。随后从 [Caddy 官方下载页](https://caddyserver.com/download) 获取 Windows 版 `caddy.exe`，放入 `C:\SPMCloud`，创建 `Caddyfile.native`，内容使用上一节 Linux 原生的站点块。运行：

```powershell
Set-Location C:\SPMCloud
.\caddy.exe run --config .\Caddyfile.native --adapter caddyfile
```

保持两个程序运行，验证 `Invoke-RestMethod https://cloud.example.com/v1/instance`。关闭窗口会停止前台程序；需要开机自启时，用 Windows 任务计划程序分别启动后端脚本和 Caddy，配置“系统启动时”、程序实际工作目录、失败后重试以及适当的运行权限。长期无人值守也可选择 Docker Compose。

## 5. 让玩家连接、注册、绑定

1. 玩家更新 SparkleMorpher，打开模组的“账号管理 → Cloud 实例”，添加服主提供的 HTTPS 根地址；若界面要求实例 ID，填写 `/v1/instance` 返回的 ID。
2. 选中这个自建实例，再进入“Cloud 账号”。选择“游戏账号登录／创建 Cloud 账号”，或“使用现有账号／手动注册”。新注册通常需要在本实例单独创建账号。
3. 使用账号密码登录的玩家，继续点击“绑定当前游戏身份”，完成当前启动器账号的验证。只显示 Cloud 账号已连接，不能证明游戏身份已经绑定。
4. 已绑定后，下次可以使用游戏账号登录。截图中原有 `test` 账号无需重建，只要登录原账号后完成绑定。
5. 在资源站上传模型并设置为“公开”，选用它，让另一名使用同一 Cloud 的玩家观察模型、贴图、轮盘播放／停止和待机状态。

各 Cloud 的账号、密码、模型和绑定独立；官方 Cloud 的账号不会自动迁移到自建实例。同一个 Minecraft 服务器上的玩家如果选了不同 Cloud，无法互相看到对方在另一个实例发布的状态。私密模型不会向其他玩家公开模型或动作。

正版账号使用官方证书或 Session Service 挑战；第三方 Yggdrasil 账号必须由服主在“游戏身份管理”的登录提供方配置中启用可信服务，并与玩家启动器使用的认证服务对应。真正离线账号不能通过自报名字/UUID 变成已验证账号；高级离线审批也不等于普通玩家联机身份验证。

`SPM_CLOUD_ALLOW_SELF_REGISTRATION=true` 与官方默认流程一致。设为 `false` 后，服主需先通过管理接口创建玩家账号，玩家再登录、绑定；管理员创建账号的例子见仓库 README。

## 6. 备份和升级

需要同时保存数据库、模型对象和原来的配置。备份数据库前先停止后端，保留 WAL 等整个数据目录；仅保存一个 `.db` 文件可能缺少最近的写入。

### Docker 备份与升级

在项目目录运行，默认卷名是 `spm-cloud-data` 和 `spm-cloud-objects`；若修改过 `.env` 中的卷名，用自己的名称替换下面两处：

```bash
mkdir -p backup
chmod 700 backup
sudo docker compose stop spm-cloud
sudo docker run --rm -v spm-cloud-data:/from:ro -v "$PWD/backup:/to" alpine sh -c 'tar czf /to/data.tgz -C /from .'
sudo docker run --rm -v spm-cloud-objects:/from:ro -v "$PWD/backup:/to" alpine sh -c 'tar czf /to/objects.tgz -C /from .'
cp .env backup/instance.env
chmod 600 backup/instance.env
sudo docker compose start spm-cloud
```

另将备份复制到服务器之外。每次使用新的备份目录，避免覆盖上一份。升级先做备份，再执行：

```bash
git pull --ff-only
sudo docker compose up -d --build
curl -fsS https://cloud.example.com/v1/instance
```

现有网站与数据卷不用重建。不要执行 `docker compose down -v`，`-v` 会删除持久化卷。恢复时先停止后端，把两份 tar 恢复到两个新卷，将 `.env` 的 `SPM_CLOUD_DATA_VOLUME` 和 `SPM_CLOUD_OBJECTS_VOLUME` 改成新卷名，然后 `sudo docker compose up -d --force-recreate spm-cloud`；这样能保留原卷供核对。

### Linux 原生备份与升级

首次部署后，建议在自己的管理用户终端保存这个备份流程：

```bash
backup_file="$HOME/spm-cloud-backup-$(date +%Y%m%d-%H%M%S).tgz"
sudo systemctl stop spm-cloud
sudo tar -czf "$backup_file" -C / var/lib/spm-cloud etc/spm-cloud.env etc/systemd/system/spm-cloud.service etc/caddy/Caddyfile
sudo chmod 600 "$backup_file"
sudo systemctl start spm-cloud
```

归档包含管理密钥，仅服主保存；复制备份时使用 sudo 或调整归档所有者，不要公开放到网站下载目录。

升级时先在原源码目录编译，编译成功后再停机、执行上面的备份，并替换程序：

```bash
cd "$HOME/spm-build/spm-cloud"
git pull --ff-only
cargo build --release --locked
# 完成上面的停机备份后，再停机替换：
sudo systemctl stop spm-cloud
sudo cp /usr/local/bin/spm-cloud /usr/local/bin/spm-cloud.previous
sudo install -m 755 target/release/spm-cloud /usr/local/bin/spm-cloud
sudo systemctl start spm-cloud
sudo systemctl status spm-cloud --no-pager
curl -fsS https://cloud.example.com/v1/instance
```

恢复时停止后端，把当前 `/var/lib/spm-cloud` 移到另一个保留目录，再从备份恢复整个目录和配置；使用与备份相配的程序，恢复目录的 `spm-cloud` 所有权，执行 `sudo systemctl daemon-reload` 后启动。不要把旧 WAL 与另一份数据库混放，也不要用旧程序直接打开已经由新版升级的数据库。

Windows 原生升级也先停止程序，备份完整 `data`、`objects` 和启动配置，再替换 `.exe`，以原实例 ID、地址和数据目录重启。新版会增量升级数据库，正常升级不需要重传模型或重建账号。

## 7. 常见问题

| 现象 | 先检查这些地方 |
|---|---|
| HTTPS 打不开／证书申请失败 | DNS A/AAAA 是否正确，TCP 80/443 是否可达，域名拼写，是否有别的程序占端口；Docker 看 caddy 日志，原生看 `journalctl -u caddy` |
| Caddy 返回 502 | 后端是否运行；原生上游应为 `127.0.0.1:8787`，Docker 上游应为 `spm-cloud:8787` |
| Linux 后端启动后退出 | `journalctl -u spm-cloud` 检查环境文件、origin 格式、目录写权限；systemd 中必须有 `EnvironmentFile` |
| `Address already in use` | 8787 已被旧后端占用，或 80/443 已被现有网站占用；先识别运行程序再调整 |
| “账号已连接，请绑定当前游戏身份” | 在同一 Cloud 实例点击“绑定当前游戏身份”；检查当前游戏账号和认证提供方 |
| 绑定提示已属于其他账号 | 使用原绑定账号登录，或由服主核对绑定归属；不要反复注册新账号 |
| 玩家注册被拒绝 | 检查 `auth.self_registration` 和部署配置；显式关闭注册时需要管理员先创建账号 |
| 别人看不到模型或轮盘 | 双方模组和服主后端是否都更新，是否选中同一 Cloud，自己的游戏身份是否已验证，模型是否公开 |
| 公共目录没有结果 | 公共目录需要输入搜索词；模型是否公开、是否已上传到当前实例 |
| 上传大模型失败 | 默认单文件上限 128 MiB；需要更大时设置 `SPM_CLOUD_MAX_ASSET_BYTES`（字节数），Docker 还需把该变量显式传入 Compose 的 environment，检查现有代理的上传限制 |
| 修改 `.env` 后没变化 | Docker 用 `up -d --force-recreate`；原生程序不自动读取 `.env`，按 systemd/启动脚本重启 |

目前一个实例使用 SQLite 和本地对象目录，适合单服务器；不要让多台后端共享同一个 SQLite 文件。实际承载人数受机器、模型大小和上传频率影响，先用两名玩家完成验收，再扩大使用范围。

验收时，两名玩家同时使用自己的账号登录绑定、选中同一实例，检查公开模型、贴图、轮盘播放/停止、待机，以及重新进入世界后的同步。只检查 `/health` 成功不能代替这些游戏内验证。

## 开发与接口说明

运行数据采用 SQLite WAL 和本地对象目录；官方适配器使用 Worker + D1 + R2 + Durable Objects，两种后端的客户端接口一致。多实例扩容需要另行替换存储和事件协调，不能直接共享 SQLite。

### 关闭注册时，管理员创建玩家账号

在自己的 PowerShell 管理终端运行，替换管理员密钥和玩家初始密码；然后让玩家在模组中登录并绑定当前游戏身份：

```powershell
$env:SPM_CLOUD_ACCESS_TOKEN = '自己的管理密钥'
$headers = @{ Authorization = "Bearer $env:SPM_CLOUD_ACCESS_TOKEN" }
$body = @{ account_id = 'alice'; password = '替换为玩家初始密码' } | ConvertTo-Json
Invoke-RestMethod https://cloud.example.com/v1/accounts -Method Post -Headers $headers -ContentType 'application/json' -Body $body
```

Linux 管理终端的等价请求（先在本地设置自己的管理密钥）：

```bash
curl -fsS https://cloud.example.com/v1/accounts \
  -H "Authorization: Bearer $SPM_CLOUD_ACCESS_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"account_id":"alice","password":"替换为玩家初始密码"}'
```

管理密钥只保留在服主终端/配置中。账号密码只建立 Cloud 会话，游戏身份仍需完成挑战；注册不会额外授予身份验证、scope 管理或模型读取权限。

### 主要协议

| 功能 | 接口 |
|---|---|
| 健康/实例信息 | `GET /health`、`GET /v1/instance` |
| 注册/密码登录 | `POST /v1/accounts`、`POST /v1/sessions` |
| 会话刷新/退出 | `POST /v1/sessions/refresh`、`DELETE /v1/sessions/current` |
| 游戏登录 | `POST /v1/auth/login-challenges` 及其 `/complete` |
| 绑定游戏身份 | `POST /v1/auth/challenges` 及其 `/complete` |
| 身份/提供方 | `/v1/identities`、`/v1/identity-providers` |
| 模型目录/上传 | `GET/POST /v1/assets` |
| 玩家外观和动作 | `PUT /v1/players/me/appearance`、`POST /v1/players/appearances/query` |

`/v1/instance` 的能力包含 `player_motion_v1` 和 `game_identity_auth_v1`，`auth` 给出密码/游戏登录、绑定和当前注册策略。玩家 appearance 的可选 `motion` 与外观共用 `expected_revision` 事务；轮盘、停止、模型 Molang 设置与待机控制器状态通过同一个实例同步。没有动作时返回 `null`；省略/清空动作或清空模型会清空旧动作。私密、过期、未验证身份或不可用模型不会向别人返回外观与动作。

scope 内的高级离线策略包括 `STRICT_APPROVAL`（默认）、`CLAIM_CODE`、`FIRST_CLAIM` 与 `DISABLED`，用于审批/认领特定实体，不能用来绕过普通玩家的已验证身份要求。

### 编译与测试

Rust 最低版本 1.87；SQLite 使用 bundled，TLS 使用 Rustls，protoc 随构建依赖提供。测试额外需要 Node.js 22 或更新版本，用于只存在于测试中的 RSA 签名夹具；正式运行无需 Node.js。

```text
cargo fmt --check
cargo check --all-targets --all-features --locked
cargo test --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
```

本开发机通过不入库的 `rust-toolchain.toml` 与 `.cargo/config.toml` 强制 GNU Rust + MSYS2 UCRT64 GCC。其他设备和 CI 不强制 GNU/MSYS2，使用自己的原生工具链。显式 target 构建产物在 `target/<target>/release`，普通原生构建产物在 `target/release`。

CI 在 Ubuntu、Windows、macOS 上执行检查、测试、release 构建和实际 HTTP 契约，另检查 Linux Docker 构建；配置见 `.github/workflows/platforms.yml`。Windows 本机已通过 48 项 Rust 测试与 242 次 release HTTP 检查；这些结果不等于 Linux/macOS/systemd/容器在本机也实际运行过。

游戏证书验证始终使用内嵌/固定官方 HTTPS 信任根，不存在 ENV 或请求字段替换生产信任根的入口。官方公钥轮换可在 `cloudflare` 目录执行 `npm run refresh:mojang-keys` 后重新构建。官方适配器部署说明见 [cloudflare/README.md](cloudflare/README.md)，测试夹具说明见 [tests/README.md](tests/README.md)。

旧模型目录只读审计工具为 `tools/migrate_legacy.py`；生成审计清单不会自动导入模型、删除原文件或授予权限。
