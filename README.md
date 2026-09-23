# SPM Cloud

独立的 SparkleMorpher Cloud 自托管后端。它不作为 Minecraft 服务端模组运行；客户端通过 HTTPS 与 WSS 使用同一套 Cloud 协议。

当前实现基于计划 v4.2 的 P0/P1/P2 边界，项目版本为 `2.0.0`：

- Axum/Tokio HTTP 服务与 WebSocket 握手、心跳、64 KiB 二进制消息限制。
- Protobuf schema 版本 `spm.cloud.v1`。
- SQLite WAL 持久化与本地原始对象目录。
- `CloudAccount`、provider registry、命名空间身份、scope、target、ACL 和外观 CAS。
- 原始资产上传/下载的 SHA-256、ETag、Range/206/304/416 语义。
- 客户端断线时不回退 Minecraft 自定义通道。

## 本地运行

需要 Rust 1.85+。服务支持 Windows、Linux 和 macOS；默认监听 `127.0.0.1:8787`，数据写入当前目录下的 `data`。路径由 `SPM_CLOUD_DATA_DIR`、`SPM_CLOUD_DATABASE` 和 `SPM_CLOUD_OBJECT_DIR` 控制，不依赖固定的 Unix 路径。生产环境必须显式设置访问令牌和反向代理 TLS：

```powershell
$env:SPM_CLOUD_ACCESS_TOKEN = "replace-with-a-secret"
cargo run
```

Windows PowerShell、Linux/macOS shell 均可直接运行同一个 Cargo 项目：

```powershell
$env:SPM_CLOUD_BIND = "127.0.0.1:8787"
$env:SPM_CLOUD_DATA_DIR = (Join-Path (Get-Location) "data")
cargo run --release
```

```bash
export SPM_CLOUD_BIND=127.0.0.1:8787
export SPM_CLOUD_DATA_DIR="$PWD/data"
cargo run --release
```

当前 `SPM_CLOUD_ACCESS_TOKEN` 是本地/受控部署的 bootstrap bearer；它不是最终的 Cloud 登录协议。游戏身份写入后默认是 `PENDING_VERIFICATION`，未完成 Session Service challenge 不能获得 Offline binding。

### 跨平台开发

使用 Rust 官方工具链为目标平台安装对应 target 与原生编译依赖。Linux、macOS 和 Windows 的构建应在各自原生环境或对应 CI runner 上执行；本仓库不固定开发机路径或 linker 配置。

健康检查：

```text
GET http://127.0.0.1:8787/health
GET http://127.0.0.1:8787/v1/instance
```

受 bootstrap bearer 保护的独立账号创建接口：

```powershell
$headers = @{ Authorization = "Bearer $env:SPM_CLOUD_ACCESS_TOKEN" }
Invoke-RestMethod http://127.0.0.1:8787/v1/accounts -Method Post -Headers $headers -ContentType 'application/json' -Body '{"account_id":"alice","password":"change-this-password"}'
```

密码只以 Argon2id 摘要写入 SQLite；登录使用 `POST /v1/sessions`，返回的 access/refresh token 只保存摘要。生产部署应通过反向代理提供 HTTPS，并把 bootstrap bearer 和数据库目录纳入密钥/备份管理。

迁移旧 `custom/auth/built` 资源时，先生成只读审计清单：

```powershell
python tools/migrate_legacy.py C:\path\to\legacy\custom --output migration.json --scope-id scope-main --world-epoch epoch-1
python -m unittest tools/test_migrate_legacy.py
```

清单只记录相对路径、格式、大小和 SHA-256；不会上传、删除源文件或授予 Cloud 权限。导入前必须人工确认 identity、target kind、scope/world epoch 和 ACL。

## 当前边界

这是独立后端的本地/自托管实现基线：已提供 provider registry 管理、官方/可信 Yggdrasil challenge 验证、外观 outbox 与 WebSocket 恢复接口，以及迁移盘点/导入工具。仓库另有 `cloudflare/` 官方部署适配器，使用 Worker + D1 + R2 + Durable Object；其当前能力和仍未通过的生产门禁见对应 README。协议模型与数据权限边界必须由两种部署复用。

Cloudflare 官方实例的配置、迁移、secret 和部署命令见 [`cloudflare/README.md`](cloudflare/README.md)。当前 Worker 已有账户/session、scope/target/ACL、外观与动画 CAS、资产目录/上传下载/可见性和离线审批等 API，并已部署到 `workers.dev`。但它仍使用管理员 bootstrap bearer 开通账户；官方/Yggdrasil 游戏身份 provider 尚未配置，实时事件恢复/lease/撤权、多客户端实测、生产告警和自定义域名也未完成验收。因此当前部署可用于受控测试，不应宣称已达到完整生产发布状态。

## 跨平台交付

- 原生运行：在 Windows x64、Linux x64/arm64、macOS x64/Apple Silicon 上用 Rust 编译；SQLite 使用 `bundled`，TLS 使用 Rustls，避免依赖系统 SQLite/OpenSSL。
- 容器运行：`Dockerfile` 是 Linux 容器镜像，Windows/macOS 使用 Docker Desktop 运行同一 `docker compose` 文件；持久化 volume 和 Caddy TLS 语义保持一致。
- CI：`.github/workflows/platforms.yml` 对 Ubuntu、Windows、macOS 执行 check/test/release build；发布时再按目标平台签名和打包二进制。
- 不把 `bash`、`systemd`、Linux 文件锁、Unix socket 或 `/var/lib` 路径写入服务核心；这些只允许出现在部署示例中。

## 单服务器部署

推荐使用仓库内的 `docker-compose.yml`：它运行一个持久化的 `spm-cloud`，并由 Caddy 负责 HTTPS/WSS。先准备 DNS、服务器防火墙的 80/443 端口，以及一个只允许管理员读取的 `.env`：

```dotenv
SPM_CLOUD_HOST=cloud.example.com
SPM_CLOUD_INSTANCE_ID=prod-1
SPM_CLOUD_ORIGIN=https://cloud.example.com
SPM_CLOUD_ACCESS_TOKEN=replace-with-a-long-random-bootstrap-token
SPM_CLOUD_BOOTSTRAP_ACCOUNT=account_local
SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH=$argon2id$v=19$m=65536,t=3,p=4$...
SPM_CLOUD_ALLOW_SELF_REGISTRATION=false
```

By default, `POST /v1/accounts` is restricted to the bootstrap bearer. To let players create their own accounts on a community instance, set `SPM_CLOUD_ALLOW_SELF_REGISTRATION=true` once in the service environment (or the matching `.env` used by Docker Compose), then restart the service. Registration still grants no game-identity verification, scope membership, target ownership, or asset ACL; those remain separate authorization steps. Keep the bootstrap bearer private.

启动、升级和备份：

```powershell
docker compose up -d --build
docker compose logs -f spm-cloud
docker compose stop spm-cloud
docker volume ls
docker run --rm -v spm-cloud-data:/from:ro -v ${PWD}/backup:/to alpine sh -c 'tar czf /to/spm-cloud-data.tgz -C /from .'
docker run --rm -v spm-cloud-objects:/from:ro -v ${PWD}/backup:/to alpine sh -c 'tar czf /to/spm-cloud-objects.tgz -C /from .'
docker compose start spm-cloud
```

Linux/macOS 的等价备份命令（如果通过 `.env` 自定义 volume 名称，应替换下面两个名称）：

```bash
mkdir -p backup
docker compose stop spm-cloud
docker run --rm -v spm-cloud-data:/from:ro -v "$PWD/backup:/to" alpine sh -c 'tar czf /to/spm-cloud-data.tgz -C /from .'
docker run --rm -v spm-cloud-objects:/from:ro -v "$PWD/backup:/to" alpine sh -c 'tar czf /to/spm-cloud-objects.tgz -C /from .'
docker compose start spm-cloud
```

Windows Docker Desktop 可使用上面的 PowerShell 形式。备份期间必须停止服务，避免 SQLite WAL 和对象目录出现不一致快照。

备份必须同时包含 SQLite 数据目录和对象目录；恢复时停止服务、恢复两个 volume，再启动服务。不要把 SQLite 文件放在多个实例之间共享，也不要把容器本地目录当作跨主机存储。生产环境应把 `.env` 放到密钥管理器，定期轮换 bootstrap bearer，并验证备份可恢复。

## 离线绑定策略

创建 scope 时可设置 `offline_policy`，缺省为 `STRICT_APPROVAL`：

- `STRICT_APPROVAL`：离线身份只能提交待审批绑定，scope 管理员用当前 `revision` 批准。
- `CLAIM_CODE`：管理端生成一次性 claim code；代码有有效期和失败次数限制，兑换成功后立即消费。
- `FIRST_CLAIM`：首个匹配 claim 成功后占用该 `scope/world_epoch/entity_uuid`，后续兑换冲突。
- `DISABLED`：禁止离线绑定和 claim code。

相关接口为 `POST /v1/identities/{identity_id}/offline-bindings`、`GET /v1/scopes/{scope_id}/offline-bindings`、`PUT /v1/scoped-identity-bindings/{binding_id}`、`POST /v1/targets/{target_id}/claim-codes` 和 `POST /v1/claim-codes/redeem`。审批请求必须携带 `expected_revision`，避免管理员页面覆盖并发变更。

## 多服务器部署边界

当前 SQLite WAL + 本地对象目录只适合单服务器。多服务器部署必须先替换为共享关系数据库、S3 兼容对象存储以及跨实例 outbox/消息广播，再允许多个 API/WS 实例；不能通过共享 SQLite 文件或共享本地目录“扩容”。Cloudflare 方案也遵循同一边界：R2 存对象，D1/外部 Postgres 存事务数据，Durable Objects 或队列承担实时协调。
