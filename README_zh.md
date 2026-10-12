# SPM Cloud 2.2 使用指南

**SPM Cloud 是 SparkleMorpher 模组使用的自建云服务。**服主运行服务并管理模型；玩家在模组里填写同一个 Cloud 地址后，可以共享公开模型、外观和动作。

> [English](README.md) | **中文**
>
> 截图里的“初始访问令牌”框留空是正常的：令牌保存在运行 Cloud 的服务器上，需要从服务器文件复制后粘贴。浏览器不会自动读取远程服务器文件。

## 先分清三种凭据

- **初始访问令牌**：服主第一次登录管理网页用的最高权限密钥，由 `spm-cloud --init-config` 生成并保存在单独文本文件中。它不是 Minecraft 令牌，也不是玩家密码。
- **Cloud 账号和密码**：在某一个 Cloud 实例注册、登录和拥有私有模型使用。不同实例的账号互不相通。
- **游戏账号凭据**：用于验证正版 Minecraft 或服主启用的第三方身份。只在对应官方或第三方登录页面输入，不要发给服主。

初始令牌不会传到浏览器。远程 WebUI 不能读取服务器磁盘；登录服务器后，从令牌文件复制内容，再粘贴到网页。

## 1. 部署 Cloud

### 推荐：Linux Docker Compose

准备 Linux 服务器、指向服务器的域名，放行 TCP 80/443，安装 Docker Engine 和 Compose 插件，然后运行：

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

这会生成配置 `deploy/runtime/config.json` 和令牌 `deploy/runtime/secrets/bootstrap-token.txt`。编辑配置，将示例域名换成自己的 HTTPS 域名并填写唯一实例 ID；将 `Caddyfile.compose` 里的 `cloud.example.com` 换成同一域名，然后启动：

```bash
sudo nano deploy/runtime/config.json
sudo nano Caddyfile.compose
sudo docker compose -f docker-compose.json.yml up -d
```

检查状态：`sudo docker compose -f docker-compose.json.yml ps`。玩家使用 HTTPS 根地址，如 `https://cloud.example.com`，不要添加端口、`/admin/` 或 `/v1`。升级时不要运行 `docker compose down -v`，它会删除数据卷。

### Linux 或 Windows 原生安装

完整步骤见[部署指南](deploy/ADMIN_JSON.md)。Linux 默认配置为 `/etc/spm-cloud/config.json`，以服务账号初始化：

```bash
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
```

Windows 使用 PowerShell 安装器安装程序（示例把程序装到当前用户目录），并在 PowerShell 中初始化（示例使用 `C:\SPMCloud`）：

```powershell
$env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
$exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
$config = "C:\SPMCloud\config.json"
New-Item -ItemType Directory -Force C:\SPMCloud | Out-Null
Set-Location C:\SPMCloud
& $exe --init-config $config
```

Windows 示例令牌在 `C:\SPMCloud\secrets\bootstrap-token.txt`。编辑配置中的域名、实例 ID 和存储路径，再以 `& $exe --config C:\SPMCloud\config.json` 启动。长期运行、自动启动和 HTTPS 反代步骤见[部署指南](deploy/ADMIN_JSON.md)。不要将 8787 后端端口直接暴露给互联网。

## 2. 找到并使用初始令牌

令牌文件是一行以 `spm_bootstrap_` 开头的长字符串：

| 部署方式 | 默认令牌文件 | 查看方法 |
|---|---|---|
| Docker Compose | 仓库目录下 `deploy/runtime/secrets/bootstrap-token.txt` | 在仓库目录执行 `sudo cat deploy/runtime/secrets/bootstrap-token.txt` |
| Linux 原生 | `/etc/spm-cloud/secrets/bootstrap-token.txt` | 执行 `sudo cat /etc/spm-cloud/secrets/bootstrap-token.txt` |
| Windows 示例 | `C:\SPMCloud\secrets\bootstrap-token.txt` | PowerShell：`Get-Content 'C:\SPMCloud\secrets\bootstrap-token.txt'` |
| 预构建包启动脚本 | 配置文件所在目录下的 `secrets/bootstrap-token.txt` | 用文本编辑器打开 |

自定义配置路径时，令牌默认在配置文件旁的 `secrets/bootstrap-token.txt`。若 JSON 的 `auth.bootstrap_access_token_file` 改过，则按该路径找；相对路径以 JSON 所在目录为基准。Docker 容器内的 `/etc/spm-cloud/secrets/bootstrap-token.txt` 对应主机挂载配置目录中的 `secrets/bootstrap-token.txt`。

1. 打开 `https://你的域名/admin/`。
2. 从服务器令牌文件复制整行，粘贴到“初始访问令牌”框并登录。
3. 进入“账号与权限”，在服主账号行选择“设置服主密码”。
4. 之后可用服主账号 ID 和密码登录。初始令牌仍然有效，请继续保管。

令牌保存在服务器文件中，不会出现在初始化命令输出，也不会被远程网页自动填入。不要把它放进 JSON、启动命令、聊天、截图或公开日志。Linux 初始化会限制文件权限；Windows 上限制为 Cloud 运行账号和服主可读。将配置和令牌一并放入受保护的离线备份。

## 3. 使用 WebUI

WebUI 已内置，不需另装网页或 Node.js。访问：

- `/admin/`：服主和服主创建、分发的管理员。
- `/account/`：玩家管理自己的 Cloud 账号和绑定。

服主可以改设置、管理账号和模型。管理员由服主创建并安全分发账号和初始密码；管理员可管理模型、查看设置，不能修改服务配置或创建账号。普通玩家无管理权限。重置密码、停用账号或撤销管理员角色会让其现有登录失效。

侧栏固定在一边，设置区和模型列表可用鼠标滚轮独立上下滚动。模型库支持搜索、分页、网格/3D 卡片、多选和批量操作。点模型名称看详情；选中后可批量改公开/私有、重新识别、人工标记、删除或恢复。普通删除可恢复，原文件会保留；永久清理前先备份。上传默认**公开**。私有模型归属某个 Cloud 账号，只允许所有者使用，上传时需选择归属账号。文件选择器支持多选；即使 Cloud 在远程服务器，选择窗口仍由操作者自己的电脑打开，只有选中的文件会上传。

设置页按“模型与内容安全、账号与权限、服务访问、维护与清理”展示日常选项；服务器路径、代理 IP、协议细节在高级设置。保存配置要求程序以 `--config 路径` 启动，且运行账号能写配置目录；部分选项需重启。JSON 编辑器会校验内容。配置导致无法启动时，从备份恢复，再执行 `spm-cloud --config 配置路径 --check-config`（Docker 用 Compose 执行检查）。不要对已有实例重复运行 `--init-config`：它不会覆盖文件，也不能重置密码。

### R18 识别与阻止是两个开关

在“模型与内容安全”分别设置：

- **识别**：读取模型元数据；可选接入 NudeNet 检查可读取的贴图，标记为 R18、清洁或未识别。
- **阻止**：阻止判定为 R18 的模型出现在普通玩家目录、下载和使用入口。

只开识别会标记但不拦截；只开阻止没有识别结果可供判断。默认都关闭。启用识别后，对已有模型执行“重新识别”。贴图识别需单独运行 [NudeNet](https://github.com/notAI-tech/NudeNet)；在本仓库 Docker 部署中，可用 `docker compose -f docker-compose.json.yml -f docker-compose.moderation.yml up -d` 启动识别服务，并在设置中把扫描服务地址设为 `http://nudenet:8080/infer`。加密 `.ysm` 还需配置 [YSMParser](https://github.com/OpenYSM/YSMParser)。检测可能漏判或误判，可人工复核；还可独立选择是否阻止未识别内容。更多配置见[管理员部署指南](deploy/ADMIN_JSON.md)。

## 4. 玩家连接

把 HTTPS 根地址发给玩家，如 `https://cloud.example.com`。玩家需使用兼容的 SparkleMorpher 版本，在模组 Cloud 设置中添加地址、选择实例并注册或登录。玩家可在 `/account/` 自助绑定自己拥有的游戏与 Cloud 账号，无需服主代绑。管理员账号仍由服主分发。微软网页登录需要服主配置自己的 Microsoft 应用，并取得 Minecraft 服务接入许可；尚未配置时使用模组内绑定或已启用的第三方身份服务。

## 5. 常见故障与极端操作

- **令牌框空白/未认证**：这是登录框，不会自动读取服务器文件。登录服务器，按上表查看令牌并粘贴完整一行；确认访问正确实例。找不到令牌时检查实际的 `--config` 路径及 `auth.bootstrap_access_token_file`，不要删数据库或重复初始化。
- **忘记服主密码**：有初始令牌就用它登录后设置新密码。令牌和密码都丢失时，先停止服务并找安全备份；不要删除或手工编辑账号数据库，按[部署指南](deploy/ADMIN_JSON.md)恢复。
- **网页无法保存设置**：确认进程使用 `--config` 且服务账号可写配置目录。管理员没有写配置权限。监听、存储、公开来源、日志等修改需重启。若关闭 WebUI，需在服务器 JSON 中重新开启。
- **网页/证书打不开**：检查 DNS、TCP 80/443、防火墙、Caddy 状态，以及代理域名是否和公开地址一致；8787 只应由本机或代理访问。
- **502/上传失败**：检查 Cloud 健康状态和代理上游。默认每个模型最大 128 MiB，代理请求体上限也要至少这么大。
- **识别不工作或仍可下载**：分别确认识别、阻止开关；检查 NudeNet 是否可被 Cloud 容器访问；重新扫描旧模型，并确认玩家连接同一实例。
- **误删/升级后数据异常**：普通删除可在模型库恢复；永久清理不能从 WebUI 撤销。升级前备份配置（含令牌）、数据库、模型对象和 Docker 卷。不要用 `docker compose down -v` 做普通升级。

## 6. 备份与更多资料

升级前备份配置目录、数据库及模型对象。Docker 还需保留命名卷；原生部署应停止服务后备份 SQLite 及 WAL 文件。恢复时使用兼容的数据版本。权限、反代、R18 服务、升级和回滚详见[管理员部署指南](deploy/ADMIN_JSON.md)。开发者可查看[源码与 API](https://github.com/sdf123098/spm-cloud)、[Compose 配置](docker-compose.json.yml)和[发布版本](https://github.com/sdf123098/spm-cloud/releases)。许可证：MIT。
