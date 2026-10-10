# SPM Cloud

**SparkleMorpher 的自建 Rust Cloud 后端。**它使用与客户端相同的 Cloud 协议，支持模型上传和下载、公开/私密、已验证游戏身份、玩家外观、贴图和轮盘动作。玩家在模组里选择同一个 Cloud 实例即可互相同步。

> [English](README.md) | **中文**

后端独立于 Minecraft 运行，可以和 Minecraft 服务器放在同一台机器，也可以部署到单独的公网服务器。玩家需要更新 SparkleMorpher，并获得服主提供的实例 HTTPS 地址。

## 选择安装方式

| 方式 | 适合谁 | 初次安装 | 后续更新 |
|---|---|---|---|
| Docker Compose | 大多数 Linux 服务器；希望由容器管理数据卷和进程重启 | 克隆本仓库，由 Docker Compose 首次构建容器；主机不需要安装 Rust | 可选 Linux systemd 更新器下载并校验 amd64/arm64 预构建 Docker 镜像 |
| Linux 原生 | Ubuntu/Debian 服务器；希望用 systemd 运行服务 | 从 GitHub 最新稳定版下载 Linux 预构建安装器，支持 x86_64 和 ARM64 | 可选 systemd 定时器下载、校验预构建包，重启服务并在健康检查失败时回滚 |
| Windows 原生 | 不使用 Docker 的 Windows 服务器 | 运行 GitHub 最新稳定版 PowerShell 安装器，提供 x64 预构建程序 | 可选任务计划程序下载并校验预构建包，健康检查失败时恢复旧程序 |
| macOS 程序包 | Intel 或 Apple Silicon 上本地/手动运行 | 从 [GitHub Releases](https://github.com/sdf123098/spm-cloud/releases/latest) 下载对应架构压缩包 | 手动替换程序；下文自动更新教程覆盖 Linux 和 Windows |

下文发布下载链接均指向 GitHub **最新稳定版**，安装命令不需要修改版本号。需要旧版时，请从 [Releases 页面](https://github.com/sdf123098/spm-cloud/releases)选择指定版本。默认单模型上传上限为 **128 MiB**。JSON 运行时设置支持热加载，详见[JSON 配置和部署指南](deploy/ADMIN_JSON.md)。

## 部署前准备

| 项目 | 准备内容 |
|---|---|
| 域名 | 例如 `cloud.example.com`，DNS 指向服务器公网 IP |
| HTTPS | 放行 TCP 80 和 443；使用 Caddy，或把 Cloud 站点加入现有反向代理 |
| 后端端口 | 8787 保持内网可访问；代理通过 HTTP 连接，玩家只使用 HTTPS 地址 |
| 实例信息 | 选好固定实例 ID；升级时保留 ID、数据目录和管理密钥 |
| 玩家客户端 | 玩家安装匹配版本的 SparkleMorpher，并选择同一个 Cloud 实例 |

提供给玩家的地址是 HTTPS 根地址，例如 `https://cloud.example.com`，不要附加 `/v1`、`/health` 或 `:8787`。

## Docker Compose

首次 Compose 部署会从检出的仓库构建容器。以后可使用自动更新器下载 GitHub Release 中的预构建镜像，不必在服务器重复构建 Rust 后端。先按 Docker 官方说明安装 Docker Engine 和 Compose 插件：[Ubuntu](https://docs.docker.com/engine/install/ubuntu/) 或 [Debian](https://docs.docker.com/engine/install/debian/)。Windows/macOS 可通过 Docker Desktop 的 Linux 容器模式运行。

### 1. 下载仓库并填写配置

在 Docker 主机的终端执行：

```bash
git clone https://github.com/sdf123098/spm-cloud.git
cd spm-cloud
cp .env.example .env
chmod 600 .env
openssl rand -hex 32
nano .env
```

将域名和刚生成的随机字符串填入配置：

```dotenv
SPM_CLOUD_HOST=cloud.example.com
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_ACCESS_TOKEN=替换为刚生成的随机密钥
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_ALLOW_SELF_REGISTRATION=true
SPM_CLOUD_MAX_ASSET_BYTES=134217728
```

`SPM_CLOUD_HOST` 只填域名，不带 `https://`；`SPM_CLOUD_ORIGIN` 填玩家访问的 HTTPS 根地址。管理密钥由服主保管，不是玩家密码。默认上传上限已经是 128 MiB，最后一行可以省略。

### 2. 启动并检查

```bash
sudo docker compose config --quiet
sudo docker compose up -d --build
sudo docker compose ps
sudo docker compose logs --tail=100 spm-cloud caddy
curl -fsS https://cloud.example.com/health
curl -fsS https://cloud.example.com/v1/instance
```

检查 `/v1/instance` 返回的实例 ID 和能力。Compose 通过 Caddy 对外提供 HTTPS，不会公开映射 8787 端口。修改 `.env` 后重建后端容器，让新环境变量生效：

```bash
sudo docker compose up -d --force-recreate spm-cloud
```

单纯执行 `docker compose restart` 不会把新环境变量传给容器。

### 3. 可选：启用自动更新

更新器每 30 分钟检查 GitHub Release，按主机架构下载 amd64/arm64 Docker 镜像压缩包，校验 SHA-256 后加载镜像，并只更新后端服务。数据卷和 Caddy 保持运行。提供的 systemd 单元默认仓库位于 `/opt/spm-cloud`：

```bash
sudo apt install -y curl jq gzip coreutils util-linux
sudo cp deploy/spm-cloud-auto-update.service deploy/spm-cloud-auto-update.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-auto-update.timer
sudo systemctl start spm-cloud-auto-update.service
sudo systemctl list-timers spm-cloud-auto-update.timer
```

仓库不在 `/opt/spm-cloud` 时，启用前修改 service 和 timer 中的路径。手动更新前先备份，在仓库目录执行 `git pull --ff-only`，再执行 `sudo SPM_CLOUD_COMPOSE_DIR="$PWD" bash deploy/docker-auto-update.sh`；该脚本应用最新预构建镜像。

## Linux 原生

此方式使用预构建 Release 安装包和 systemd。运行时不需要 Rust 工具链、Node.js、SQLite 或 OpenSSL 软件包。下文以 Ubuntu/Debian 为例，程序安装到 `/usr/local/bin`。

### 1. 安装最新稳定版

```bash
sudo apt update
sudo apt install -y ca-certificates curl jq nano
curl --fail --location --proto '=https' --tlsv1.2 \
  https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
spm-cloud --version
```

安装器会按服务器架构选择 x86_64 或 ARM64 程序。它不会创建服务用户，也不会配置 HTTPS。

### 2. 创建服务账号和环境配置

首次部署时创建账号；已有账号请跳过 `useradd`。升级时继续使用原来的数据和对象目录：

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
sudo install -m 600 /dev/null /etc/spm-cloud.env
openssl rand -hex 32
sudo nano /etc/spm-cloud.env
```

将配置填写如下，并替换域名、实例 ID 和密钥：

```dotenv
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_BIND=127.0.0.1:8787
SPM_CLOUD_ACCESS_TOKEN=替换为生成并保存的随机管理密钥
SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data
SPM_CLOUD_DATABASE=/var/lib/spm-cloud/data/spm-cloud.db
SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1
SPM_CLOUD_MAX_ASSET_BYTES=134217728
```

默认上传上限为 128 MiB，`SPM_CLOUD_MAX_ASSET_BYTES` 可以省略。管理密钥应保密，后续升级保持同一个值。Rust 程序不会自动读取 `.env`；下一步的 systemd 服务会读取 `/etc/spm-cloud.env`。

### 3. 安装并启动 systemd 服务

从当前稳定版获取服务模板并设置开机启动：

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

用 `sudo journalctl -u spm-cloud -n 100 --no-pager` 查看日志。修改 `/etc/spm-cloud.env` 后运行 `sudo systemctl restart spm-cloud`。

### 4. 配置 HTTPS

按 [Caddy 官方说明](https://caddyserver.com/docs/install)安装 Caddy。编辑 `/etc/caddy/Caddyfile`，把下面站点加入现有配置，替换域名并保留已有网站：

```caddyfile
cloud.example.com {
    reverse_proxy 127.0.0.1:8787 {
        header_up X-SPM-Client-IP {http.request.remote.host}
    }
}
```

检查并重载：

```bash
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl enable --now caddy
sudo systemctl reload caddy
curl -fsS https://cloud.example.com/v1/instance
```

申请 HTTPS 证书前，DNS 必须指向服务器，80/443 端口必须可以从公网访问。Caddy 会代理 HTTPS 和 WebSocket；上面的代理头配合 `SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1` 传递玩家 IP。

### 5. 可选：启用 Linux 自动更新

systemd 定时器每 30 分钟检查一次。它下载匹配架构的预构建 Linux 包，校验 SHA-256 和版本，替换程序、重启服务并检查 `/health`；健康检查失败会恢复旧程序。后端正常运行后安装：

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

通过 `sudo journalctl -u spm-cloud-native-update.service` 查看更新记录。启用无人值守升级前备份数据库与模型对象；数据库迁移可能不能仅通过恢复旧程序回退。

## Windows 原生

PowerShell 安装器会下载 GitHub 最新稳定版的 Windows x64 预构建程序。使用日常运行后端的 Windows 账号安装：

```powershell
$env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
```

程序位于 `$env:USERPROFILE\.cargo\bin\spm-cloud.exe`。数据单独放在例如 `C:\SPMCloud` 的目录。创建并保存私有启动脚本 `C:\SPMCloud\start-cloud.ps1`：

```powershell
Set-Location C:\SPMCloud
$env:SPM_CLOUD_INSTANCE_ID = 'my-server-cloud'
$env:SPM_CLOUD_ORIGIN = 'https://cloud.example.com'
$env:SPM_CLOUD_BIND = '127.0.0.1:8787'
$env:SPM_CLOUD_DATA_DIR = 'C:\SPMCloud\data'
$env:SPM_CLOUD_OBJECT_DIR = 'C:\SPMCloud\objects'
$env:SPM_CLOUD_ACCESS_TOKEN = '替换为保存好的随机管理密钥'
$env:SPM_CLOUD_BOOTSTRAP_ACCOUNT = 'account_local'
$env:SPM_CLOUD_ALLOW_SELF_REGISTRATION = 'true'
$env:SPM_CLOUD_TRUSTED_PROXY_IPS = '127.0.0.1'
& "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
```

用 `-join (1..4 | ForEach-Object { [Guid]::NewGuid().ToString('N') })` 生成一次密钥，保存到启动脚本后妥善保管。原生 Windows 程序不会读取 `.env` 文件。

从 [caddyserver.com](https://caddyserver.com/download)下载 Caddy，将 `caddy.exe` 放进 `C:\SPMCloud`。新建 `Caddyfile`，内容与 Linux 一节的站点块相同，然后在两个终端分别运行后端脚本和 Caddy。先检查 `http://127.0.0.1:8787/health`，再检查 `https://cloud.example.com/v1/instance`。无人值守运行可在任务计划程序中设置两个程序开机启动。

### 可选：启用 Windows 自动更新

先在任务计划程序中创建名为 `SPM Cloud` 的后端任务，并确保它以安装程序所用的账号运行。下载最新更新脚本：

```powershell
$release = Invoke-RestMethod https://api.github.com/repos/sdf123098/spm-cloud/releases/latest
$url = "https://raw.githubusercontent.com/sdf123098/spm-cloud/$($release.tag_name)/deploy/windows-auto-update.ps1"
$updater = "$env:ProgramData\SPMCloud\windows-auto-update.ps1"
New-Item -ItemType Directory -Force (Split-Path $updater) | Out-Null
Invoke-WebRequest $url -OutFile $updater
```

新建一个每天运行的计划任务：程序设为 `powershell.exe`，参数为 `-NoProfile -ExecutionPolicy Bypass -File "C:\ProgramData\SPMCloud\windows-auto-update.ps1"`，运行账号与后端任务相同。更新器会校验预构建包的 SHA-256 和版本，停止后端任务、替换程序、重新启动并检查 `/health`；失败时恢复旧程序。手动运行更新器前先关闭交互方式启动的后端。

## 配置与 JSON 热加载

Docker 和简单原生部署可以使用环境变量。Rust 程序本身不会读取 `.env`，必须由 Compose、systemd 或 Windows 启动脚本传入配置。

也可以使用严格管理员 JSON：[Schema](config.schema.json)、[示例文件](config.example.json)、[完整部署指南](deploy/ADMIN_JSON.md)。JSON 文件每 2 秒检查一次。上传/请求限制、注册策略、身份提供方和视觉运行设置等运行时字段会热加载；监听地址、实例身份/地址、存储路径和日志设置需要重启。显式环境变量的优先级高于 JSON 中对应字段。

`limits.max_asset_bytes` / `SPM_CLOUD_MAX_ASSET_BYTES` 默认 `134217728` 字节（128 MiB）。上传更大的模型时，还需同步提高反向代理的请求体限制。JSON 中的上传上限修改后热加载；环境变量修改后需要重新创建容器或重启服务。

Linux 使用 JSON 配置时，路径为 `/etc/spm-cloud/config.json`。创建服务账号可访问的目录，再用已安装的程序初始化、编辑和检查：

```bash
sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
# 编辑 JSON：设置 HTTPS origin 和绝对存储路径
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --print-effective-config
```

Windows 使用 Windows 路径，并以运行后端的账号执行：

```powershell
$exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
$config = 'C:\SPMCloud\config.json'
New-Item -ItemType Directory -Force (Split-Path $config) | Out-Null
& $exe --init-config $config
# 编辑 JSON：设置 HTTPS origin 和存储路径
& $exe --config $config --check-config
& $exe --config $config --print-effective-config
```

初始化会在配置文件旁生成 `secrets/bootstrap-token.txt`。配置和 token 只能向服务账号开放，不要提供给玩家。`--print-effective-config` 会隐藏凭据。迁移到新的数据路径时，必须同时迁移原有数据库和模型对象，不能直接用空目录启动旧实例。

## 玩家连接和账号

1. 玩家更新 SparkleMorpher，打开模型面板的“实例管理”。
2. 点击“添加实例”，填写服主提供的 HTTPS 地址，例如 `https://cloud.example.com`；实例 ID 会由服务器返回。
3. 选中该实例，在账号管理页面注册或登录。每个 Cloud 实例的账号彼此独立。
4. 登录后点击“绑定当前游戏身份”完成游戏账号验证。Cloud 账号登录成功并不等于游戏身份已经绑定。
5. 上传模型并设为“公开”，让另一名也选择该实例的玩家测试。私密模型不会向其他玩家公开。

默认允许自行注册。关闭注册后，服主需要通过 `POST /v1/accounts` 创建玩家账号，见[接口说明](#接口与开发)。

## 可选：外置 Yggdrasil 登录

单一外置认证服务可以在 `.env`、`/etc/spm-cloud.env` 或 Windows 启动脚本中配置：

```dotenv
SPM_CLOUD_HAS_JOINED_URL=https://auth.example.com/all-in-one/hasJoined
```

填写完整 HTTPS `hasJoined` 地址。后端会追加 `username` 和 `serverId`，认证服务应返回经过验证的 Yggdrasil 玩家 ID 与名称。需要多个认证提供方或稳定的提供方 ID 时，配置 `SPM_CLOUD_IDENTITY_PROVIDERS` 或使用 JSON。更新外置认证服务时保留原提供方 ID，因为它关联已有账号身份。详见[JSON 部署指南](deploy/ADMIN_JSON.md)。

## 备份和升级

升级前备份配置、完整数据目录和模型对象目录。SQLite 数据库需在停止服务后备份，避免漏掉 WAL 中的写入。备份包含凭据和玩家数据，应限制访问。

| 部署方式 | 更新方法 | 必须保留的数据 |
|---|---|---|
| Docker Compose | 启用可选 systemd 更新器；手动更新可拉取仓库后运行 `deploy/docker-auto-update.sh`，使用最新预构建镜像 | Docker 数据/对象卷、`.env`、Caddy 数据/配置卷 |
| Linux 原生 | 使用 systemd 定时器；手动更新使用最新 shell 安装器并重启 `spm-cloud` | `/var/lib/spm-cloud`、`/etc/spm-cloud.env` 或 JSON 配置/token、systemd 覆盖配置、Caddy 配置 |
| Windows 原生 | 使用任务计划更新器；或停止程序后运行最新 PowerShell 安装器 | `data`、`objects`、启动脚本、Caddy 配置及 JSON/token 文件 |

不要在需要保留数据时运行 `docker compose down -v`。正常升级会保留账号和模型文件；数据库结构更新可能要求使用较新的程序。回滚时应恢复相匹配的数据/配置备份和兼容版本的程序。详细迁移与回滚说明见[管理员 JSON 部署文档](deploy/ADMIN_JSON.md)。

## 常见问题

| 问题 | 检查项 |
|---|---|
| HTTPS 无法访问或证书申请失败 | DNS 是否指向服务器，80/443 是否放行，Caddy 配置和日志 |
| Caddy 返回 502 | 后端健康状态；原生上游是 `127.0.0.1:8787`，Docker 上游是 `spm-cloud:8787` |
| Linux 原生服务退出 | `sudo journalctl -u spm-cloud -n 100`；origin 格式、环境文件、文件归属和存储目录权限 |
| 端口已被占用 | 确认哪个进程占用了 8787/80/443，再调整后端监听地址或代理配置 |
| 玩家不能登录或绑定 | 是否选择同一 Cloud；账号会话是否有效；是否完成游戏身份绑定；外置提供方是否匹配启动器 |
| 其他玩家看不到模型/动作 | 双方是否选择同一实例；身份是否验证；模型是否公开；客户端和后端是否为兼容版本 |
| 模型上传被拒 | 默认 128 MiB；检查 `limits.max_asset_bytes` 或 `SPM_CLOUD_MAX_ASSET_BYTES`，以及代理请求体限制 |
| JSON 修改没有生效 | 进程是否使用目标 `--config`；环境变量可能覆盖 JSON；需要重启的字段是否已重启 |
| `.env` 修改没变化 | Docker 需要重建容器；原生部署确认服务/启动脚本读取了文件并重启程序 |

一个实例使用 SQLite 和本地对象目录，不要让多个后端进程共用一个 SQLite 文件。用两名真实玩家完成验收；`/health` 只能证明服务可达。

## 接口与开发

Rust 后端使用 Axum、SQLite WAL 和本地对象存储；官方适配器使用 Cloudflare Worker、D1、R2 和 Durable Objects。客户端协议一致，但部署和存储方式不同。

| 功能 | 接口 |
|---|---|
| 健康与实例发现 | `GET /health`、`GET /v1/instance` |
| 注册与密码登录 | `POST /v1/accounts`、`POST /v1/sessions` |
| 刷新/退出 | `POST /v1/sessions/refresh`、`DELETE /v1/sessions/current` |
| 游戏登录与身份绑定 | `/v1/auth/login-challenges`、`/v1/auth/challenges` |
| 身份和认证提供方 | `/v1/identities`、`/v1/identity-providers` |
| 模型目录和上传 | `GET/POST /v1/assets` |
| 玩家外观和动作 | `PUT /v1/players/me/appearance`、`POST /v1/players/appearances/query` |

关闭自行注册后，服主可使用管理员 Bearer 创建玩家账号：

```bash
curl -fsS https://cloud.example.com/v1/accounts \
  -H "Authorization: Bearer $SPM_CLOUD_ACCESS_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"account_id":"alice","password":"替换为玩家初始密码"}'
```

Rust 开发需要 1.87 或更新版本。测试额外需要 Node.js 22+ 提供测试夹具；正式运行不需要 Node.js。构建和 CI 说明见[跨平台 CI 配置](.github/workflows/platforms.yml)，官方 Worker 部署见 [cloudflare/README.md](cloudflare/README.md)。

**许可证：** MIT。
