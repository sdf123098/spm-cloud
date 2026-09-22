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

需要 Rust 1.85+。默认监听 `127.0.0.1:8787`，数据写入 `./data`。生产环境必须显式设置访问令牌和反向代理 TLS：

```powershell
$env:SPM_CLOUD_ACCESS_TOKEN = "replace-with-a-secret"
cargo run
```

当前 `SPM_CLOUD_ACCESS_TOKEN` 是本地/受控部署的 bootstrap bearer；它不是最终的 Cloud 登录协议。游戏身份写入后默认是 `PENDING_VERIFICATION`，未完成 Session Service challenge 不能获得 Offline binding。

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

这是独立后端的第一批实现，不宣称 Cloudflare 公共实例、第三方 provider 的远程认证、完整管理 GUI 或迁移导入已经完成。协议模型与数据库边界先固定，后续 Cloudflare 适配必须复用这些领域语义。
