# SPM Cloud

**自建 Rust Cloud 后端。** [GitHub 最新稳定版](https://github.com/sdf123098/spm-cloud/releases/latest)提供 Linux、Windows、macOS 预构建包，以及 amd64/arm64 Docker 镜像。以下安装命令始终指向最新稳定版，不需要手动改版本号。JSON 运行时设置支持热加载，无需重启服务；默认单模型上传上限为 128 MiB。部署、配置与升级说明见[管理员 JSON 文档](deploy/ADMIN_JSON.md)。

> [English](README.md) | **中文**

SparkleMorpher 的独立 Rust 自建 Cloud 后端，使用与官方 Cloud 相同的客户端协议、登录、注册和游戏身份绑定流程。提供模型上传下载、权限、公开/私密、玩家模型与贴图同步、轮盘动作、停止动作和待机状态同步。支持与官方相同的客户端协议；运行中的 Rust 服务和官方 Worker 分别通过各自部署流程更新。

本教程提供三种方式：Docker Compose、Linux 原生、Windows 原生。选一种做即可。Cloud 是独立程序，可以与 Minecraft 服务器部署在同一台机器，也可以放在另一台机器；它通过 HTTPS 为玩家同步模型、贴图、轮盘动作和待机状态。参与同步的玩家需要安装对应版本的 SparkleMorpher，并选择同一个 Cloud 实例。

源码仓库：[sdf123098/spm-cloud](https://github.com/sdf123098/spm-cloud)。模组仓库：[sdf123098/Sparkle-Morpher](https://github.com/sdf123098/Sparkle-Morpher)。

## 配置文件

Rust 后端支持严格管理员 JSON，提供[编辑器 Schema](config.schema.json) 和[示例](config.example.json)。先运行 `spm-cloud --init-config C:/spm-cloud/config.json` 生成配置和持久 bootstrap token，调整对外 HTTPS 域名与存储路径，再运行 `spm-cloud --config C:/spm-cloud/config.json --check-config`。启动使用相同的 `--config` 参数。`--print-effective-config` 输出合并后的字段与来源，凭据脱敏；两个检查命令均在数据库初始化、账户创建和监听前退出。选中的 JSON 文件每 2 秒检查一次；上传上限（默认 128 MiB）、请求大小、自助注册、身份提供方及视觉运行设置会热加载，无需重启 `spm-cloud`。监听地址、实例身份、存储路径和日志设置仍需重启。

文件选择顺序为 `--config` → `SPM_CLOUD_CONFIG` → 工作目录中已存在的 `config.json`；没有选中文件继续使用旧环境变量方式。显式环境变量覆盖 JSON；JSON 相对路径以配置目录为基准，环境变量路径以工作目录为基准。生成的 `secrets/bootstrap-token.txt` 应限服务账号访问。bootstrap 密码只初始化缺失凭据，重启不覆盖既有密码。玩家显示状态、坐骑独立绑定及绑定实体 motion 可显式启用，默认关闭；投射物快照仍不可启用。新客户端已协商查询限制，服务旧客户端时维持 `max_entity_query_count` 默认 64。完整配置和部署说明见 [JSON 管理](deploy/ADMIN_JSON.md)。

| 部署方式 | 配置文件 | 加载方式 |
|---|---|---|
| Docker Compose | [.env.example](.env.example) 复制为 `.env` | Compose 将配置传给后端与 Caddy |
| Linux 原生 | [.env.native.example](.env.native.example) 复制为 `/etc/spm-cloud.env` | [deploy/spm-cloud.service](deploy/spm-cloud.service) 通过 systemd 读取 |
| Windows 原生 | 固定保存的 PowerShell 启动脚本 | 启动前设置 `$env:SPM_CLOUD_...` |
| 原生 HTTPS 代理 | [deploy/Caddyfile.native](deploy/Caddyfile.native) | 编辑域名后由 Caddy 读取 |

Rust 程序本身不自动读取 `.env`。升级时保留实例 ID、origin、提供方 ID、数据路径和管理密钥。官方 Worker 的代码与 Rust 共享协议能力，部署步骤见 [cloudflare/README.md](cloudflare/README.md)。

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

适合希望程序自动重启、数据目录由 Docker 管理的服主。首次启动使用仓库 Dockerfile 在主机上构建；无需在主机安装 Rust。之后可启用更新器，它从 GitHub 下载已构建并带校验和的 Docker 镜像，不会在服务器重新编译。下面命令在 Linux 终端运行；Windows/macOS 使用 Docker Desktop 的 Linux 容器模式。

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
sudo docker compose config --quiet
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

### 启用 GitHub 版本自动更新（可选）

可选启用 systemd 定时器，每 30 分钟检查 GitHub 最新稳定版。更新器下载对应架构的预构建镜像包、核验 SHA-256 后切换容器，不在服务器编译源码；数据卷和 Caddy 不变。支持 amd64 与 arm64。要求项目目录为 `/opt/spm-cloud`，且主机已安装 Docker Compose、`curl`、`jq`、`gzip`、`sha256sum` 和 `flock`：

```bash
sudo apt install -y curl jq gzip coreutils util-linux
sudo cp deploy/spm-cloud-auto-update.service deploy/spm-cloud-auto-update.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-auto-update.timer
sudo systemctl start spm-cloud-auto-update.service
sudo systemctl list-timers spm-cloud-auto-update.timer
```

如果项目目录不是 `/opt/spm-cloud`，请先修改 service 文件中的 `SPM_CLOUD_COMPOSE_DIR` 与脚本路径再安装。定时器首次运行会更新到最新 Release。Linux 原生部署也可安装下文的 systemd 更新器；Windows 原生部署可用下文的计划任务脚本更新。

## 3. 方式二：Linux 原生运行（不使用 Docker）

以下是 Ubuntu/Debian 的完整流程。Release 提供 x86_64 和 ARM64 预构建包；服务器无需安装 Rust 或编译器。运行时也不需要 Node.js 或手动安装 SQLite/OpenSSL。

### 3.1 安装并检查预构建程序

```bash
sudo apt update
sudo apt install -y ca-certificates curl jq nano
curl --fail --location --proto '=https' --tlsv1.2 \
  "https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh" \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
spm-cloud --version
```

安装器会根据服务器架构选择二进制，并将 `spm-cloud` 放入 `/usr/local/bin`。下文的 systemd 更新器会下载并校验预构建程序包后再替换它。

### 3.2 创建服务用户与数据目录

首次部署创建服务专用用户和数据目录：

```bash
sudo useradd --system --user-group --home-dir /var/lib/spm-cloud --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
```

如果服务用户已存在，不要重复创建。

### 3.3 创建环境配置

```bash
openssl rand -hex 32
sudo install -m 600 /dev/null /etc/spm-cloud.env
sudo nano /etc/spm-cloud.env
```

将 `/etc/spm-cloud.env` 填成下面的内容，替换域名、实例 ID 和密钥：

```dotenv
SPM_CLOUD_INSTANCE_ID=my-server-cloud
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_BIND=127.0.0.1:8787
SPM_CLOUD_DATA_DIR=/var/lib/spm-cloud/data
SPM_CLOUD_DATABASE=/var/lib/spm-cloud/data/spm-cloud.db
SPM_CLOUD_OBJECT_DIR=/var/lib/spm-cloud/objects
SPM_CLOUD_ACCESS_TOKEN=填入随机管理密钥
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_ALLOW_SELF_REGISTRATION=true
SPM_CLOUD_TRUSTED_PROXY_IPS=127.0.0.1
SPM_CLOUD_MAX_ASSET_BYTES=134217728
```

这里没有 `SPM_CLOUD_HOST`，因为 Linux 原生的域名写在 Caddy 配置中。后端自己不会读取 `.env` 文件；下一步由 systemd 读取 `/etc/spm-cloud.env` 并把配置传给程序。

外置认证配置适用于三种部署方式，见下文“外置登录配置”。


### 3.4 配置 systemd 开机自启

从当前稳定版获取 systemd 服务模板并安装：

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

服务通过 `EnvironmentFile=/etc/spm-cloud.env` 读取配置，工作目录为 `/var/lib/spm-cloud`。日志用 `sudo journalctl -u spm-cloud -n 100 --no-pager` 查看；修改配置后执行 `sudo systemctl restart spm-cloud`。

### 3.5 启用 Linux 原生自动更新

每 30 分钟检查一次 GitHub 最新 Release，下载对应架构的预构建包，核对 SHA-256 和程序版本后原子替换二进制，重启服务并检查 `/health`。启动检查失败会恢复之前的程序。服务正常运行后执行：

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

默认健康检查地址是 `http://127.0.0.1:8787/health`。如果后端使用其他本机地址，在 `/etc/spm-cloud-auto-update.env` 中设置 `SPM_CLOUD_AUTO_UPDATE_HEALTH_URL`。查看运行记录：`sudo journalctl -u spm-cloud-native-update.service`。

### 3.6 安装 Caddy 并配置 HTTPS

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

把以下站点加入 Caddyfile，替换成自己的域名并保留已有网站配置：

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

使用 Release 提供的 Windows x64 PowerShell 安装器；它会下载预构建程序，无需安装 Rust：

```powershell
$env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
```

程序位于 `$env:USERPROFILE\.cargo\bin`；数据仍放在例如 `C:\SPMCloud` 的独立目录。

在 PowerShell 中运行（替换域名）：

```powershell
Set-Location C:\SPMCloud
$env:SPM_CLOUD_INSTANCE_ID = 'my-server-cloud'
$env:SPM_CLOUD_ORIGIN = 'https://cloud.example.com'
$env:SPM_CLOUD_BIND = '127.0.0.1:8787'
$env:SPM_CLOUD_DATA_DIR = 'C:\SPMCloud\data'
$env:SPM_CLOUD_OBJECT_DIR = 'C:\SPMCloud\objects'
$env:SPM_CLOUD_ACCESS_TOKEN = '替换为已保存的随机管理密钥'
$env:SPM_CLOUD_BOOTSTRAP_ACCOUNT = 'account_local'
$env:SPM_CLOUD_ALLOW_SELF_REGISTRATION = 'true'
$env:SPM_CLOUD_TRUSTED_PROXY_IPS = '127.0.0.1'
& "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
```

先用 `-join (1..4 | ForEach-Object { [Guid]::NewGuid().ToString('N') })` 生成一次密钥，固定保存到仅服主可读的启动配置中，后续使用同一个值。把上述配置和启动命令存为 `start-cloud.ps1`，运行时窗口会持续显示日志。这个文件包含密钥，不要随教程发给玩家；程序不会自动读取解压目录中的 `.env`。

另开 PowerShell 验证：`Invoke-RestMethod http://127.0.0.1:8787/health`。随后从 [Caddy 官方下载页](https://caddyserver.com/download) 获取 Windows 版 `caddy.exe`，放入 `C:\SPMCloud`，创建 `Caddyfile.native`，内容使用上一节 Linux 原生的站点块。运行：

```powershell
Set-Location C:\SPMCloud
.\caddy.exe run --config .\Caddyfile.native --adapter caddyfile
```

保持两个程序运行，验证 `Invoke-RestMethod https://cloud.example.com/v1/instance`。关闭窗口会停止前台程序；需要开机自启时，用 Windows 任务计划程序分别启动后端脚本和 Caddy，配置“系统启动时”、程序实际工作目录、失败后重试以及适当的运行权限。长期无人值守也可选择 Docker Compose。

如需 Windows 原生自动更新，请先将后端启动脚本配置为名为 `SPM Cloud` 的计划任务，并使用安装程序的同一 Windows 账号运行。下载当前稳定版的更新脚本：

```powershell
$release = Invoke-RestMethod https://api.github.com/repos/sdf123098/spm-cloud/releases/latest
$url = "https://raw.githubusercontent.com/sdf123098/spm-cloud/$($release.tag_name)/deploy/windows-auto-update.ps1"
$updater = "$env:ProgramData\SPMCloud\windows-auto-update.ps1"
New-Item -ItemType Directory -Force (Split-Path $updater) | Out-Null
Invoke-WebRequest $url -OutFile $updater
```

在任务计划程序中新增一个每天运行的任务，程序设为 `powershell.exe`，参数为 `-NoProfile -ExecutionPolicy Bypass -File "C:\ProgramData\SPMCloud\windows-auto-update.ps1"`，运行账号选后端的同一个账号。脚本会下载 Windows 预构建包、校验 SHA-256 和版本，停止 `SPM Cloud` 后端任务后替换程序，再启动任务并检查 `/health`；检查失败会恢复旧程序。若后端由当前桌面窗口直接运行，更新前需先关闭它。

## 外置登录配置：自建 Yggdrasil / all-in-one

在环境配置中增加一行即可启用外置认证提供方 `custom`：

```dotenv
SPM_CLOUD_HAS_JOINED_URL=https://auth.example.com/all-in-one/hasJoined
```

填写完整的 HTTPS `hasJoined` 地址，无需拆分根地址和接口路径。Cloud 自动追加 `username` 和 `serverId` 参数；服务应返回 Yggdrasil 格式的玩家 `id` 和 `name`。all-in-one 网关只需配置一次，由网关验证其支持的账号。玩家令牌仍只交给启动器的 Session Service，Cloud 不接收游戏令牌。

Linux 原生部署修改 `/etc/spm-cloud.env` 后重启服务；Docker 部署修改 `.env` 后重新创建容器。需要多个认证服务、保留已有提供方 ID，或关闭某个提供方时，改用 JSON 数组（两个配置不能同时设置）：

```dotenv
SPM_CLOUD_IDENTITY_PROVIDERS='[{"provider_id":"my-auth","display_name":"我的外置登录","has_joined_url":"https://auth.example.com/all-in-one/hasJoined","enabled":true}]'
```

提供方 ID 是已绑定身份的命名空间，更新已有服务时应保持原 ID；网关也必须避免其代理的不同用户发生 UUID 冲突。这些配置在启动时写入数据库，删除环境变量不会删除已有提供方；停用时将 `enabled` 设为 `false`。标准 Yggdrasil 服务仍可使用 `base_url` 加可选的 `session_path`，例如 `https://auth.example.com/api/yggdrasil` 默认追加 `/sessionserver/session/minecraft/hasJoined`。

管理员也可使用管理员 Bearer 调用 `POST /v1/identity-providers`，正文与上面的单个 JSON 对象相同。Rust 与 Worker 均支持 `has_joined_url`，它和 `base_url` / `session_path` 二选一；URL 不应包含凭据、查询串或片段。玩家不能自行添加可信认证服务。首次使用需在模组的 Cloud 账号页连接并完成身份绑定；已绑定身份随后自动登录。官方 Cloud 的第三方服务由官方运营者配置，自建实例的配置不会同步到官方实例。

Windows 在启动脚本中增加 `$env:SPM_CLOUD_HAS_JOINED_URL = 'https://auth.example.com/all-in-one/hasJoined'`。验证地址必须是公网 HTTPS 域名，不含非默认端口、凭据、查询串或片段；保留已有提供方 ID，不能替换固定的 `official` 提供方。

## 5. 让玩家连接、注册、绑定

1. 玩家更新 SparkleMorpher，打开模型面板的“实例管理”标签，点击“添加实例”。在子页面填写服主提供的 HTTPS 根地址，名称可以留空；实例 ID 会自动读取，无需手动填写。模型页 Cloud 标签旁的“＋”也能打开同一页面。
2. 选中这个自建实例，点击“管理账号”，或切换到“账号管理”标签。可以使用游戏账号登录／创建 Cloud 账号，也可以点击“登录”或“注册”，在子页面填写账号、密码；注册成功会自动登录。各实例的账号独立，官方账号不会自动成为自建实例的账号。
3. 使用账号密码登录的玩家，继续点击“绑定当前游戏身份”，完成当前启动器账号的验证。只显示 Cloud 账号已连接，不能证明游戏身份已经绑定。
4. 已绑定后，客户端自动恢复登录，失败后间隔重试，切换 Cloud 后重新尝试；手动退出会停止自动恢复，重新选择实例后恢复。自动恢复不创建新账号。已有账号可直接登录并绑定，无需重建。
5. 在资源站上传模型并设置为“公开”，选用它，让另一名使用同一 Cloud 的玩家观察模型、贴图、轮盘播放／停止和待机状态。

各 Cloud 的账号、密码、模型和绑定独立；官方 Cloud 的账号不会自动迁移到自建实例。同一个 Minecraft 服务器上的玩家如果选了不同 Cloud，无法互相看到对方在另一个实例发布的状态。私密模型不会向其他玩家公开模型或动作。

正版账号使用官方证书或 Session Service 挑战；第三方 Yggdrasil 账号必须由服主通过上述环境变量或管理员接口启用可信服务，并与玩家启动器使用的认证服务对应。真正离线账号不能通过自报名字/UUID 变成已验证账号；高级离线审批也不等于普通玩家联机身份验证。

`SPM_CLOUD_ALLOW_SELF_REGISTRATION=true` 与官方默认流程一致。设为 `false` 后，服主需先通过管理接口创建玩家账号，玩家再登录、绑定；管理员创建账号的例子见仓库 README。

社区“全部模型”显示当前账号可访问的模型，不要求搜索词；最近使用、收藏、我的模型和公开模型各有独立页面。单独的公开模型页仍需输入搜索词，只有所有者能修改公开/私有。

## 配置项参考

以下默认值来自 Rust 程序。Compose 对监听地址、容器存储与代理 IP 另有固定配置。

| 变量 | 默认值 | 作用 |
|---|---|---|
| `SPM_CLOUD_INSTANCE_ID` | `local-dev` | 固定实例 ID；模板用 `self-hosted` |
| `SPM_CLOUD_ORIGIN` | `https://localhost` | 对外 HTTPS 根地址 |
| `SPM_CLOUD_BIND` | `127.0.0.1:8787` | HTTP 监听地址；Compose 固定 `0.0.0.0:8787` |
| `SPM_CLOUD_DATA_DIR` | `data` | 数据目录 |
| `SPM_CLOUD_DATABASE` | `<DATA_DIR>/spm-cloud.db` | SQLite 数据库 |
| `SPM_CLOUD_OBJECT_DIR` | `<DATA_DIR>/objects` | 模型对象目录；模板用 `objects` |
| `SPM_CLOUD_ACCESS_TOKEN` | 未设置 | 管理员 Bearer 密钥 |
| `SPM_CLOUD_BOOTSTRAP_ACCOUNT` | `account_local` | bootstrap 账号 ID |
| `SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` | 未设置 | 可选 Argon2id 密码摘要 |
| `SPM_CLOUD_ALLOW_SELF_REGISTRATION` | `true` | `true` 或 `false` |
| `SPM_CLOUD_TRUSTED_PROXY_IPS` | 空 | 逗号分隔的可信代理 IP |
| `SPM_CLOUD_HAS_JOINED_URL` | 未设置 | 单一完整接口，提供方 `custom` |
| `SPM_CLOUD_IDENTITY_PROVIDERS` | 未设置 | 提供方 JSON 数组 |
| `SPM_CLOUD_MAX_ASSET_BYTES` | `134217728` | 单模型字节数，允许 1 字节至 4 GiB |
| `SPM_CLOUD_MAX_MESSAGE_BYTES` | `65536` | 协议消息字节数，允许 1 KiB 至 1 MiB |

仅 Compose 使用的变量：`SPM_CLOUD_HOST`、`SPM_CLOUD_DATA_VOLUME`、`SPM_CLOUD_OBJECTS_VOLUME`、`SPM_CLOUD_NETWORK_SUBNET` 和 `SPM_CLOUD_PROXY_IP`。网络冲突时一起修改子网与代理 IP。

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

推荐使用 GitHub 最新稳定版的预构建安装包升级，不需要在服务器安装 Rust 或编译源码。先完成上面的停机备份，再运行：

```bash
curl --fail --location --proto '=https' --tlsv1.2 \
  https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
  --output /tmp/spm-cloud-installer.sh
sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
sudo systemctl restart spm-cloud
sudo systemctl status spm-cloud --no-pager
curl -fsS https://cloud.example.com/v1/instance
```

安装器按服务器架构下载最新稳定版并安装到 `/usr/local/bin/spm-cloud`。如已启用 Linux 原生自动更新器，日常可让 systemd 定时器自动完成下载、校验、替换和健康检查；手动升级前可先停止定时器，避免同时更新。

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
| 第三方无法登录 | 检查提供方是否启用、hasJoined 地址是否完整、是否对应当前启动器、是否已完成首次绑定 |
| 绑定提示已属于其他账号 | 使用原绑定账号登录，或由服主核对绑定归属；不要反复注册新账号 |
| 玩家注册被拒绝 | 检查 `auth.self_registration` 和部署配置；显式关闭注册时需要管理员先创建账号 |
| 别人看不到模型或轮盘 | 双方模组和服主后端是否都更新，是否选中同一 Cloud，自己的游戏身份是否已验证，模型是否公开 |
| 公共目录没有结果 | 公共目录需要输入搜索词；模型是否公开、是否已上传到当前实例 |
| 上传大模型失败 | 默认单文件上限 128 MiB；检查反向代理的请求体限制。JSON 配置的 `limits.max_asset_bytes` 修改后热加载；环境变量 `SPM_CLOUD_MAX_ASSET_BYTES` 修改后 Docker 需重建容器，Linux systemd 需重启服务 |
| JSON 配置修改后没变化 | 确认进程启动时使用了目标 `--config` 文件；有效运行时字段约 2 秒内热加载，监听、实例身份、存储路径和日志设置仍需重启 |
| 修改 `.env` 后没变化 | Docker 用 `up -d --force-recreate`；原生程序不自动读取 `.env`，检查 systemd/启动脚本是否加载了环境变量并重启 |

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

CI 在 Ubuntu、Windows、macOS 上执行检查、测试、release 构建和实际 HTTP 契约，另检查 Linux Docker 构建；配置见 `.github/workflows/platforms.yml`。各平台 CI 的运行结果应以对应提交的检查页面为准；本地 Windows 检查不代表 Linux/macOS/systemd/容器已在本机运行。

游戏证书验证始终使用内嵌/固定官方 HTTPS 信任根，不存在 ENV 或请求字段替换生产信任根的入口。官方公钥轮换可在 `cloudflare` 目录执行 `npm run refresh:mojang-keys` 后重新构建。官方适配器部署说明见 [cloudflare/README.md](cloudflare/README.md)，测试夹具说明见 [tests/README.md](tests/README.md)。

旧模型目录只读审计工具为 `tools/migrate_legacy.py`；生成审计清单不会自动导入模型、删除原文件或授予权限。

**许可证：** MIT。
