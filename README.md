# SPM Cloud: setup and everyday operation

**English** · [简体中文](README_zh.md) · [Download a release](https://github.com/sdf123098/spm-cloud/releases/latest)

This guide describes **SPM Cloud 2.4.x**, the self-hosted service used by SparkleMorpher clients for accounts, models, permissions, and display-state synchronization. The management website is built into the service. There is no separate frontend to install.

**Choose your starting point:**

| Your situation | Start here |
|---|---|
| Try the website on a Windows PC | [B1: Windows installation](#b1-windows-installation-and-first-start) |
| Run a long-lived Linux server | [B2: Docker Compose](#b2-docker-compose) or [B3: native Linux](#b3-native-linux-and-macos) |
| The service already runs; manage it in a browser | [A1: first sign-in](#a1-first-sign-in-and-account-roles) |
| You are a player rather than the operator | [A4: player accounts and linking](#a4-player-accounts-and-linking) |
| An older ENV deployment has read-only settings | [B9: ENV compatibility and migration](#b9-env-compatibility-and-migration) |
| Something went wrong | [Troubleshooting](#troubleshooting) |

The guide has two main parts: **A: WebUI**, for browser operations, and **B: command line**, for installation, file configuration, and maintenance. Complete one installation path before using the WebUI. Most routine management can then be done in part A.

The shipped interface currently uses Chinese labels. This English guide includes the actual labels you need to find.

## Terms to understand first

| Term | Meaning |
|---|---|
| Server | The computer running SPM Cloud. It can be a rented server or your own PC. It does not have to be the computer running your Minecraft server. |
| Operator computer | The computer where you open the browser. The upload file picker selects files on this computer, not on the Cloud server. |
| Cloud root address | For example, `https://cloud.example.com`. Players enter this root address in the mod, without `/admin/`, `/account/`, or `/v1`. |
| Management website | Add `/admin/` to the root address. Owners and administrators use it. |
| Account portal | Add `/account/`. Players sign in and verify their own account/identity links here. |
| Listen address | Where the process accepts requests, such as `127.0.0.1:8787`. This is different from its public HTTPS address. |
| Reverse proxy | Caddy or Nginx receives public HTTPS and forwards it to Cloud. Cloud's listener serves HTTP; entering an HTTPS public address does not create certificates. |
| Configuration | Usually `config.json`: switches, limits, paths, and references to credential files. |
| Database and object directory | The database holds accounts, permissions, bindings, and model metadata. The object directory holds original model files. They must be backed up and restored together. |
| Bootstrap token | An initial owner credential with full management access. It is not a player password, Microsoft token, or third-party password. |
| Restart | Stop and start the actual process/container. Reloading a browser tab is not a service restart. |

Replace `cloud.example.com` with your domain. Paths such as `C:\SPMCloud` and `/etc/spm-cloud` are examples chosen for this guide. Existing installations should keep their actual paths and data; do not create a second empty database just to match the examples.

---

# A. WebUI: browser operations

## A1. First sign-in and account roles

**Before starting:** Cloud must already be running. If it is not installed, use B1, B2, or B3 first.

1. Open `https://YOUR-DOMAIN/admin/`. For a local trial on the same computer as Cloud, use `http://127.0.0.1:8787/admin/`.
2. Expand **服主使用初始令牌登录** (owner bootstrap-token sign-in).
3. Read the token on the **server running Cloud**. The initial default location is `secrets/bootstrap-token.txt` next to the configuration file. Commands are in [B5](#b5-configuration-paths-and-credential-files).
4. Copy the complete line into the token input and sign in. The website cannot automatically read the server's token file.
5. Open **服务器配置 → 账号与认证 → 更改服主密码** (Server settings → Accounts and authentication → Change owner password).
6. Set a password of at least eight characters and select **更新密码并重新登录**. Use the owner account ID and this password for later sign-ins. The initial ID is normally `account_local`; check your actual instance.

Password sign-in, bootstrap-token sign-in, and game-identity sign-in are different methods. Owning Minecraft does not automatically grant Cloud management privileges.

| Role | Allowed | Not allowed |
|---|---|---|
| Owner | Manage models and accounts; assign administrators; edit available settings; run connection checks; update, back up, and export | Enable functionality that this build does not implement; replace the fixed owner ID through the WebUI |
| Administrator | Manage models; read settings; search/export audit records; manage their own links | Save server settings, run diagnostics, grant administrator roles, install updates, or export full backups/model-library archives/runtime logs |
| Member | Use content allowed to their Cloud account; manage their own verified links in the account portal | Enter the management console merely because they can sign into the account portal |

Authorization is enforced by the backend. Read-only controls for an administrator are expected. An owner also sees read-only configuration in ENV-only mode; B9 explains migration to writable JSON.

Keep the bootstrap token private. Do not include it in screenshots, chat, issues, repositories, or command-line arguments. If the password is forgotten but the token is available, sign in with the token and set a new password.

### Create accounts, assign roles, and reset passwords

The **owner** uses the separate **账号与权限** (Accounts and permissions) sidebar page, outside the nine settings tabs:

1. Find **创建并分发账号** (Create and distribute an account).
2. Enter an account ID beginning with an ASCII letter or digit. Remaining characters can be ASCII letters, digits, periods, underscores, or hyphens, up to 128 characters total; for example, `player_001`. This is a Cloud login ID, not an email address or game character name.
3. Set an initial password of at least eight characters. Do not distribute your bootstrap token as another person's password.
4. Choose an ordinary member or administrator. Players who only need sign-in/private models should be members; administrators can manage models.
5. Select **创建账号**. After success, privately give the recipient their ID, password, and Cloud root address. The website does not send an email or message for you.
6. Search the account list by ID and use its pagination. Non-owner cards offer grant/revoke administrator, disable/enable, and reset password. These changes are immediate; role changes and password resets invalidate existing sessions, requiring sign-in again.

Disabling retains the account and does not delete its models. Re-enabling does not reset its password. The owner's role/identity cannot be transferred or disabled here. The owner can create accounts even when self-registration is off.

## A2. What editing, saving, and applying mean

**服务器配置** (Server settings) is near the bottom of the sidebar, above the current account. Update/export and audit tools are now inside its nine tabs.

For an ordinary setting:

1. Open its tab and edit the field.
2. Check the pending-change count in the save bar. This is a **draft**, not a server change.
3. Select **校验草稿** (Validate draft). Fix any field errors. Validation alone does not save.
4. Select **保存配置** (Save configuration), and complete any confirmation or generated-token dialog.
5. Check the active value and restart notice. If a restart is required, follow B7; refreshing the browser does nothing to startup-only settings.

| State/action | Meaning |
|---|---|
| Draft / pending changes | Present only in the current browser page |
| Saved | Written to the selected configuration file; some values may still await a restart |
| Active / 正在使用 | What the running service is actually using |
| Pending restart / 待重启 | A saved startup-only value differs from the running value |
| Restore default / 恢复默认 | Put that field's default into the draft; still requires saving |
| Undo all / 撤销全部 | Discard unsaved edits and secret inputs; does not reverse completed operations |
| Reload / 重新载入 | Read the server configuration again; confirmation discards the current draft |
| Connection/service check | Inspect candidate settings without saving, migrating, or linking an account |

Changing tabs, searching, reading audit records, and checking updates retain the draft. Closing/reloading the page or signing out can discard it. Secrets stay in browser memory rather than persistent storage. Do not close the page during a save: generated tokens are returned only once.

**If a revision conflict / HTTP 409 appears:**

1. Do not immediately use Reload.
2. Select **载入新值并保留我的改动** (Load new values and keep my edits).
3. Compare the current server values with your changes.
4. If appropriate, select **采用此修订号，保留我的草稿** (Use this revision and keep my draft).
5. Validate and save again. When someone changed the same field, decide which value should win.

Changing an owner password, managing an account, and editing a model are **immediate independent operations**, not configuration drafts. Undo all does not undo them. Installing an update requires handling the draft first. Backups and exports use the active instance/data, not unsaved changes.

## A3. Uploading and managing models

### Upload files

1. Open **模型库** (Model library), then **上传模型** (Upload models).
2. Select **选择本机模型文件**. This opens the file picker on the computer running the browser. The WebUI does not browse the server's disk.
3. Choose one or several files; Ctrl/Shift allows multiple selection. Supported extensions are `.ysm`, `.zip`, `.bbmodel`, `.gltf`, and `.glb`.
4. The name is optional. Empty uses the filename; with multiple files, a supplied name is a prefix.
5. Choose public/private and the **归属 Cloud 账号** (owning Cloud account). Uploads default to public. Enter an actual Cloud account ID, not a third-party service name.
6. Start the upload and inspect each file's result. If only some fail, retry unfinished items.
7. A completed upload result and an entry in the library confirm success. Selecting a file alone does not upload it.

The default limit is 128 MiB **per file**. HTTP 413 may come from a reverse proxy before Cloud receives the request; see B4.

| Operation | How it works |
|---|---|
| Find a model | Search name, model ID, owning account, or format; the server returns one page at a time |
| Inspect details | Select the model name to view owner, format, original size, version, visibility, and moderation |
| Preview | Choose grid/3D cards. Failed preview is not proof that the original file cannot be downloaded. Encrypted YSM needs a server-side YSMParser; GLTF/GLB do not fetch remote resources |
| Change public/private | Use details or the bulk controls. Public visibility does not grant edit rights; private visibility is not deletion |
| Rescan | Apply the current recognition settings to old content; enabling detection does not automatically rescan every existing model |
| Manual moderation | Mark R18, confirm clean, mark unknown, or remove a manual mark. Identical file content shares content-based moderation, so another model with the same bytes can be affected |
| Change owner | Enter the destination Cloud account and confirm. Existing ownership/shared authorizations are removed; verify the destination first |
| Delete/restore | Deletion stops normal download/use but is reversible. Enable **包括已删除模型** (Include deleted models), select the deleted item, and restore |
| Export one model | **导出原始模型** in details downloads its original format/bytes |
| Bulk actions | Select models first. “Select this page” means this page, not the entire library |

Uploaded, recognized successfully, and available to ordinary players are separate states. A7 explains recognition and blocking.

## A4. Player accounts and linking

Give players the same **Cloud root address**. In a compatible mod, add it to the Cloud server list, select the instance, and register/sign in. Self-registration creates members, not administrators. If registration is disabled, the owner must distribute accounts.

The website is `https://YOUR-DOMAIN/account/`. Sign in with your own Cloud account and open **我的账号与绑定** (My account and links).

1. Check the current WebUI account and linked Cloud account to avoid using the wrong identity.
2. For Microsoft identity, use the Microsoft sign-in flow after the owner configures an application as described in A6.
3. For third-party identity, choose an enabled service, enter **that service's** username/password, and select **验证并绑定第三方账号**.
4. If multiple characters are returned, choose your own character. A service account without an available character must be fixed at that service first.
5. Refresh the links and verify the provider, character name, and UUID.
6. An identity already belonging to another Cloud account you own can establish an account association after verification. The expandable manual Cloud-account association form also requires proving ownership of that other account.

Linking does not grant administrator privileges or transfer model ownership. It cannot overwrite someone else's binding. Investigate provider/account/character/network errors instead of deleting the database.

Advanced manual verification expects an actual Minecraft or third-party **access token**, not the owner bootstrap token. Third-party manual verification also requires the correct character UUID/name. Prefer normal sign-in if you do not already know how to obtain those values.

## A5. Tab 1: General and network — 基础与网络

The defaults below are field defaults. Initialization templates and existing instances may already contain different values; check the active value.

| UI field / JSON path | What to enter and why | Default / application |
|---|---|---|
| 实例标识 / `instance.instance_id` | A stable internal identifier such as `my-cloud`; use letters/digits/underscore/hyphen. Do not casually change an established instance ID | `self-hosted`; restart |
| 玩家连接地址 / `instance.origin` | The real public HTTPS root, e.g. `https://cloud.example.com`. No `/admin/`, query, or embedded credentials | `https://localhost` is a local default, not a public deployment address; restart |
| 监听方式与端口 / `instance.bind` | Same-host native proxy: usually `127.0.0.1:8787`. Docker network: `0.0.0.0:8787`. 0.0.0.0 means all interfaces, not an address to give players | `127.0.0.1:8787`; restart |
| 可信反向代理 / `instance.trusted_proxy_ips` | One literal proxy IP per line, not player IPs/domains/CIDRs. Same-host native Caddy: `127.0.0.1`; this guide's Compose: `172.30.10.3` | Empty list; live |
| 开放管理网页 / `web_ui.enabled` | Keep enabled for the website and management APIs. Copy the recovery command before disabling. The account portal shares the guarded website routes | Enabled; live |

Trusted proxies may forward `X-SPM-Client-IP` for client-IP handling. This is not a general player allowlist. Setting the public address does not configure DNS, certificates, ports, or the reverse proxy.

## A6. Tab 2: Accounts and authentication — 账号与认证

### Registration, owner password, and bootstrap credentials

| Field / JSON path | Purpose | Default / application |
|---|---|---|
| 允许玩家自行注册 / `auth.allow_self_registration` | Allow new member registration through supported mod flows. Disabling does not delete existing accounts | Enabled; live |
| 固定服主账号 / `auth.bootstrap_account_id` | Initial owner account ID; read-only in the website. A game character with a matching name does not gain ownership | `account_local`; not editable here |
| 服主初始令牌 / `auth.bootstrap_access_token_file` | Highest-privilege initial credential; keep, replace, generate, or explicitly clear it. Replacement invalidates the old token immediately | Field default null; initialization creates a random token/reference |
| 首次初始化密码摘要 / `auth.bootstrap_password_hash_file` | Argon2id hash for initial credential creation. It is not a plain password field and does not replace an existing password | Null; bootstrap only |

For a normal password change use **更改服主密码**, enter the new plain password, and submit **更新密码并重新登录**. This immediately updates the account and revokes old password sessions. The bootstrap token is a separate credential and is not automatically revoked by a password change.

To rotate a token, first ensure you know a working owner password. Choose Generate, save, and store the returned value before closing its dialog. It cannot be read back later. Avoid changing/clearing your only working credentials before securely retaining replacements.

### Rules shared by credential editors

These also apply to Redis, MySQL, and the Microsoft application secret.

| Choice | Meaning |
|---|---|
| Keep existing | Reuse the old reference/value. An empty password display does not mean no password is stored |
| Replace | Supply a new secret or structured connection; a successful save creates a new server-managed credential file |
| Clear | Explicitly remove the reference. Dependent features can stop working; a mode requiring that credential may reject the save |
| Existing credential file | Enter a path readable by the **server**, relative to the server configuration directory if not absolute. It does not upload a browser-local file |
| Generate token | Bootstrap token only; do not invent a short predictable value |

Passwords and full connection URLs are not returned by GET. New files live in `.managed-secrets/` beside the configuration. Previous secret files are retained rather than overwritten/deleted. JSON export includes references, not their contents. Back up configuration, `secrets/`, and `.managed-secrets/` separately.

### Add a Yggdrasil provider

1. Select **添加第三方服务** (Add third-party service).
2. In standard mode supply a friendly name and the provider's official HTTPS API entry.
3. Select **检查服务** and read the result; this neither saves nor proves an account can log in.
4. Select **加入配置草稿**, then validate and save the shared configuration draft.
5. Use a dedicated real account in the account portal to verify sign-in/linking and the returned character.

Use a public HTTPS DNS hostname. Do not enter HTTP, localhost/private addresses, embedded credentials, an explicit `:443`, or a web page that redirects somewhere else. A reachable website homepage is not necessarily a Yggdrasil API root.

LittleSkin's documented entry is `https://littleskin.cn/api/yggdrasil`; consult its [official integration guide](https://github.com/LittleSkinChina/manual/blob/master/advanced/yggdrasil.md). This is an address example, not a claim that this project has completed real LittleSkin account verification.

| Mode / field | Meaning |
|---|---|
| Standard | Start here when a provider gives one Yggdrasil root entry |
| Custom | Use when the service documents different relative session/login paths; paths begin with `/` rather than a complete URL |
| Gateway | Use a complete hasJoined verification URL instead of a standard root |
| `provider_id` | Automatically generated stable ID for new entries; retain it when renaming. Identical UUIDs from different providers are separate identities |
| `display_name` | Friendly service name shown to players |
| `base_url` | HTTPS base for standard/custom mode, including any documented prefix |
| `enabled` | Whether this provider may authenticate; disabling retains historical bindings |
| `session_path` | Optional relative session path; defaults to `/sessionserver/session/minecraft/hasJoined` |
| `has_joined_url` | Complete gateway verification URL; mutually exclusive with base_url/session_path |
| `authenticate_path` | Optional relative login path. Omitted preserves compatibility inference, usually `/authserver/authenticate`, or `/auth/authenticate` for the supported special gateway path |

The list is `auth.identity_providers` and defaults to empty configuration. Provider cards show source, enabled status, and check results. The built-in `official` entry is protected against interface edits/disabling.

A successful network probe is not successful account authentication. Missing metadata, one probe returning 404, or an unrecognized login interface requires reading the actual result. Changing endpoints invalidates old check results.

Rename keeps the stable ID and links. Disable stops that provider from authenticating but retains links; restore it from disabled providers. Removing a configuration-managed entry soft-disables it. Legacy database entries can be adopted into configuration management; the first save does not silently wipe every legacy provider.

### Optional Microsoft sign-in: bring your own application

You can skip this if Cloud-account sign-in is enough.

| Field / JSON path | Value |
|---|---|
| 微软登录应用标识 / `web_ui.microsoft_client_id` | Application (client) ID from Entra, formatted as UUID; not the tenant ID or a secret ID |
| 微软应用密钥 / `web_ui.microsoft_client_secret_file` | In Replace mode enter the client secret **Value**; file mode references a server-side file |
| 微软登录回调地址 / `web_ui.microsoft_redirect_uri` | `https://YOUR-DOMAIN/admin/oauth/microsoft/callback`; null derives it from the public root. Must match the registered URI |

All three default to null and apply live after saving. Entering an ID does not create an application.

1. Register your own application in Entra. It must accept **personal Microsoft accounts**, because this implementation uses the `consumers` flow. See [Microsoft registration guidance](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app).
2. Add the exact callback as a **Web** redirect URI, not just the website homepage. See [redirect-URI configuration](https://learn.microsoft.com/en-us/entra/identity-platform/how-to-add-redirect-uri).
3. If configured as a confidential Web client, create a client secret under Certificates & secrets and retain its **Value** and expiry. The secret ID is not usable as the value. Cloud currently accepts client secrets, not a certificate-upload credential workflow. See [Microsoft credential guidance](https://learn.microsoft.com/en-us/entra/identity-platform/how-to-add-credentials).
4. Fill the corresponding Cloud fields, validate, and save. After changing the public root, update the registered redirect as well and restart if the root is pending.
5. Test the entire flow with a dedicated account having an available Minecraft Java profile. The flow contacts Microsoft, Xbox, and Minecraft services; application registration alone does not guarantee access to all of them.

For mismatched redirects, invalid clients, service rejection, or expired flows, check application audience, secret value/expiry, exact redirect, and server network access. Restart sign-in in the original browser; do not share one-time callback links.

**Verification status:** 2.4.0 has automated authentication/callback coverage, but no completed real Microsoft-account OAuth end-to-end test or dedicated LittleSkin account test. A connection check does not replace those tests.

## A7. Tab 3: Models and content safety — 模型与内容安全

### Recognition and blocking are different

| Combination | Behavior |
|---|---|
| Everything off, the default | No new content recognition or enforcement from these switches |
| Recognition only | Generate results without automatically blocking because of those results |
| Block R18 | Enforce existing automatic/manual R18 classification on upload/list/download/use; this switch does not produce recognition results |
| Texture recognition plus a scanner | Send supported embedded model textures to the configured scanner |
| Reject unclassified | Reject content that could not be classified, including disabled detection, timeout, encryption/parser failure, or exhausted budgets |

Start by checking actual recognition results before adopting strict blocking. Rescan old models explicitly. “Unknown” is not the same as “safe.”

| Field / JSON path | Default / range | Guidance |
|---|---|---|
| 单个模型大小上限 / `limits.max_asset_bytes` | 128 MiB; 1 byte–4096 MiB | UI uses MiB, JSON bytes. Align proxy limits too. Higher limits increase resource use |
| 识别模型内容 / `moderation.detection_enabled` | Off | Enable recognition; disabling retains existing results/manual marks |
| 阻止成人内容 / `moderation.block_r18` | Off | Enforce R18 results/manual marks |
| 拒绝未识别模型 / `moderation.block_unclassified` | Off | Strict policy; first check scanner availability, budgets, and handling of old models |
| 识别模型贴图 / `moderation.texture_detection_enabled` | Off | Requires recognition; when enabled a scanner URL is required |
| 贴图识别服务地址 / `moderation.scanner_url` | Null | Full endpoint, e.g. `http://127.0.0.1:8080/infer`, or `http://nudenet:8080/infer` on a compatible Compose network. Internal HTTP(S) is allowed here, unlike provider authentication |
| 裸露内容识别置信度 / `moderation.score_threshold` | 70%; 0–100% | UI 70 = JSON 0.7. Lower thresholds flag more content and can increase false positives |
| 识别等待上限 / `moderation.timeout_seconds` | 30 seconds; 1–300 | A timeout means unclassified, not approved |
| 单模型贴图预算 / `moderation.max_textures` | 64; 1–256 | Insufficient budget can prevent complete recognition |
| 单张贴图大小预算 / `moderation.max_texture_bytes` | 8 MiB; 1 KiB–32 MiB | UI MiB, JSON bytes; limits read/decode work |
| 加密模型解析程序 / `moderation.ysm_parser_path` | Null | Path to an installed YSMParser executable **on the server**, matching its operating system |

These settings affect subsequent operations live. Some scanner controls require both recognition and texture recognition. Disabled controls retain old values rather than clearing them. **检查扫描服务连接** checks connectivity; upload a dedicated model to test actual classification.

Native installations can run the bundled `nudenet_service.py` adapter; commands are in B6. Initial dependency/model downloads may take time.

The optional `docker-compose.moderation.yml` overlay needs an image providing the exact `/infer` JSON contract. A container named “NudeNet” is not automatically API-compatible. If incompatible, run/containerize the supplied adapter instead; do not treat a scanner error as evidence that a model passed.

## A8. Tab 4: Performance and cache — 性能与缓存

### Task concurrency

| Field / JSON path | Default / range | Meaning |
|---|---|---|
| 服务器性能预设 / `performance.preset` | `medium` | `low` = 2 uploads / 1 processing task; medium = 4/2; high = 8/4. Only `custom` uses the fields below |
| 同时上传数量 / `performance.upload_concurrency` | 4; 0–32 | Custom upload limit; 0 means unlimited, not disabled |
| 扫描与解码并发 / `performance.model_processing_concurrency` | 2; 0–16 | Shared scan/decode/preview budget; 0 means unlimited |

Changes apply live. Reducing limits lets existing work finish. More concurrency can exhaust memory rather than improve speed. For non-custom presets the active resolved limits come from the preset.

### Cache is not the database

| Field / JSON path | Default / range | Guidance |
|---|---|---|
| 目录缓存方式 / `cache.backend` | `memory` | Keep the default for a simple instance; `redis` needs Redis; `none` queries the database without directory caching |
| Redis 连接与凭据 / `cache.redis_url_file` | Null | Structured replacement or existing server credential file; Redis mode requires a valid connection |
| 缓存有效时间 / `cache.ttl_seconds` | 30 seconds; 1–3600 | Page expiry, not account/model deletion |
| 最多缓存页数 / `cache.max_pages` | 256; 1–4096 | Cache bound, not model-library capacity |
| Redis 等待上限 / `cache.timeout_ms` | 300 milliseconds; 50–5000 | Redis timeout before bounded in-memory fallback |

To enable Redis:

1. Provision Redis and verify that the **Cloud server** can reach it.
2. Select Redis and Replace credentials.
3. Enter host without `redis://` or a path, actual port (commonly 6379), actual ACL username if needed, password, and database index (usually start with 0).
4. Enable TLS only if that Redis endpoint actually supports it.
5. Check connection, validate, save, and inspect the active backend.

Redis failure falls back to bounded memory; it does not erase durable accounts/models. Switching modes retains other mode parameters. TTL/page controls are disabled when caching is off; Redis timeout is editable in Redis mode. Cache hits still check current authorization.

## A9. Tab 5: Storage and database — 存储与数据库

**Keep SQLite unless you have a reason to operate MySQL.** A database switch is not a required upgrade step.

| Field / JSON path | Default | Meaning |
|---|---|---|
| 数据库类型 / `storage.backend` | `sqlite` | SQLite or MySQL; restart |
| 数据目录 / `storage.data_dir` | `data` | Base directory relative to configuration; restart |
| SQLite 文件位置 / `storage.database_path` | Null → `spm-cloud.db` in data directory | A file path. An explicit path does not move when data_dir changes; restart |
| 模型文件目录 / `storage.object_dir` | Null → `objects` in data directory | A directory. Docker uses paths inside its container; restart |
| MySQL 连接与凭据 / `storage.mysql_url_file` | Null | Structured connection or existing server URL file; actual database switch requires restart |

Read the displayed active database/object paths. Changing configuration does not move files. An unexpectedly empty library after changing paths can mean you connected to a new empty database.

### SQLite → MySQL, step by step

1. Record the actual SQLite/object paths and arrange maintenance.
2. Create a dedicated database and service account **in MySQL first**, with schema/read/write/transaction permissions needed by the service. Cloud does not install MySQL or create the target database/user.
3. Select MySQL and Replace credentials. Enter host, actual port (commonly 3306), existing database name, username, and password. A database name is not a SQLite file path.
4. Enter special characters directly in the structured password field; the backend assembles the URL.
5. Check the target. This does not create business tables or import data.
6. Download and inspect a current backup; separately retain configuration and credential directories.
7. In the save confirmation, review current/target details, acknowledge the backup, and confirm restart-required storage changes.
8. Saved MySQL / active SQLite / pending restart is expected. Restart the actual service as in B7.
9. Verify accounts, links, permissions, model listing/downloads, and a new backup. Retain the original SQLite and object directory until verified.

Automatic import happens **only at startup**, from an existing valid SQLite source with accounts into a MySQL target containing no accounts or models. Keep the original SQLite path and object directory. A nonempty MySQL target is neither overwritten nor automatically merged. Automatic MySQL→SQLite migration is not provided.

WebUI backups of MySQL produce a SQLite snapshot. B8 explains restoring that snapshot; changing a dropdown does not migrate data in both directions. The MySQL adapter currently serializes SQL through a synchronous connection, so test your intended load.

## A10. Tab 6: Client features — 客户端功能

Both the server and the installed client must support a feature. Enabling a switch does not install/update the mod or create game entities.

| Field / JSON path | Default / range | Meaning |
|---|---|---|
| 实体显示动作 / `features.entity_motion` | Off | Display-action timelines for supported bound maids/fake players |
| 玩家显示状态 / `features.player_display_state` | Off | Supported player display state/variables |
| 载具模型绑定 / `features.vehicle_bindings` | Off | Display data for bound vehicles |
| 弹射物快照 / `features.projectile_snapshots` | Off; unavailable | Not implemented; setting true in JSON is rejected |
| 单条协议消息上限 / `limits.max_message_bytes` | 64 KiB; 1–1024 KiB | Ordinary protocol messages, not uploaded model files |
| 单次实体查询数量 / `limits.max_entity_query_count` | 64; 1–256 | Entity count in one query |
| 显示状态大小上限 / `limits.max_visual_state_bytes` | 8 KiB; 1–64 KiB | Cannot exceed max_message_bytes |
| 显示变量数量上限 / `limits.max_visual_variables` | 32; 0–128 | 0 permits no variables; it does not mean unlimited |
| 显示状态持续发布频率 / `limits.visual_publish_requests_per_second` | 20/sec; 1–1000 | Sustained publication rate |
| 显示状态突发预算 / `limits.visual_publish_burst` | 40; 1–2000 | At least the sustained rate |
| 每个发布者的弹射物预算 / `limits.max_projectile_snapshots_per_publisher` | 256; reserved 1–4096 | Read-only/unavailable; cannot exceed world budget |
| 每个世界的弹射物预算 / `limits.max_projectile_snapshots_per_world` | 4096; reserved 1–65536 | Read-only/unavailable |
| 弹射物闲置保留时间 / `retention.projectile_idle_ttl_seconds` | 600 seconds; reserved 30–86400 | Read-only; cannot exceed maximum lifetime |
| 弹射物最长保留时间 / `retention.projectile_max_lifetime_seconds` | 86400 seconds; reserved 60–604800 | Read-only/unavailable |
| 临时显示数据清理间隔 / `retention.cleanup_interval_seconds` | 60 seconds; 1–3600 | Clears expired temporary display data, not models; cannot exceed idle TTL |

Available settings apply live. Reserved ranges exist for protocol/validation compatibility, not as an indication that a feature is implemented. Do not generalize “0 means unlimited” from concurrency to unrelated fields.

## A11. Tab 7: Logging — 日志

| Field / JSON path | Default | Guidance |
|---|---|---|
| 日志详细程度与模块过滤 / `logging.level` | `info` | error: errors; warn: warnings too; info: everyday use. debug/trace are for investigation and should be turned back down afterward |
| 日志输出格式 / `logging.format` | `text` | Human-readable text, or `json` for log collection tools |

Both require restart. For module filtering, a global `info` plus `spm_cloud=debug` corresponds to `info,spm_cloud=debug`. If you do not know module names, use a global level. Existing legal complex expressions are preserved when not edited.

Runtime log export contains only the **current process's most recent 2 MiB**. Retrieve pre-restart history from systemd, Docker, or your configured process logging.

## A12. Tab 8: Updates and backups — 更新与备份

Owner only. Administrators export audit records from A13.

| Setting/action | Meaning |
|---|---|
| 自动检查并安装更新 / `updates.auto_update` | Off by default. Supported native deployments check/install releases; container images are updated through the deployment platform |
| 更新检查间隔 / `updates.check_interval_hours` | 24 hours; 1–168. Editable when auto-update is enabled; applies live |
| Check update | Fetch version information without installing/restarting |
| Install update | Handle the draft, drain work, verify checksum/version, back up, and schedule restart. Requires writable program directory; native install is unavailable inside containers |
| Full backup | Consistent database snapshot plus model objects in ZIP |
| Model-library archive | Original model content and manifests, not a full account/permission database backup |
| Audit export | All records or search-filtered records |
| Runtime log export | The current process's recent 2 MiB |

After requesting a backup, finish any active-settings confirmation, wait for the browser download to complete, locate the ZIP, and open/extract it. Expect `manifest.json`, `spm-cloud.db`, and object files when present.

The ZIP does **not** include `config.json`, `secrets/`, `.managed-secrets/`, TLS certificates, or Compose environment files. Keep those separately. Restoration is not a ZIP upload button: stop the service and follow B8.

## A13. Tab 9: Audit records — 操作记录

Owners and administrators retain their existing read/export rights.

1. Enter a search and submit, or clear it and search for all records.
2. Use paging, page size, and page-number jump.
3. Export uses the search filter; it is not limited to the currently displayed page.
4. Audit records help establish who changed configuration/accounts/models. They are neither a data backup nor an Undo button.

## A14. Search, optional JSON tools, and disabling the website

Search Chinese labels, descriptions, or technical names such as `Redis` or `max_asset_bytes`. Select a result to jump to its field. The tab row scrolls horizontally and supports arrows/Home/End; the narrow-screen save bar stays at the bottom.

The separate **导入/导出配置** (Import/export configuration) tool is optional. Export represents the draft with credential references, not secret contents or a full data backup. Import enters the same draft and requires validation/saving. Omitted settings may take defaults: do not import a partial JSON fragment as a complete configuration. Prefer forms if unsure.

Before disabling WebUI in A5, copy its actual recovery command. Website/management routes then become unavailable. Run on the server:

~~~sh
spm-cloud --config /ACTUAL-CONFIG-PATH/config.json --enable-web-ui
~~~

Use the actual Windows executable path as shown in B7. Recovery changes the website switch, not accounts/data. A running file watcher adopts the change; otherwise it takes effect on the next start.

---

# B. Command line: installation and maintenance

## B0. Reading the command examples

- Windows blocks use **PowerShell**; Linux/macOS blocks use **Bash/terminal**. Do not paste Bash `sudo` or backslash continuations into PowerShell.
- `./spm-cloud` means the program in the current directory; PowerShell uses `.\spm-cloud.exe`.
- Quote paths containing spaces. PowerShell needs `&` before a quoted executable path.
- Replace example domains/paths. Angle-bracket placeholders are descriptions, not literal arguments.
- Variables defined in a block remain available only in that terminal session. A newly opened terminal needs its own variables.
- Follow one deployment path, not every command in the document. Verify each step before continuing.

Public deployment needs a domain, reachable server, and persistent data location. Public HTTPS normally needs correct DNS and reachable TCP 80/443. If an existing proxy already owns those ports, configure that proxy rather than starting another conflicting instance.

**Open and save the configuration:** on Windows, press Win+R and enter `notepad C:\SPMCloud\config.json`. Confirm it is the initialized file, edit, and press Ctrl+S. On Linux, use an editor such as `sudo nano /etc/spm-cloud/config.json`; for the Compose example use `sudo nano /opt/spm-cloud/deploy/runtime/config.json`. In nano, press Ctrl+O, Enter to save, and Ctrl+X to exit. Install nano with your distribution's package manager if absent, or use a familiar editor. Save plain JSON, not a Word/WPS rich-text document, and keep the name `config.json`, not `config.json.txt`.

**Set DNS at your domain's DNS provider:** create an A record for the chosen hostname pointing to the server's public IPv4. Add AAAA only when public IPv6 actually works. Cloud security groups, host firewalls, and home-router forwarding can each block incoming traffic. Do not use 127.0.0.1, 0.0.0.0, or a Docker-private address as the public DNS destination.

## B1. Windows installation and first start

### Option 1: download and extract the ZIP

1. In [Releases](https://github.com/sdf123098/spm-cloud/releases/latest), download `spm-cloud-x86_64-pc-windows-msvc.zip`. Docker archives and “Source code” are not directly runnable Windows programs.
   Also download the matching `.zip.sha256`. In the download directory, run `Get-FileHash -LiteralPath .\spm-cloud-x86_64-pc-windows-msvc.zip -Algorithm SHA256` in PowerShell. Open the checksum file in Notepad and compare its first 64 hexadecimal characters with the complete Hash output; letter case does not matter. If they differ, download again before running anything.
2. Extract to `C:\SPMCloud`. Confirm `C:\SPMCloud\spm-cloud.exe` exists; do not run inside the ZIP preview.
3. Open PowerShell and run:

~~~powershell
Set-Location C:\SPMCloud
.\spm-cloud.exe --version
.\spm-cloud.exe --init-config C:\SPMCloud\config.json
~~~

Check the version/initialization output. Initialization creates `config.json` and `secrets\bootstrap-token.txt`. Existing installations skip initialization; it does not reset passwords.

4. For a same-computer trial, keep the initial local listener/public placeholder and run:

~~~powershell
.\spm-cloud.exe --config C:\SPMCloud\config.json --check-config
.\spm-cloud.exe --config C:\SPMCloud\config.json
~~~

Leave this terminal running after the listening message. Open `http://127.0.0.1:8787/admin/` and follow A1. Open another PowerShell window for additional commands.

5. Before public use, edit the real configuration with the actual HTTPS root and stable instance ID, configure B4's proxy, and restart.
6. Stop a foreground trial with Ctrl+C. Closing the terminal may also stop the process. For unattended startup, configure an actual Windows service manager or scheduled task with the correct executable, arguments, working directory, and account. Cloud does not register a Windows service automatically.

The optional `start-cloud.ps1` launcher sits beside the executable in the extracted package. It initializes missing configuration, checks it, and starts:

~~~powershell
.\start-cloud.ps1 -ConfigPath C:\SPMCloud\config.json
~~~

If execution policy blocks the script, use the executable commands above. Changing global execution policy is not necessary merely to start Cloud.

### Option 2: official installer

The installer installs the program, not DNS, a system service, or your data configuration. Its entry is:

`https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1`

You can download/inspect the script before running it. A common installation location is `$env:USERPROFILE\.cargo\bin\spm-cloud.exe`; the installer's `SPM_CLOUD_INSTALL_DIR` setting changes the prefix. If unsure about paths, use the ZIP option.

## B2. Docker Compose

This path is for **Linux + Docker Engine + the Compose plugin**. Windows Docker Desktop/NAS panels require their own host paths, permissions, and networking; do not paste Linux shell commands into PowerShell.

Confirm prerequisites:

~~~bash
docker version
docker compose version
~~~

Install missing software using [Docker's installation guide](https://docs.docker.com/engine/install/). This path builds a local image from source. B7 covers loading prebuilt release images.

### Step 1: files, image, and initialization

~~~bash
sudo git clone https://github.com/sdf123098/spm-cloud.git /opt/spm-cloud
cd /opt/spm-cloud
sudo docker compose -f docker-compose.json.yml build spm-cloud
sudo install -d -o 10001 -g 10001 -m 700 deploy/runtime
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --init-config /etc/spm-cloud/config.json
~~~

Wait for image build completion. The one-off `run --rm` container is removed after initialization; mounted configuration/credentials remain. Skip initialization when `deploy/runtime/config.json` already exists.

### Step 2: edit the actual configuration

Open host file `/opt/spm-cloud/deploy/runtime/config.json`. Adjust these values while retaining other generated settings:

~~~json
{
  "schema_version": 1,
  "instance": {
    "instance_id": "my-cloud",
    "origin": "https://cloud.example.com",
    "bind": "0.0.0.0:8787",
    "trusted_proxy_ips": ["172.30.10.3"]
  },
  "storage": {
    "backend": "sqlite",
    "data_dir": "/var/lib/spm-cloud/data",
    "database_path": "/var/lib/spm-cloud/data/spm-cloud.db",
    "object_dir": "/var/lib/spm-cloud/objects"
  },
  "auth": {
    "bootstrap_account_id": "account_local",
    "bootstrap_access_token_file": "secrets/bootstrap-token.txt",
    "allow_self_registration": true
  }
}
~~~

This is a complete minimal **new-instance** example; omitted fields use defaults. Do not replace an existing instance's provider list/credential references/settings with it.

| Host/deployment item | Container location/purpose |
|---|---|
| `./deploy/runtime` | `/etc/spm-cloud`: JSON, secrets, and .managed-secrets |
| Named volume `spm-cloud-data` | `/var/lib/spm-cloud/data`: database |
| Named volume `spm-cloud-objects` | `/var/lib/spm-cloud/objects`: model objects |
| `./Caddyfile.compose` | Proxy site configuration |
| Caddy data/config volumes | Certificate/proxy state, not model data |

Do not use `127.0.0.1:8787` as Cloud's container listener: the separate Caddy container cannot reach Cloud's loopback. Container configuration needs container paths, not arbitrary host `/opt/...` paths that were never mounted.

### Step 3: domain and HTTPS

Edit the first line of `/opt/spm-cloud/Caddyfile.compose` to the same domain as instance.origin. Keep `reverse_proxy spm-cloud:8787` and its forwarding header.

Configure A/AAAA DNS for the actual server. Retain AAAA only when IPv6 is reachable. Allow inbound TCP 80/443. The default Compose publishes proxy ports rather than Cloud's backend 8787.

The default proxy IP is `172.30.10.3` in `172.30.10.0/24`. If the subnet conflicts, change both Compose network/proxy values and the JSON trusted-proxy IP.

### Step 4: check and start

~~~bash
cd /opt/spm-cloud
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo docker compose -f docker-compose.json.yml up -d
sudo docker compose -f docker-compose.json.yml ps
sudo docker compose -f docker-compose.json.yml logs --tail 100 spm-cloud caddy
~~~

Success means running services, no configuration/permission errors, and a reachable `https://YOUR-DOMAIN/admin/`. Read the token:

~~~bash
sudo cat /opt/spm-cloud/deploy/runtime/secrets/bootstrap-token.txt
~~~

Complete A1. Automatic certificates require the network/domain conditions described by [Caddy](https://caddyserver.com/docs/automatic-https).

Keep the same deployment directory, Compose file, and volume names. **Do not upgrade/fix with `docker compose down -v`**: the volume-removal flag can delete persistent data.

## B3. Native Linux and macOS

### Linux download and initial setup

Choose `spm-cloud-x86_64-unknown-linux-gnu.tar.xz` or `spm-cloud-aarch64-unknown-linux-gnu.tar.xz` plus its same-name `.sha256`. `uname -m` reports architecture: x86_64 → x64; aarch64/arm64 → ARM64. These are GNU targets, not universal Alpine/musl binaries.

With an x64 download in the current directory:

~~~bash
sha256sum -c spm-cloud-x86_64-unknown-linux-gnu.tar.xz.sha256
mkdir -p unpacked
tar -xJf spm-cloud-x86_64-unknown-linux-gnu.tar.xz -C unpacked
~~~

Checksum must report OK. Enter the extracted directory actually containing `spm-cloud`, then:

~~~bash
./spm-cloud --version
sudo install -m 755 ./spm-cloud /usr/local/bin/spm-cloud
sudo useradd --system --home /var/lib/spm-cloud --create-home --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
~~~

Skip user creation when the service account already exists.

Edit the actual public root, stable ID, and native listener. The following is the **storage object**, not an entire config file:

~~~json
{
  "data_dir": "/var/lib/spm-cloud/data",
  "database_path": "/var/lib/spm-cloud/data/spm-cloud.db",
  "object_dir": "/var/lib/spm-cloud/objects",
  "backend": "sqlite",
  "mysql_url_file": null
}
~~~

Keep the initialized token reference and other settings.

### systemd

The unit `deploy/spm-cloud-json.service` comes from the **source repository**, not necessarily the native binary archive. Obtain deployment templates:

~~~bash
sudo git clone https://github.com/sdf123098/spm-cloud.git /opt/spm-cloud
sudo cp /opt/spm-cloud/deploy/spm-cloud-json.service /etc/systemd/system/spm-cloud-json.service
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-json.service
sudo systemctl status spm-cloud-json.service
~~~

Reuse an existing source checkout instead of cloning over it. The template uses user `spm-cloud`, executable `/usr/local/bin/spm-cloud`, configuration `/etc/spm-cloud/config.json`. Adjust the unit first if your paths differ.

~~~bash
sudo journalctl -u spm-cloud-json.service -n 100 --no-pager
sudo systemctl restart spm-cloud-json.service
~~~

The hardened unit permits writes to `/var/lib/spm-cloud` and `/etc/spm-cloud`. Other data locations need both filesystem permissions and an appropriate `ReadWritePaths` change. Configure B4 before public use.

### macOS

Apple Silicon uses `aarch64-apple-darwin`; Intel uses `x86_64-apple-darwin`. Extract, enter the directory, run `./spm-cloud --version`, initialize JSON as in B5, and start with `--config`. macOS does not use systemd; configure launchd/another process manager and HTTPS for unattended use.

## B4. HTTPS, proxies, ports, and public access

The whole route must work:

**Player/browser → public HTTPS domain:443 → Caddy/Nginx → Cloud HTTP listener**

instance.origin advertises the public address; instance.bind chooses the listener. Neither creates DNS records, forwards router ports, or issues TLS certificates.

### Same-host native Caddy

Install/manage Caddy separately. The repository's `deploy/Caddyfile.native` contains:

~~~caddyfile
cloud.example.com {
    reverse_proxy 127.0.0.1:8787 {
        header_up X-SPM-Client-IP {http.request.remote.host}
    }
}
~~~

Cloud should use origin `https://cloud.example.com`, bind `127.0.0.1:8787`, trusted proxies `["127.0.0.1"]`. This snippet belongs in the running Caddy configuration, not config.json. A proxy on another machine/network namespace needs a different reachable upstream.

### Nginx, body size, and WebSocket

`nginx-cloud.conf.example` is a snippet for an existing HTTPS server block, not a complete certificate-bearing nginx.conf. Retain HTTP/1.1, Upgrade/Connection, and client-IP headers. Proxy the website, `/v1`, and `/v1/realtime`, not just /admin/.

The default 128 MiB model limit corresponds to `client_max_body_size 128m;`. Increase the proxy limit when increasing Cloud's limit, or uploads can be rejected before reaching Cloud.

The proxy limits the complete request, including multipart-form overhead. A model near 128 MiB can therefore still exceed a 128m request limit. For the full 128 MiB file allowance, leave proxy headroom, for example `client_max_body_size 136m;`, while keeping Cloud's own 128 MiB per-file limit. Check Nginx configuration before reloading it through your actual service manager.

### Check from inside outward

1. For a native local listener, check `http://127.0.0.1:8787/health` on the Cloud host. This host URL is not expected to work when Docker deliberately publishes no host 8787.
2. Inspect Cloud's startup/listener logs.
3. Inspect proxy upstream/certificate logs.
4. Test HTTPS /health and /admin/ from an external network.
5. Finally test mod sign-in and model browsing using the root address.

## B5. Configuration, paths, and credential files

### What each CLI mode does

Here `spm-cloud` means an executable available on PATH. On Windows without PATH, substitute `& "C:\SPMCloud\spm-cloud.exe"`.

| Command | Effect |
|---|---|
| `spm-cloud --version` | Print version and exit |
| `spm-cloud --help` | Print supported arguments without changes |
| `spm-cloud --init-config /PATH/config.json` | Initialize a new configuration/random token; refuses overwriting existing configuration/credentials; not password recovery |
| `spm-cloud --config /PATH/config.json --check-config` | Read/validate configuration and referenced credentials; no listener, database creation, or migration; does not prove external connectivity |
| `spm-cloud --config /PATH/config.json --print-effective-config` | Print redacted effective values/paths/sources without starting; inspect for internal paths/addresses before sharing |
| `spm-cloud --config /PATH/config.json` | Run the service in the foreground; does not register a background system service |
| `spm-cloud --config /PATH/config.json --enable-web-ui` | Enable the website in a valid writable file without resetting accounts/migrating data; cannot fix unrelated invalid configuration |

Choose either check or print, not both. Do not combine initialization/help/version/recovery indiscriminately with other modes. There is no extra CLI option to reset every account, repair a database, or change an existing owner password.

### Configuration selection

1. Explicit `--config PATH`.
2. `SPM_CLOUD_CONFIG`.
3. `config.json` in the process working directory.
4. ENV-only compatibility mode if none was selected.

A selected missing/invalid file causes an error rather than silent ENV fallback. With JSON selected, business ENV values and RUST_LOG do not override JSON. Omitted fields use defaults. Deployment indicators such as the container flag are separate from business-field overrides.

Use explicit --config for predictable operation.

### Relative paths

For `C:\SPMCloud\config.json`:

| Value | Resolves to |
|---|---|
| `"data_dir": "data"` | `C:\SPMCloud\data` |
| `"database_path": null` | `spm-cloud.db` inside data_dir |
| `"object_dir": null` | `objects` inside data_dir |
| `"bootstrap_access_token_file": "secrets/bootstrap-token.txt"` | `C:\SPMCloud\secrets\bootstrap-token.txt` |
| `"database_path": "data/spm-cloud.db"` | `C:\SPMCloud\data\spm-cloud.db`; not data/data/... |
| `"object_dir": "D:/CloudObjects"` | The absolute directory, independent of the config directory |

Use JSON `"C:/SPMCloud/data"` or escaped `"C:\\SPMCloud\\data"`, not raw unescaped backslashes. Docker paths are inside the container; browser-local paths are not server paths.

### Read the initial token

Windows:

~~~powershell
Get-Content -LiteralPath C:\SPMCloud\secrets\bootstrap-token.txt
~~~

This guide's native Linux:

~~~bash
sudo -u spm-cloud cat /etc/spm-cloud/secrets/bootstrap-token.txt
~~~

This guide's Docker:

~~~bash
sudo cat /opt/spm-cloud/deploy/runtime/secrets/bootstrap-token.txt
~~~

After rotation the active reference can point into .managed-secrets. Read the current `auth.bootstrap_access_token_file` reference rather than assuming the original file remains current.

### Editing JSON safely

1. Back up configuration and credential files.
2. Find the JSON path in A5–A12; cache.backend means backend inside the cache object.
3. Edit only intended fields. Use UTF-8 JSON, quoted keys/strings, no comments/trailing commas, and real `true/false` rather than strings.
4. Check configuration before starting/restarting. A running process polls file changes every two seconds and retains its previous active configuration on invalid updates.
5. Inspect restart requirements. Network identity, storage, and logging remain startup-only.

Keep `schema_version` as the integer `1`: it is a configuration-format version, not the program's `2.4.0` version. `null` means no explicit value and differs from an empty string `""`, `0`, or `false`; only use it where that field allows it. Configuration keys are case-sensitive.

Units: UI 128 MiB = JSON `134217728` bytes; 64 KiB = `65536`; UI 70% = JSON `0.7`. Seconds/milliseconds/hours are not interchangeable. Only fields explicitly documented as unlimited interpret 0 that way.

### Credential files for file-only configuration

The `*_file` field is a path. The file contains a single secret value, not a JSON object or explanatory text.

| Reference | File content |
|---|---|
| `auth.bootstrap_access_token_file` | Owner bootstrap token |
| `auth.bootstrap_password_hash_file` | Full Argon2id hash, not a plain password |
| `storage.mysql_url_file` | Full MySQL URL, structurally `mysql://USER:ENCODED_PASSWORD@HOST:3306/DATABASE` |
| `cache.redis_url_file` | Redis URL; `rediss://` for a supported TLS endpoint |
| `web_ui.microsoft_client_secret_file` | Client secret Value |

Percent-encode reserved characters in URL usernames/passwords, not the entire URL. Prefer structured WebUI fields if unsure.

On POSIX, credential files are commonly restricted to 600 and their service-owned directory to authorized users; configure equivalent Windows permissions. The service also needs write permission on the configuration directory for atomic replacement and managed-secret creation. A writable file inside an unwritable parent is insufficient.

A's tables cover all 56 business fields and provider subfields. [config.schema.json](config.schema.json) defines exact types/bounds; [config.example.json](config.example.json) is an example, not a production-file replacement.

## B6. Scanner, external MySQL, and Redis

### Local NudeNet adapter

Python helper scripts require a separate Python installation; the native Cloud binary does not. On Windows, confirm `py --version` works. On Linux, confirm `python3 --version` and install pip/venv support (`python3-pip` and `python3-venv` packages on Debian/Ubuntu). A missing command means Python setup is incomplete, not that Cloud configuration is corrupt. B8's backup scripts use the same Python installation.

Run on the **Cloud server**. From the source repository root:

~~~bash
python3 -m venv .venv-moderation
.venv-moderation/bin/python -m pip install -r tools/requirements-moderation.txt
.venv-moderation/bin/python tools/nudenet_service.py --bind 127.0.0.1 --port 8080
~~~

Windows PowerShell from the source root:

~~~powershell
py -m venv .venv-moderation
.\.venv-moderation\Scripts\python.exe -m pip install -r tools/requirements-moderation.txt
.\.venv-moderation\Scripts\python.exe tools\nudenet_service.py --bind 127.0.0.1 --port 8080
~~~

In an **extracted release package**, remove the tools/ prefix: the scripts/dependency file are in the package root.

In another terminal verify `http://127.0.0.1:8080/health`, then set Cloud's scanner URL to `http://127.0.0.1:8080/infer`. This works when native Cloud and the scanner share a host. Inside a container, localhost means that container, not the host; use an intentionally reachable controlled address.

This adapter has no public-facing account/login layer. Do not expose it as a public model server. Arrange process management/resource limits for unattended use.

### External databases/cache

Provision MySQL/Redis first. Cloud does not install them or create a MySQL database/user. Verify networking and permissions before A8/A9's checks.

SQLite/MySQL provide durable state; Redis is disposable cache. Clearing Redis is not account-database recovery.

Existing MySQL URL mode can use connection options supported by the driver. The structured editor's TLS checkbox is for **Redis**, not a universal MySQL TLS configuration. Website HTTPS does not encrypt the separate database link; use appropriate database transport/certificates/network controls.

## B7. Stop, restart, update, and recover the website

### Identify the process manager

| Deployment | Stop/restart |
|---|---|
| Foreground Windows/Linux/macOS | Ctrl+C in its original terminal; start again with the original --config |
| This guide's systemd | `sudo systemctl stop spm-cloud-json.service` / `sudo systemctl restart spm-cloud-json.service` |
| This guide's Compose | In the original directory, `sudo docker compose -f docker-compose.json.yml stop spm-cloud` / `sudo docker compose -f docker-compose.json.yml restart spm-cloud` |
| Your own service/task | Use its actual name rather than assuming this guide's unit exists |

Arrange maintenance for player connections. Do not run a second foreground instance alongside a background instance on the same port/database.

### Website disabled: offline recovery

Windows:

~~~powershell
& "C:\SPMCloud\spm-cloud.exe" --config "C:\SPMCloud\config.json" --enable-web-ui
~~~

Native Linux:

~~~bash
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --enable-web-ui
~~~

Docker in the original deployment directory:

~~~bash
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --config /etc/spm-cloud/config.json --enable-web-ui
~~~

Wait for a running watcher's adoption, then refresh the original URL. If stopped, start normally. Wrong paths, invalid configuration, and missing write permissions must be corrected first.

### Manual native upgrade

1. Record old version, program/configuration paths, and startup method.
2. Back up as in B8 and retain the old program.
3. Download the correct platform package plus SHA-256 and verify it.
4. Stop Cloud. Windows generally cannot overwrite a running executable.
5. Replace the program without replacing your configuration, secrets, or data. Do not initialize again. Keep the new schema/helpers with the matching version.
6. Run --version and --config ... --check-config, then use the original startup method.
7. Verify health, sign-in, permissions, and actual model downloads before restoring update schedules.

An old executable is not a database rollback. If a newer schema is incompatible with the old program, restore the matching pre-upgrade database/objects too.

### Load prebuilt Docker release archives

Release files `spm-cloud-docker-linux-amd64.tar.gz` and `spm-cloud-docker-linux-arm64.tar.gz` are **docker load archives**, not native executables.

With the correct amd64 archive/checksum in the current directory:

~~~bash
sha256sum -c spm-cloud-docker-linux-amd64.tar.gz.sha256
sudo docker load -i spm-cloud-docker-linux-amd64.tar.gz
~~~

Read the loaded image tag, e.g. `spm-cloud:v2.4.0`. Set `SPM_CLOUD_IMAGE=spm-cloud:v2.4.0` in the deployment's .env, using the actual loaded version and retaining other existing values. Then:

~~~bash
sudo docker compose -f docker-compose.json.yml up -d --no-build spm-cloud
sudo docker compose -f docker-compose.json.yml logs --tail 100 spm-cloud
~~~

Restart alone uses the existing container/image; up recreates it for a new image. Keep volume names and configuration. Compose .env substitutions for images/mounts are not JSON business-field overrides.

### Optional automatic update: choose one owner of the process

Do not let multiple updaters race over the same installation.

- **WebUI native updater:** A12. Needs a writable program directory and tar/xz on Unix. The hardened service user generally cannot write /usr/local/bin; use a controlled external updater/manual maintenance rather than making the directory writable by everyone.
- **Native Linux timer:** `deploy/native-auto-update.sh` with `spm-cloud-native-update.service/.timer`. Needs curl, jq, tar/xz, sha256sum, flock, systemd. Its default application service is **spm-cloud.service**, while this guide installs **spm-cloud-json.service**: correct the name explicitly.
- **Compose timer:** `deploy/spm-cloud-json-auto-update.service/.timer` and `deploy/docker-auto-update.sh`; default checkout /opt/spm-cloud, Compose file docker-compose.json.yml, Linux amd64/arm64.
- **Windows script:** `deploy/windows-auto-update.ps1` operates an existing application scheduled task. It does not register that task. Check ApplicationTaskName, ExecutablePath, and HealthUrl before use.

Native Linux timer example:

~~~bash
sudo install -d -m 755 /usr/local/libexec
sudo install -m 755 /opt/spm-cloud/deploy/native-auto-update.sh /usr/local/libexec/spm-cloud-native-update.sh
sudo cp /opt/spm-cloud/deploy/spm-cloud-native-update.service /opt/spm-cloud/deploy/spm-cloud-native-update.timer /etc/systemd/system/
sudo systemctl edit spm-cloud-native-update.service
~~~

Save this drop-in:

~~~ini
[Unit]
After=
After=network-online.target spm-cloud-json.service
Requires=
Requires=spm-cloud-json.service

[Service]
Environment=SPM_CLOUD_SYSTEMD_SERVICE=spm-cloud-json.service
Environment=SPM_CLOUD_BINARY=/usr/local/bin/spm-cloud
Environment=SPM_CLOUD_AUTO_UPDATE_HEALTH_URL=http://127.0.0.1:8787/health
~~~

Run `sudo systemctl daemon-reload`, manually start `spm-cloud-native-update.service`, and inspect its journal. Only after success enable `spm-cloud-native-update.timer`. Disable scheduled checks with `sudo systemctl disable --now spm-cloud-native-update.timer`; Cloud itself need not stop.

For Compose, copy the JSON auto-update templates to `/etc/systemd/system/spm-cloud-auto-update.service` and `spm-cloud-auto-update.timer`. Adjust paths first, manually run/check the service, then enable the timer. These updaters follow the latest stable release, not a permanent 2.4.0 pin.

## B8. Backup and restore

### Keep everything needed for recovery

| Item | Why |
|---|---|
| Consistent database + model objects | Accounts/permissions/links/revisions must match the actual files |
| config.json | Identifies the right database, directories, providers, and credential references |
| secrets and .managed-secrets | Contains bootstrap/database/cache/Microsoft credentials, omitted from the WebUI backup |
| Deployment/proxy files, certificates/volumes, old program version | Recreates the environment and supports compatible rollback |

Store a safe separate copy and verify it is readable. A copy on the same disk you are about to delete/replace is insufficient.

### Offline SQLite backup

Stop Cloud first. From the **source repository root**:

~~~bash
python3 tools/backup_restore.py backup --database /ACTUAL-DATA/spm-cloud.db --objects /ACTUAL-OBJECTS/objects --destination /NEW-BACKUP/cloud-backup-20261016
~~~

Windows from an **extracted release directory**:

~~~powershell
py .\backup_restore.py backup --database C:\SPMCloud\data\spm-cloud.db --objects C:\SPMCloud\data\objects --destination D:\CloudBackups\cloud-backup-20261016
~~~

Choose a new destination. The tool checks SQLite, object hashes/lengths, and paths. Do not mix ordinary file copies of live database/objects from different times. This tool reads SQLite files, not MySQL URLs; use a WebUI full backup or an appropriate MySQL backup process for MySQL.

### Restore a WebUI ZIP or an offline backup

1. Stop Cloud and any updater/task that can restart it.
2. Retain current damaged data separately.
3. Extract a WebUI ZIP. Locate the directory directly containing manifest.json, spm-cloud.db, and objects when present.
4. Choose **new** destination paths for the database and model directory.
5. From the source root:

~~~bash
python3 tools/backup_restore.py restore --backup /EXTRACTED-BACKUP-ROOT --database /srv/cloud-restore/spm-cloud.db --objects /srv/cloud-restore/objects
~~~

Windows from a release directory:

~~~powershell
py .\backup_restore.py restore --backup D:\CloudBackups\unpacked --database C:\CloudRestore\spm-cloud.db --objects C:\CloudRestore\objects
~~~

6. Point configuration at the restored paths. The snapshot is SQLite, so set storage.backend to sqlite. Restore matching configuration/secrets and permissions, correcting references when paths changed.
7. Check configuration, start, and verify sign-in, links, permissions, and original model download.
8. Decide how to archive old data only after success, then resume schedules.

Existing destinations are rejected by default. `--force` overwrites the database and rebuilds the destination object directory, potentially deleting its contents. Use it only after stopping and preserving originals; default examples deliberately omit it. Backup/destination paths must not overlap or contain one another; symlinks/invalid hashes are rejected.

### Docker backups

The guide's database/models live in named volumes. Copying deploy/runtime alone backs up configuration/credentials, not accounts/model objects. The WebUI backup plus separately retained runtime/deployment/proxy state is usually easier. For offline volume backups, stop writes and follow Docker's volume-backup process, keeping database/objects from one consistent point. Down -v is not a backup command.

## B9. ENV compatibility and migration

### Why editing .env may do nothing

Rust does not automatically load .env/.env.native. Compose reads substitutions but only passes variables specified by its template. With JSON selected, business ENV values do not override it. ENV-only WebUI settings are read-only because the website cannot rewrite your system service/container environment.

| ENV name | JSON equivalent / purpose |
|---|---|
| `SPM_CLOUD_CONFIG` | Select the file |
| `SPM_CLOUD_INSTANCE_ID` / `SPM_CLOUD_ORIGIN` / `SPM_CLOUD_BIND` | instance.instance_id/origin/bind |
| `SPM_CLOUD_TRUSTED_PROXY_IPS` | instance.trusted_proxy_ips; comma-separated in ENV |
| `SPM_CLOUD_DATA_DIR` / `SPM_CLOUD_DATABASE` / `SPM_CLOUD_OBJECT_DIR` | storage.data_dir/database_path/object_dir |
| `SPM_CLOUD_ACCESS_TOKEN` | Move the value into a file referenced by auth.bootstrap_access_token_file |
| `SPM_CLOUD_BOOTSTRAP_ACCOUNT` | auth.bootstrap_account_id; retain the existing owner |
| `SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` | A file referenced by auth.bootstrap_password_hash_file |
| `SPM_CLOUD_ALLOW_SELF_REGISTRATION` | auth.allow_self_registration |
| `SPM_CLOUD_MAX_ASSET_BYTES` / `SPM_CLOUD_MAX_MESSAGE_BYTES` | limits.max_asset_bytes / limits.max_message_bytes; bytes in both ENV and JSON |
| `SPM_CLOUD_IDENTITY_PROVIDERS` / `SPM_CLOUD_HAS_JOINED_URL` | Legacy provider list / single gateway; choose one and retain stable IDs on migration |
| `SPM_CLOUD_MYSQL_URL` | Compatibility URL source corresponding to storage.mysql_url_file; a URL alone does not select MySQL mode |
| `SPM_CLOUD_REDIS_URL` | Compatibility URL source corresponding to cache.redis_url_file; a URL alone does not enable Redis |
| `SPM_CLOUD_MICROSOFT_CLIENT_SECRET` | Compatibility secret source; JSON uses web_ui.microsoft_client_secret_file. A secret alone does not configure an application ID |
| `RUST_LOG` | Legacy ENV filter; JSON uses logging.level |
| `SPM_CLOUD_CONTAINER` | Deployment marker controlling native-update availability |
| `SPM_CLOUD_IMAGE`, `SPM_CLOUD_CONFIG_DIR`, `SPM_CLOUD_DATA_VOLUME`, `SPM_CLOUD_OBJECTS_VOLUME`, `SPM_CLOUD_NETWORK_SUBNET`, `SPM_CLOUD_PROXY_IP`, `SPM_CLOUD_HOST` | Compose/proxy template substitutions, not arbitrary JSON overrides |

Do not invent ENV names for every advanced field. Use JSON or WebUI for complete configuration. New deployments use docker-compose.json.yml; docker-compose.yml and .env.example retain legacy compatibility.

### Move an existing instance to JSON

1. Record version, startup command, working directory, ENV source, owner ID/token, absolute database/object paths, and volume names.
2. Stop and back up. Do not combine configuration migration with a database-backend switch.
3. Initialize JSON in a **new separate configuration location**, then point it to the original data. Do not accidentally use the new empty data directory.
4. Store the old token/hash in protected files; retain owner/provider IDs, domain, proxy settings, and registration policy.
5. Check configuration as the actual service account.
6. Update the process manager/Compose command to explicit --config. Keep existing volume names and correct container paths.
7. Start and verify original accounts/links/permissions/models/downloads, then writable WebUI settings.
8. Keep old ENV and backups for rollback. Bootstrap hashes do not overwrite existing account passwords.

`tools/migrate_legacy.py` inventories/stages old model files; it is not an ENV→JSON converter or proof of game identity. Read --help first. A read-only inventory example:

~~~bash
python3 tools/migrate_legacy.py /OLD-MODEL-ROOT --output /SAFE-LOCATION/legacy-inventory.json
~~~

--import-manifest plans by default; explicit --apply-import copies. Use a staging destination rather than a running production directory. Review ownership/identity mappings; names alone are not verified identities.

## B10. Building, API, and reference files

Operators can use release binaries without installing Rust/Node. Developers need **Rust 1.88+**:

~~~bash
cargo build --release --locked
cargo fmt --all -- --check
cargo check --all-targets --all-features --locked
cargo test --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
node --check web/admin.js
node --check web/settings.js
node --test web/preview.test.cjs web/settings.test.cjs
~~~

Only changes to preview.js require Node.js 22 and, in web/, npm ci followed by npm run build. The preview bundle is committed. npm test in web includes preview and settings tests.

SPM_TEST_MYSQL_URL/SPM_TEST_REDIS_URL are for dedicated isolated test services. Some tests return early without them; a green framework result is not proof of real external-service coverage. Never run destructive integration tests against production. tools/service_contract.py --binary PATH checks a compiled service in isolation; --model is optional.

Discovery is /v1/instance; business endpoints are under /v1; realtime uses /v1/realtime; health is /health. Configuration APIs use revision conflict protection, validation, fixed-kind connection checks, and provider checks. Use WebUI if you do not need API automation. Original models retain bytes/hash/version; downloads support ETag/single-range requests.

| Reference | Source repository | Extracted binary release |
|---|---|---|
| Protocol | docs/PROTOCOL_V1.md | PROTOCOL_V1.md |
| Backup/restore | tools/backup_restore.py | backup_restore.py |
| Legacy inventory | tools/migrate_legacy.py | migrate_legacy.py |
| Scanner/dependencies | tools/nudenet_service.py, tools/requirements-moderation.txt | Same basenames in package root |
| Windows launcher | deploy/start-cloud.ps1 | start-cloud.ps1 |
| systemd, external updaters, Compose | deploy/ and repository root | Obtain separately from source; not every native archive contains them |
| Configuration/schema/notices | Repository root and web/credits.txt | Package root and credits.txt |

[Protocol reference](https://github.com/sdf123098/spm-cloud/blob/main/docs/PROTOCOL_V1.md). Licensed under [MIT](LICENSE); font/icon notices are available in the website and bundled credits. Offline authentication fixtures contain synthetic public keys/certificates/public signatures, not private keys or real account credentials. Production trust roots cannot be replaced through requests or ENV.

---

# Troubleshooting

Work through checks in order. Do not start by deleting databases/volumes or initializing a new instance.

| Symptom | First checks | Success means |
|---|---|---|
| Executable window immediately closes | Run from PowerShell and read the actual error; check file/port | Process stays running with a listener message |
| Works locally, not for friends | 127.0.0.1 means the visitor's own computer; inspect domain/proxy/DNS/firewall | External HTTPS /health and /admin/ work |
| Docker 502 | Cloud listens on 0.0.0.0; proxy points to spm-cloud:8787; inspect logs/network | Proxy can reach upstream |
| Port occupied | Existing background/foreground duplicate; do not kill unknown processes | Only intended instance owns the port |
| Empty token field | Read current token reference on the server | Complete current token signs in |
| Old token stopped working | Rotation/reference changed; original file may be obsolete | Current token or owner password works |
| Forgotten owner password | Use retained bootstrap token to change it; if both lost, restore trusted credentials while stopped | Original account/data retained, not a new empty instance |
| Administrator controls disabled | Expected authorization | Owner can edit permitted settings |
| Owner cannot save | ENV read-only? Correct JSON? Directory/file write permission? Conditional controls? | Validate/save succeeds with accurate active/restart state |
| HTTP 409 | Another writer changed configuration | A2 conflict comparison/rebase/save succeeds |
| Saved network/log setting unchanged | Restart required, browser reload insufficient | Active equals saved after restart |
| Upload 413 | File size, Cloud limit, proxy body limit | A file within both limits uploads |
| Private model inaccessible | Linked Cloud account, ownership/sharing, moderation | Authorized account can download; unauthorized cannot |
| All data seems missing | Wrong/new database/object path, working directory, or volume | Original verified data location is in use |
| Scanner fails/everything unknown | /infer contract, process/address, timeout/budgets, parser | Actual model classification succeeds, not just health |
| Strict policy blocks every upload | Reject-unclassified combined with disabled/failed detection | Scanner/policy match intended test outcomes |
| Provider check fails | Real HTTPS API root, public DNS, explicit port, redirects | Accurate check result, then separate real-account test |
| Microsoft callback fails | ID, secret value/expiry, personal-account support, exact redirect, network | Restarted flow completes in original browser |
| MySQL connection/migration fails | Actual host/port, existing database, privileges/password/TLS, source/target import conditions | Connection works; active backend correct after restart |
| Redis fails | Host/ACL/password/TLS/index, container-localhost mistake | Normal connection works and outage fallback remains available |
| Update unavailable | Container/platform update path, native permissions/extraction tools/package | Correct method yields healthy expected version |
| Old logs absent | Only current process's recent 2 MiB is exported | Retrieve history from Docker/systemd/log storage |
| Restore refuses overwrite | Existing target, overlap, hash mismatch, symlink | Valid backup restores into a new safe destination |
| Website disabled | Actual config path and B7 recovery command | /admin/ returns, without account reset |
