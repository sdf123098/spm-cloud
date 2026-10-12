# SPM Cloud 使用与运维手册

SPM Cloud 是 SparkleMorpher 模组使用的自建云服务。服主负责部署与管理；玩家在模组里添加相同的 Cloud 地址后，可以浏览和使用有权访问的模型、外观与动作。服务包含 HTTPS API、实时 WebSocket 和内置管理网页，不需要单独部署前端。

> [English](README.md) | **简体中文**

## 1. 选择部署方式

### Docker Compose（推荐）

适用于 Linux 服务器。准备 Docker Engine、Compose 插件、一个指向服务器的域名，并开放 TCP 80/443。仓库根目录提供 Caddy HTTPS 反代和持久数据卷：

    git clone https://github.com/sdf123098/spm-cloud.git
    cd spm-cloud
    mkdir -p deploy/runtime
    docker compose -f docker-compose.json.yml build spm-cloud
    sudo chown 10001:10001 deploy/runtime
    sudo docker run --rm --user 10001:10001 \
      -v "$PWD/deploy/runtime:/etc/spm-cloud" spm-cloud:local \
      --init-config /etc/spm-cloud/config.json

初始化会生成 deploy/runtime/config.json 和独立的 secrets/bootstrap-token.txt。编辑配置中的公网 HTTPS 地址和唯一实例 ID；再把 Caddyfile.compose 中的示例域名换成同一个域名，然后启动：

    sudo docker compose -f docker-compose.json.yml up -d
    sudo docker compose -f docker-compose.json.yml ps
    sudo docker compose -f docker-compose.json.yml logs -f spm-cloud

玩家访问 HTTPS 根地址，例如 https://cloud.example.com。不要向互联网直接开放后端 8787 端口。平常升级不要执行 docker compose down -v，该命令会删除命名数据卷。

### Linux 原生安装

需要 systemd 的服务器可从 GitHub 最新 Release 下载 shell 安装器。安装目录设为 /usr/local，安装后程序位于 /usr/local/bin：

    curl --fail --location --proto '=https' --tlsv1.2 \
      https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.sh \
      --output /tmp/spm-cloud-installer.sh
    sudo env SPM_CLOUD_INSTALL_DIR=/usr/local sh /tmp/spm-cloud-installer.sh
    sudo useradd --system --home /var/lib/spm-cloud --create-home --shell /usr/sbin/nologin spm-cloud
    sudo install -d -o spm-cloud -g spm-cloud -m 750 /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
    sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud
    sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json

初始化后，配置和令牌位于 /etc/spm-cloud。将 config.json 中 storage.data_dir、storage.database_path 和 storage.object_dir 设置为 /var/lib/spm-cloud 下的持久目录。先检查仓库模板 deploy/spm-cloud-json.service 中的路径，再安装并启动：

    sudo cp deploy/spm-cloud-json.service /etc/systemd/system/
    sudo systemctl daemon-reload
    sudo systemctl enable --now spm-cloud-json.service
    sudo systemctl status spm-cloud-json.service
    sudo journalctl -u spm-cloud-json.service -f

反代建议只转发 HTTPS 到本机 127.0.0.1:8787。

### Windows 原生安装

在 PowerShell 中用官方安装器将程序放入当前用户 Cargo bin，再在独立数据目录初始化配置：

    $env:SPM_CLOUD_INSTALL_DIR = "$env:USERPROFILE\.cargo"
    irm https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1 | iex
    $exe = "$env:USERPROFILE\.cargo\bin\spm-cloud.exe"
    New-Item -ItemType Directory -Force C:\SPMCloud | Out-Null
    Set-Location C:\SPMCloud
    & $exe --init-config C:\SPMCloud\config.json

编辑配置中的公网 HTTPS 地址、实例 ID、数据库与对象路径，然后启动：

    & $exe --config C:\SPMCloud\config.json

生产环境还应配置 Windows 服务或受控的自动启动方式，并在 Cloud 前部署 HTTPS 反向代理。令牌、配置、数据库和模型对象应限制为服务账号与服主可访问。

## 2. 首次登录和账号权限

初始访问令牌是服主第一次登录 /admin/ 的最高权限凭据。它保存在服务器文件，不会自动发送给浏览器，也不是 Minecraft 密码。默认位置为配置文件旁边的 secrets/bootstrap-token.txt；自定义了 auth.bootstrap_access_token_file 时以该路径为准。

1. 在服务器上读取令牌文件，并复制完整的一行。Linux 可用 sudo cat；Windows 可用 PowerShell Get-Content。
2. 打开 https://你的域名/admin/，粘贴令牌登录。
3. 在“账号与权限”中为服主账号设置密码。
4. 后续使用服主账号与密码登录；保管好初始令牌，以便恢复管理权限。

令牌不要放入仓库、截图、聊天记录、命令行参数或公开日志。若令牌泄漏，应立即登录管理台轮换；无法登录时，先停止服务并使用可信备份恢复。

服主可创建管理员并分配账号。管理员能管理模型、查看设置与操作记录，但不能创建其他管理员或更改服务器配置。玩家账号可自行注册，具体由服主的注册设置决定。停用账号、变更权限或重置凭据会使相关会话失效。

玩家在兼容版本的模组中添加 Cloud 根地址、选择实例，再注册或登录。玩家可在 /account/ 管理自己拥有的账号绑定，不必把游戏密码交给服主。微软网页登录需要服主配置自己的 Microsoft 应用及相应的 Minecraft 服务许可；尚未配置时，使用模组内账号绑定或已启用的第三方 Yggdrasil 身份服务。

## 3. WebUI 日常操作

- “模型库”支持搜索、分页、网格与 3D 卡片、多选和批量操作。“全部模组”展示有权浏览的目录；服主账号在账号网格置顶。
- 点击模型名称查看详情；选择模型后可调整公开/私有、重新识别、人工标记、删除或恢复。普通删除可恢复；永久清理不可撤销，执行前先备份。
- 模型显示原始文件大小，并按 B、KiB、MiB 等单位换算。3D 预览面向 Minecraft 模型正面；支持 BBModel、Bedrock ZIP/YSM 和内嵌资源的 GLTF/GLB。加密 YSM 预览需配置 YSMParser；GLTF/GLB 不会访问外部网络资源。
- 配置、绑定身份和操作记录列表支持搜索。模型搜索包括名称、模型 ID、归属账号与格式；列表使用服务端分页，避免一次性请求全量数据。
- “维护与清理”可检查稳定版更新，导出数据库和模型备份、完整模型库、当前进程最近 2 MiB 日志及按条件筛选的操作记录。服主可导出完整备份；管理员权限按页面说明执行。

上传默认公开。私有模型仅所有者账号可访问；上传时选择正确的归属账号。文件选择窗口始终在操作者本机打开，只会上传用户选择的文件。

### 内容识别与阻止

在“模型与内容安全”中分别设置“识别”和“阻止”：

- 识别读取可解析的模型元数据；可选 NudeNet 扫描可读取贴图并标记 R18、清洁或未识别。
- 阻止会把判定为 R18 的模型从普通玩家目录、下载和使用入口隐藏。

这两个功能相互独立，默认关闭。只开识别会生成标签但不会拦截；只开阻止时没有识别结果可判断。启用识别后，对旧模型执行重新识别。容器环境可叠加启动 NudeNet：

    docker compose -f docker-compose.json.yml \
      -f docker-compose.moderation.yml up -d

随后在设置中将贴图扫描地址指向 http://nudenet:8080/infer。加密 .ysm 还需要安装并配置 YSMParser。自动检测可能漏判或误判，应提供人工复核流程；未识别内容是否阻止可单独设置。

## 4. 配置、路径与凭据

新部署使用 JSON。生成配置：

    spm-cloud --init-config /path/to/config.json

检查配置但不启动服务：

    spm-cloud --config /path/to/config.json --check-config

查看脱敏后的有效配置与来源：

    spm-cloud --config /path/to/config.json --print-effective-config

被选中的 JSON 文件是配置事实源；环境变量不会覆盖 JSON 中的值。仅在未选择 JSON 的旧 ENV 启动方式中读取 SPM_CLOUD_* 环境配置。JSON 内相对路径以配置文件所在目录为基准；旧 ENV 模式的相对路径以进程工作目录为基准。配置示例和完整字段类型见 config.example.json 与 config.schema.json。

| 配置 | 用途与默认行为 |
|---|---|
| instance.instance_id / origin / bind | 稳定实例 ID、公网 HTTPS 根地址、监听地址。监听、ID 和公网地址要重启服务才生效。 |
| storage.data_dir / database_path / object_dir | 数据库和模型对象位置；SQLite 默认保存于数据目录。备份时必须同时考虑数据库和对象文件。 |
| auth.bootstrap_account_id | 服主账号 ID。 |
| auth.bootstrap_access_token_file | 服务器端初始访问令牌文件；不要将令牌写入 JSON。 |
| auth.bootstrap_password_hash_file | 可选 Argon2id 密码摘要文件，不可填明文密码。 |
| auth.allow_self_registration | 是否允许玩家自助注册。 |
| auth.identity_providers | 可选 Yggdrasil 身份服务列表。首次配置可用 has_joined_url，或填写 base_url 与 session_path。已有 provider ID 应保持稳定；停用时保留条目并设 enabled=false。 |
| limits.max_asset_bytes / max_message_bytes | 单模型与普通协议消息上限。默认分别为 128 MiB 和 64 KiB；反向代理上传上限也必须足够大。 |
| performance.preset | 低配 2/1，中配 4/2（默认），高配 8/4，或自定义上传与模型扫描/解码并发。自定义的每项填 0 表示该项不限制；0/0 表示两项都不限制。降低限制不会中断已经开始的工作。 |
| updates.auto_update / check_interval_hours | 自动更新默认关闭；检查间隔默认 24 小时。容器镜像由部署平台更新。 |
| cache.backend | memory（默认）、redis 或 none。Redis 故障时回落到有界本地缓存。 |
| cache.ttl_seconds / max_pages / timeout_ms | Redis/内存目录缓存的过期时间、最大页数和响应超时。 |
| moderation.* | 识别、阻止、扫描服务地址、阈值、解码预算与 YSMParser 路径。 |
| features.* / retention.* | 可选游戏能力开关及状态保留时间；未被客户端正式支持的能力不要随意启用。 |
| logging.level / format | 日志等级和文本/JSON 格式。重启生效。 |
| web_ui.enabled | 是否开放管理网页；关闭后须通过服务器配置重新开启。 |

JSON 配置会每两秒检查一次。有效运行期选项就地更新；格式错误时保留上一次有效设置。监听、实例身份、公网地址、存储位置、启动凭据和日志配置需要重启。配置保存需要以 --config PATH 启动，并确保服务账号可以写配置目录。不要对现有实例重复初始化；初始化不会覆盖配置，也不会重置现有密码。

旧部署的 ENV 文件不由 Rust 自动加载。迁移时停服、备份数据和凭据、生成 JSON、将现有数据库/对象/令牌路径映射到 JSON，再运行 --check-config。SPM_CLOUD_CONFIG 只用于选择 JSON 文件。完整历史变量映射已并入本手册的配置字段和 .env.example。

## 5. 可选 MySQL 持久库与 Redis 缓存

SQLite 与内存缓存仍是默认设置，小型单机部署不需要额外服务。

### MySQL

建立专用数据库和最小权限的服务账号，将连接 URL 存入仅服务账号可读的文件，然后设置：

    {
      "storage": {
        "backend": "mysql",
        "mysql_url_file": "secrets/mysql-url.txt"
      }
    }

连接 URL 可使用 mysql://用户:密码@主机:3306/数据库。不要把密码提交到仓库或写入共享日志；远程 MySQL 应限制网络访问并启用受支持的传输加密。

首次启动时，如果 storage.database_path 是有效 SQLite 库且 MySQL 目标没有账号或模型，服务会把旧数据库表导入 MySQL。保留原来的 object_dir，让数据库里的对象路径继续指向原模型文件。已有非空 MySQL 不会被自动合并。迁移前保留停机备份，迁移后检查账号、权限、模型下载和 WebUI 备份后，再决定何时归档旧库。MySQL 快照导出为 SQLite 格式；tools/backup_restore.py 只适用于停止服务后的 SQLite 文件备份。

当前 MySQL 访问适配层使用一个同步连接串行执行 SQL；高并发部署应先在目标负载下压测。MySQL 适合需要外置持久存储的部署，不代表默认 SQLite 部署必须改造。

### Redis

Redis 只缓存短期目录页等可重建数据，不保存账号、权限、审计或模型文件。将凭据放进受保护文件，配置例如：

    {
      "cache": {
        "backend": "redis",
        "redis_url_file": "secrets/redis-url.txt",
        "ttl_seconds": 30,
        "max_pages": 256,
        "timeout_ms": 300
      }
    }

可用 SPM_CLOUD_REDIS_URL 为旧 ENV 部署提供地址。服务会限制页数与 TTL；Redis 不可用时切换到有界内存缓存。缓存命中不会绕过当前数据库权限验证。设 backend 为 none 可关闭目录缓存。

## 6. 备份、恢复与升级

### WebUI 在线导出

服主可从“更新与导出”下载数据库与不可变模型版本组成的一致性备份 ZIP、原格式完整模型库 ZIP、当前进程最近 2 MiB 日志，以及全部或搜索过滤后的操作记录。单模型导出保留原始文件。凭据、配置、反向代理证书和容器环境文件不包含在备份内，应另行保护。重启前的日志从 systemd、Docker 或部署平台获取。

下载备份后解压，再使用恢复工具：

    python tools/backup_restore.py restore \
      --backup /path/to/extracted-backup \
      --database /path/to/restore/spm-cloud.db \
      --objects /path/to/restore/objects

离线创建备份前先停服，避免数据库与对象目录跨时点。命令示例：

    python tools/backup_restore.py backup \
      --database DATA/spm-cloud.db \
      --objects OBJECTS \
      --destination NEW_BACKUP

备份工具检查数据库和模型对象的 SHA-256、字节长度、路径重叠和符号链接。恢复默认不覆盖已存在目标；确需覆盖时才使用 --force。保持服务停止直到数据库和对象都恢复完毕，启动后检查登录、权限和模型下载。恢复前不要删掉原备份。

### 自动更新与回滚

更新前备份配置、令牌、数据库、模型对象和 Docker 命名卷。WebUI 可以检查最新正式发布；原生更新器会校验 SHA-256 和二进制版本，等待上传与模型处理任务结束，创建本地备份，再替换程序并重启。自动更新默认关闭，程序安装目录必须可写。Unix 主机需要 tar 和 xz 支持。

Docker 镜像应通过部署平台更新。仓库提供 Docker Compose systemd 定时更新模板，仅支持 Linux amd64/arm64，并需要 Docker Compose、curl、jq、gzip、sha256sum、flock。若部署目录不在 /opt/spm-cloud，修改 unit 内的 Compose 路径。先手动执行一次 updater service 并检查日志，再启用 timer。失败时脚本会尝试恢复先前镜像。

Compose 自动更新器假设仓库位于 /opt/spm-cloud。将工作树和 Compose .env 放在那里后安装模板：

    sudo apt install -y curl jq gzip coreutils util-linux
    sudo cp deploy/spm-cloud-json-auto-update.service /etc/systemd/system/spm-cloud-auto-update.service
    sudo cp deploy/spm-cloud-json-auto-update.timer /etc/systemd/system/spm-cloud-auto-update.timer
    sudo systemctl daemon-reload
    sudo systemctl enable --now spm-cloud-auto-update.timer
    sudo systemctl start spm-cloud-auto-update.service
    sudo systemctl status spm-cloud-auto-update.timer

若路径不同，修改 service 中 SPM_CLOUD_COMPOSE_DIR 与脚本路径。timer 第一次运行也会应用最新 Release；确认服务已启动、日志无误后再长期启用。

原生 Linux 更新器从最新 Release 下载与系统匹配的预编译归档和校验文件；若重启后的健康检查失败会恢复旧程序。首次上线前确认服务名、二进制位置和健康检查 URL，并查看 journalctl 中的 updater 日志。恢复旧程序不等于回退数据库迁移；若旧二进制不兼容新数据库，必须同时还原升级前的数据库备份。

原生更新器安装示例（仓库工作树本身不必位于固定位置）：

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

默认程序和服务分别为 /usr/local/bin/spm-cloud 与 spm-cloud.service，健康检查使用 http://127.0.0.1:8787/health。需要改动时，在 /etc/spm-cloud-auto-update.env 设置 SPM_CLOUD_BINARY、SPM_CLOUD_SYSTEMD_SERVICE 或 SPM_CLOUD_AUTO_UPDATE_HEALTH_URL；运行后检查 journalctl -u spm-cloud-native-update.service。

## 7. 故障排查

| 现象 | 检查步骤 |
|---|---|
| 登录页令牌框空白 | 这是输入框，不会读取远程服务器文件。到服务器读取令牌文件并粘贴完整一行；核对实际配置路径。 |
| 忘记服主密码 | 若初始令牌仍在，可登录后重置。令牌与密码都丢失时停服并恢复可信凭据备份；不要直接编辑账号数据库。 |
| WebUI 无法保存 | 确认进程以 --config PATH 启动、JSON 有效、服务账号对配置目录有写权限；管理员账号没有配置写权限。 |
| HTTPS、证书或 502 故障 | 检查 DNS、TCP 80/443、防火墙、反代上游和证书续期；后端 8787 只允许本地反代访问。 |
| 上传失败 | 检查服务与反代日志、模型字节数、反向代理 body 上限和磁盘空间。默认单模型上限为 128 MiB。 |
| Redis 数据库离线 | 确认 URL 文件权限、网络与 Redis 状态。服务会回退到内存缓存；目录数据仍从持久库验证。 |
| MySQL 无法连接或迁移 | 校验库名、账号权限、TLS 与 URL 转义；首次迁移要求有效旧 SQLite 且 MySQL 目标为空。先恢复备份再重试。 |
| 内容检测不工作 | 检查识别与阻止是否分别启用、扫描服务可达性、YSMParser 配置和模型扫描预算；旧模型需重新识别。 |
| 更新后数据异常 | 停止更新器和服务，保留日志及旧文件，从升级前一致性备份恢复数据库和模型对象，再使用兼容程序启动。 |

## 8. API、能力与客户端兼容

玩家和模组连接 HTTPS 根地址，实例发现与模型目录位于 /v1；账号自助页面为 /account/，管理员页面为 /admin/，健康检查为 /health。服务使用 Protobuf 的 spm.cloud.v1 共享协议；WebSocket 实时入口为 /v1/realtime，普通消息默认不超过 64 KiB，模型文件通过 HTTP 上传和下载，不通过 WebSocket 发送。API 约定见 docs/PROTOCOL_V1.md。

模型修订保存原始字节、SHA-256、长度和对象路径。下载支持 ETag 与单字节范围；外观写入用 expected_revision 检测并发冲突。权限按账号、作用域、目标和模型可见性核查。私有模型和被阻止内容不会进入普通目录。

玩家显示状态、载具绑定和实体动作时间线通过 features 中的独立开关控制；只启用已部署客户端实际支持的能力。投射物快照仍属于预留能力，不要因为配置 schema 出现字段就视为客户端已支持。能力条件、权限、期限和协议数据请查 PROTOCOL_V1.md 与配置 schema。

## 9. 从源码构建、测试与贡献

构建 Rust 服务需安装与 rust-toolchain.toml 一致的 Rust stable 工具链。预览包已提交到仓库，普通 Cargo 构建不要求 Node。修改 web/preview.js 时需要 Node.js 22：

    cd web
    npm ci
    npm run build
    cd ..
    cargo build --locked

常规验证：

    cargo fmt --all -- --check
    cargo check --all-targets --all-features --locked
    cargo test --all-targets --all-features --locked
    cargo clippy --all-targets --all-features --locked -- -D warnings
    node --test web/preview.test.cjs

跨平台预编译归档、安装器、校验文件和 Docker 发布包由 GitHub Release 工作流从版本标签构建。开发者如要验证完整 HTTP 服务契约，可构建 release binary 后使用 tools/service_contract.py；测试需指定测试模型文件。不要在生产实例上跑破坏性测试。

离线认证测试夹具位于 fixtures/profile-proofs.json，只包含合成公钥、证书和公开签名载荷，不含私钥或游戏账号。测试直接把夹具目录传给纯验证函数；线上服务始终使用仓库内固定的 Mojang 信任根和官方 HTTPS 公钥地址，不接受环境变量或请求字段替换生产信任锚。

## 10. 许可证与项目资料

项目使用 MIT 许可证。协议细节见 docs/PROTOCOL_V1.md；可编辑配置模板与字段约束见 config.example.json、config.schema.json；Compose 和反代模板位于仓库根目录。源代码、问题反馈和正式构建产物均可从 GitHub 项目页与 Releases 页面获取。
