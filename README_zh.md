# SPM Cloud 从零使用手册

**简体中文** · [English](README.md) · [下载正式版](https://github.com/sdf123098/spm-cloud/releases/latest)

适用于 **SPM Cloud 2.4.x**。SPM Cloud 是 SparkleMorpher 模组连接的自建服务，负责账号、模型、权限和显示数据同步。管理网页已经包含在程序里，不需要再部署一个前端。

**第一次使用，先选一条路：**

| 你现在的情况 | 从这里开始 |
|---|---|
| 只想在 Windows 电脑上先打开网页试用 | [B1：Windows 安装和第一次启动](#b1-windows-安装和第一次启动) |
| 要在 Linux 服务器上长期运行 | [B2：Docker Compose 部署](#b2-docker-compose-部署) 或 [B3：Linux 原生服务](#b3-linux-原生服务) |
| 程序已经运行，想在网页中管理 | [A1：第一次登录](#a1-第一次登录与三种账号) |
| 只是一名玩家 | [A4：玩家登录与绑定](#a4-玩家登录与绑定) |
| 从旧 ENV 部署升级，网页不能保存 | [B9：旧 ENV 模式与迁移](#b9-旧-env-模式与迁移) |
| 网页被关闭、密码忘了、上传失败 | [故障排查](#故障排查按现象处理) |

本文分为 **A：WebUI 网页操作** 和 **B：命令行、安装与运维**。先完成一次安装启动，之后大部分日常管理都可以在 A 部分完成。

## 开始前先分清这几件事

| 名词 | 用普通话解释 |
|---|---|
| 服务器 | 运行 SPM Cloud 程序的那台电脑；可以是云服务器，也可以是你自己的电脑。不要求它与 Minecraft 服务端运行在同一台机器上。 |
| 操作电脑 | 你正在用来打开浏览器的电脑。你在网页选择上传文件时，选的是这台电脑上的文件。 |
| Cloud 根地址 | 例如 `https://cloud.example.com`。玩家在模组里填这个地址，**不加** `/admin/`、`/account/` 或 `/v1`。 |
| 管理网页 | 根地址后加 `/admin/`，供服主和管理员使用。 |
| 账号中心 | 根地址后加 `/account/`，供玩家登录、查看和验证自己的绑定。 |
| 监听地址 | 程序实际接收请求的位置，如 `127.0.0.1:8787`。它与玩家连接的公网 HTTPS 地址不是一回事。 |
| 反向代理 | Caddy/Nginx 先接收公网 HTTPS，再转发给 Cloud。Cloud 自己的监听端口提供 HTTP，不会因为填写了 HTTPS 地址就自动生成证书。 |
| 配置文件 | 通常叫 `config.json`，记录开关、端口、目录和凭据文件路径。 |
| 数据库 / 模型目录 | 数据库保存账号、权限、绑定等；模型目录保存模型原始文件。备份恢复通常需要两者一起处理。 |
| 初始令牌 | 初始化生成的一串服主管理凭据，可直接取得最高权限；不是玩家密码，也不是微软或第三方密码。 |
| 重启服务 | 真正停止并重新启动 Cloud 进程/容器；刷新浏览器不算重启。 |

本文的 `cloud.example.com` 是示例域名，必须换成你的域名。`C:\SPMCloud`、`/etc/spm-cloud` 等是本文选定的示例目录；已经部署过的实例应使用原目录，不要为了与本文一致而重新建一套空数据库。

---

# A. WebUI：在浏览器里操作

## A1. 第一次登录与三种账号

**前提：** Cloud 已经启动。未安装请先按 B1/B2/B3 操作。

1. 打开 `https://你的域名/admin/`。如果只是同一台电脑上的本机试用，打开 `http://127.0.0.1:8787/admin/`。
2. 展开“**服主使用初始令牌登录**”。
3. 到**运行 Cloud 的服务器**读取令牌文件。默认在配置文件旁边的 `secrets/bootstrap-token.txt`。具体命令见 [B5](#b5-配置文件路径和秘密文件)。
4. 复制文件里完整的一行，粘贴到网页输入框，登录。网页不会自动替你读取服务器文件。
5. 打开“**服务器配置 → 账号与认证 → 更改服主密码**”，设置至少 8 个字符的新密码，点击“**更新密码并重新登录**”。
6. 以后可以使用服主账号 ID 和这个密码登录。默认服主 ID 是 `account_local`；以页面实际显示为准。令牌仍需单独保管。

“账号密码登录”“初始令牌登录”“微软/第三方游戏身份登录”是不同入口。**不会因为某个游戏账号拥有正版，就自动获得服主管理权限。**

| 身份 | 可以做什么 | 不能做什么 |
|---|---|---|
| 服主 | 管理模型；创建/停用账号和分配管理员；修改全部可用配置；连接检查；更新、备份与导出 | 不能把当前未实现的能力变成已实现；固定服主 ID 不在网页更换 |
| 管理员 | 模型管理；只读查看服务器配置；搜索和导出操作记录；管理自己的绑定 | 不能保存服务器配置、做服务/数据库连接检查、分配管理员、安装更新，或导出完整备份/模型库归档/运行日志 |
| 普通账号 | 在模组使用自己有权限的内容；在账号中心验证和管理自己的身份关联 | 不能因为登录了账号中心就进入管理控制台 |

权限由后端检查。管理员看到灰色设置是正常现象。纯 ENV 启动时，服主的配置页面也只读；解决方法在 B9。

**保管令牌：** 不要把它贴进聊天、问题反馈、截图或 Git 仓库。需要更换时使用 A6 的令牌操作。忘记密码但还有初始令牌时，可以用令牌登录后重新设置密码。

### 给别人创建账号、授权和重置密码

这部分由**服主**操作，不在九个配置标签里：

1. 点击侧栏“**账号与权限**”，找到“**创建并分发账号**”。
2. 填账号 ID：以英文字母或数字开头，其后可用英文字母、数字、点、下划线、短横线，最长 128 个字符，例如 `player_001`。这是 Cloud 登录 ID，不是游戏角色名或邮箱。
3. 填至少 8 个字符的初始密码。不要把初始令牌当成分发给其他人的密码。
4. 选择“普通账号”或“管理员”。只是让玩家登录、使用私有模型时选普通账号；管理员可管理模型，应只授予需要管理的人。
5. 点击“**创建账号**”，看到成功结果后，通过自己的安全渠道把账号 ID、密码和 Cloud 根地址交给对应的人。网页不会代发邮件或私信。
6. 在下面的账号列表中，可按 ID 搜索并翻页。对非服主账号可“授权管理员 / 撤销管理权”“停用 / 启用”“重置密码”。操作立即提交，权限变化及重置密码会使已有会话失效，相关用户需要重新登录。

停用保留账号，不等于删除模型；重新启用也不替用户重置密码。服主账号不能在这里降权、停用或转让。关闭自助注册后，仍可由服主按上述步骤创建账号。

## A2. 先学会“改了以后怎样才算生效”

侧栏下部的“**服务器配置**”位于当前账号上方；“更新与导出”和“操作记录”已经合并进配置页。配置页有九个标签，右侧或底部有统一保存栏。

一次普通修改按这个顺序做：

1. 打开对应标签，修改字段。
2. 看保存栏出现“几项待保存”。此时只是**草稿**，服务器还没有采用。
3. 点击“**校验草稿**”。有错误时按提示定位修改；校验通过也**没有保存**。
4. 点击“**保存配置**”，等待成功提示。有数据库切换、关闭网页或新令牌弹窗时，按弹窗完成。
5. 查看“正在使用”及“待重启”提示。显示待重启时，按 B7 真正重启服务，再回来确认。

| 页面上的状态/操作 | 含义 |
|---|---|
| 草稿 / 待保存 | 只在当前网页内，尚未写入服务器配置 |
| 已保存 | 已写入配置文件；其中一些设置可能仍等待重启 |
| 正在使用 | 当前运行中的服务实际采用的值 |
| 待重启 | 已保存值与启动时的值不同，刷新浏览器没有用 |
| 恢复默认 | 把该字段的默认值放进草稿，仍需保存；不恢复整台服务器 |
| 撤销全部 | 放弃尚未保存的修改及秘密输入；不撤回已经成功保存的操作 |
| 重新载入 | 重新读服务器配置；确认后会丢弃当前草稿 |
| 检查连接 / 检查服务 | 检查候选设置；不保存配置、不迁移数据、不创建玩家绑定 |

切换标签、搜索、看记录、检查更新，草稿会保留。关闭/刷新网页、退出登录会丢失未保存内容；秘密输入不会保存在浏览器持久存储中。保存过程中不要关掉网页；生成的新令牌只在本次成功响应里出现一次。

**遇到“配置已被其他操作更新 / 409”：**

1. 不要先点“重新载入”。
2. 在冲突弹窗点“载入新值并保留我的改动”。
3. 比较服务器新值，确认是否仍要保留自己的改动。
4. 选择“采用此修订号，保留我的草稿”。
5. 再校验、再保存。别人同时改了同一项时，需要你决定最终值。

“更改服主密码”、模型操作、账号授权等是**立即提交的独立操作**，不等“保存配置”；“撤销全部”不会撤回它们。安装更新前会要求处理草稿；备份/导出使用实际运行的数据和设置，不包含尚未保存的草稿。

## A3. 上传、管理与导出模型

### 上传一个或多个模型

1. 点击侧栏“**模型库**”，再点“**上传模型**”。
2. 点击“**选择本机模型文件**”。选的是**浏览器所在电脑**上的文件；要上传服务器磁盘上的文件，应先把文件安全地取到操作电脑，网页不提供服务器磁盘浏览器。
3. 可按 Ctrl/Shift 多选。支持 `.ysm`、`.zip`、`.bbmodel`、`.gltf`、`.glb`。
4. 名称可留空，使用文件名；多文件时填写的名称作为前缀。
5. 选择公开/私有，以及“**归属 Cloud 账号**”。默认公开。私有模型的归属要填真正使用它的 Cloud 账号 ID，不能填第三方服务名字。
6. 点击上传。看每个文件自己的结果；部分成功、部分失败时，可以重试未完成项，不需要把成功项再传一遍。
7. 出现在模型库且显示“已上传”，才算成功；仅选择文件不算上传。

默认单文件上限 128 MiB；批量中的每个文件分别检查。HTTP 413/文件过大还可能来自反向代理的上传限制，见 B4。

### 模型库里的操作

| 要做的事 | 怎样操作 / 会发生什么 |
|---|---|
| 找模型 | 搜索名称、模型 ID、归属账号或格式；翻页只载入当前页 |
| 看详细信息 | 点模型名称，看所有者、格式、大小、版本、可见性与识别结果 |
| 看预览 | 切换网格/3D 卡片；预览不成功不等于原文件一定不能下载。加密 YSM 需要服务器的 YSMParser；GLTF/GLB 不会联网拉取外部资源 |
| 改为私有/公开 | 在详情或批量工具中操作；公开不等于授予编辑权，私有也不是删除 |
| 重新识别 | 按当前识别设置重扫旧内容；开关开启后不会自动替全部旧模型重扫 |
| 人工复核 | 标记 R18、确认通过、标记未识别或移除人工标记。识别以内容哈希关联，相同字节的其他模型也可能受同一人工标记影响 |
| 更改归属 | 输入目标 Cloud 账号并确认；原账号及现有共享授权会移除，转移前核对目标 |
| 删除 / 恢复 | 删除后停止正常下载和使用，之后可恢复；恢复入口需先勾选“包括已删除模型”并选中已删除项 |
| 导出单个模型 | 在详情点“导出原始模型”，下载原始格式和字节 |
| 批量处理 | 勾选当前页需要处理的模型，再使用批量工具；不要把“当前页全选”理解成整个库全选 |

识别状态与拦截策略见 A7。上传成功、识别通过、能够被普通玩家访问，是三件不同的事。

## A4. 玩家登录与绑定

服主把**同一个 Cloud 根地址**发给所有玩家。玩家在兼容版本模组的 Cloud 服务器列表添加该地址，选择实例后注册/登录。允许自助注册时，新账号是普通账号；关闭自助注册时请服主创建/分发账号。

网页账号中心：`https://你的域名/account/`。使用自己的 Cloud 账号登录，打开“**我的账号与绑定**”。

1. 先看“当前 WebUI 账号”“关联的 Cloud 账号”，确认没有登录错账号。
2. 绑定微软正版身份时，使用页面提供的微软登录流程；服主需要先完成 A6 的微软应用设置。
3. 绑定第三方身份时，选择服主已启用的服务，填写**该服务的账号与密码**，点击“验证并绑定第三方账号”。
4. 服务返回多个角色时，按网页提示选择自己的角色；无可用角色应先到该第三方服务建立/检查角色。
5. 成功后刷新绑定列表，确认服务、角色名和 UUID。
6. 若游戏身份已经属于自己的另一个 Cloud 账号，验证可建立账号关联；也可展开“手动关联自己的另一个 Cloud 账号”，用那个账号的凭据证明所有权。

关联不会赋予管理员权限，不会自动转移模型归属，也不能覆盖别人的绑定。验证失败先核对服务、账号、角色和网络，不要用删除数据库的方式解决。

高级“手动验证”输入的是 Minecraft 或第三方服务签发的**访问令牌**，不是服主初始令牌；第三方手动验证还需准确的角色 UUID 与名称。不清楚这些值从哪里来时，用正常登录流程。

## A5. 标签一：基础与网络

位置：**服务器配置 → 基础与网络**。本表的“默认”指程序字段默认值；初始化模板或原实例可能已经填写了不同的值，以“正在使用”为准。

| 网页字段（JSON 路径） | 填什么、怎么选 | 默认 / 生效 |
|---|---|---|
| 实例标识（`instance.instance_id`） | 同一实例长期固定的内部名字。用字母、数字、下划线或短横线，例如 `my-cloud`；不要把域名或中文描述当 ID，已有实例不要随意改 | `self-hosted`；重启 |
| 玩家连接地址（`instance.origin`） | 玩家真正能访问的 HTTPS 根地址，如 `https://cloud.example.com`。不带网页路径、查询参数或账号密码；不要把 `/admin/` 填进去 | `https://localhost` 是本机默认，不适合发给玩家；重启 |
| 监听方式与端口（`instance.bind`） | 同机原生反代通常选本机监听 `127.0.0.1:8787`；Docker 必须让容器网络访问，填 `0.0.0.0:8787`。`0.0.0.0` 表示所有接口，不是给玩家访问的地址 | `127.0.0.1:8787`；重启 |
| 可信反向代理（`instance.trusted_proxy_ips`） | 每行一个**实际直连 Cloud 的代理 IP**，不是玩家 IP、域名或 CIDR。原生同机 Caddy 常为 `127.0.0.1`；本文 Compose 默认为 `172.30.10.3`。只信任你控制的代理 | 空列表；保存后即时 |
| 开放管理网页（`web_ui.enabled`） | 开着才能访问管理网页和管理 API。关闭前复制弹窗恢复命令；账号中心也使用同一网页路由，应一并安排访问影响 | 开；即时 |

可信代理用于接收代理写入的 `X-SPM-Client-IP`，影响真实客户端 IP 识别。不是“把所有访问者加入白名单”。只改公网地址，不会替你配置 DNS、防火墙、证书或代理。

## A6. 标签二：账号与认证

### 注册、服主密码和初始凭据

| 网页字段（JSON 路径） | 用途 / 操作 | 默认 / 生效 |
|---|---|---|
| 允许玩家自行注册（`auth.allow_self_registration`） | 开：玩家可经模组支持的注册入口创建普通账号。关：拒绝新自助注册；不会删除已经存在的账号 | 开；即时 |
| 固定服主账号（`auth.bootstrap_account_id`） | 显示初始服主账号 ID。网页只读；不要以为改名或创建同名游戏角色就能转移服主身份 | `account_local`；网页不可改 |
| 服主初始令牌（`auth.bootstrap_access_token_file`） | 最高管理凭据。保留、替换、生成新令牌或明确清除。替换后旧初始令牌立即失效；新生成值只展示一次 | 字段默认空；初始化会生成引用及随机令牌 |
| 首次初始化密码摘要（`auth.bootstrap_password_hash_file`） | 给**尚未创建密码的初始账号初始化**使用的 Argon2id 摘要文件；不是明文密码输入框，也不能覆盖已有账号密码 | 空；仅首次初始化 |

**日常改密码：** 使用本页的“更改服主密码”，填新的明文密码，点“更新密码并重新登录”。这是账号操作，会撤销旧密码会话。初始令牌是独立凭据，不因改密码自动失效。

**轮换令牌：** 先确保自己知道服主账号密码，再选择“生成新令牌”，保存。新令牌弹出后立即保存到密码管理器，再关闭弹窗。网页不会再次读出这个值；旧令牌登录的会话需要采用新值。不要在唯一可用凭据都未妥善保存时同时清除令牌和修改密码。

### 所有“连接与凭据”控件的共同规则

这套规则也适用于 A8 的 Redis、A9 的 MySQL 和下方微软密钥。

| 选项 | 含义 |
|---|---|
| 保留现有凭据 | 沿用旧引用和旧值；密码框没有显示旧密码是正常的 |
| 替换凭据 | 填入新值/新连接参数；成功保存时，服务端生成一个新的受管秘密文件并更新引用 |
| 清除 | 明确取消这个凭据的引用；可能使依赖它的功能无法工作。需要凭据的当前模式可能拒绝保存 |
| 使用已有凭据文件 | 填**服务器能读取的文件路径**，不上传你电脑上的文件；相对路径以服务器配置文件所在目录为基准 |
| 生成新令牌 | 仅初始令牌有此项；不要自己编造短令牌 |

网页不会读回密码或完整数据库连接 URL。新文件位于配置旁的 `.managed-secrets/`；旧秘密文件不自动覆盖或删除，留作回退。文件引用可以出现在配置 JSON 中，秘密内容不包含在导入/导出 JSON 内。配置、`secrets/` 和 `.managed-secrets/` 必须单独安全备份。

### 添加第三方 Yggdrasil 服务

1. 点击“**添加第三方服务**”。
2. 标准模式只填“服务名称”和“服务入口地址”，例如名称 `我的皮肤站`，地址由该服务官方提供。
3. 点“**检查服务**”，阅读具体结果。此步不保存、不验证玩家账号。
4. 点“**加入配置草稿**”，然后在保存栏校验并保存。
5. 回到“我的账号与绑定”或账号中心，用该服务的真实测试账号完成登录/绑定，确认角色正确。

地址必须是可解析到允许公网地址的 HTTPS 域名；不要填 `http://`、`localhost`、内网 IP、带密码的地址、手工附加的 `:443`，或会跳转到其他地址的网页入口。浏览器能打开某个主页，不代表它就是 Yggdrasil API 入口。

LittleSkin 的服务入口示例为 `https://littleskin.cn/api/yggdrasil`；使用前仍应核对其[官方接入说明](https://github.com/LittleSkinChina/manual/blob/master/advanced/yggdrasil.md)。**该示例不是本项目已用真实账号验证成功的承诺。**

| 模式 / 字段 | 什么时候填 |
|---|---|
| 标准模式 | 服务文档提供一个 Yggdrasil 根入口，默认先选这个 |
| 自定义模式 | 服务明确提供了不同的相对“会话验证路径”和“登录路径”时使用；路径以 `/` 开头，不填完整网址 |
| 网关模式 | 文档给的是完整 hasJoined 验证网址；选网关模式填写完整地址，不再同时填标准基础地址 |
| 内部 ID（`provider_id`） | 新服务自动生成；改展示名称不要换 ID。相同 UUID 来自不同服务仍不是同一个身份 |
| 名称（`display_name`） | 给玩家看的服务名 |
| 入口（`base_url`） | 标准/自定义模式使用的 HTTPS 根入口，可带官方要求的路径前缀 |
| 启用（`enabled`） | 开启后才允许使用该服务认证；停用保留历史绑定 |
| 会话路径（`session_path`） | 缺省 `/sessionserver/session/minecraft/hasJoined`；相对于基础入口 |
| 完整验证地址（`has_joined_url`） | 网关模式；与 `base_url/session_path` 互斥 |
| 登录路径（`authenticate_path`） | 可选相对路径；不填保留兼容推导，通常为 `/authserver/authenticate`，特定网关为 `/auth/authenticate` |

界面中的第三方服务列表对应 `auth.identity_providers`，默认空列表。服务卡片会显示配置来源、启用状态与检查状态；内置 `official` 受保护，不能改接口或停用。

“可连接”仅证明网络/接口探测结果，**不等于账号登录成功**。缺 metadata、某个探测 404、登录接口未识别，都应看具体提示；不要因为一个绿色检查就跳过账号验证。改地址后旧检查结果失效，需重查。

改名保持原 ID 和绑定；停用后停止该服务认证，历史绑定不删除，可从“已停用服务”恢复。从配置移除配置管理的服务也会软停用。数据库遗留服务可“纳入管理”，首次保存不会自动清掉所有遗留项。

### 微软正版网页登录：先有自己的应用

这部分是**可选功能**。只想使用 Cloud 账号管理模型时可以先跳过。

| 网页字段（JSON 路径） | 填什么 |
|---|---|
| 微软登录应用标识（`web_ui.microsoft_client_id`） | Microsoft Entra 应用“应用程序（客户端）ID”，UUID 格式；不是租户 ID，也不是密钥 ID |
| 微软应用密钥（`web_ui.microsoft_client_secret_file`） | 选择替换后填应用客户端密码的**值 Value**；已有文件模式则填服务器文件路径 |
| 微软登录回调地址（`web_ui.microsoft_redirect_uri`） | `https://你的域名/admin/oauth/microsoft/callback`。留空由公网根地址推导；必须与微软登记值一致 |

三个字段默认都为空，修改保存后即时生效。配置 Client ID 不会自动创建微软应用。

1. 进入 Microsoft Entra 的“应用注册”，注册自己的应用。账号类型必须允许**个人 Microsoft 账号**，因为本程序使用个人账号的 `consumers` 流程。见[微软注册文档](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)。
2. 为应用添加 **Web** 平台回调地址，填上表的完整地址，不能只填网站首页。见[回调地址设置](https://learn.microsoft.com/en-us/entra/identity-platform/how-to-add-redirect-uri)。
3. 若应用按机密 Web 客户端配置，在“证书和密码 → 客户端密码”创建客户端密码，复制**值**，记好到期时间；密钥 ID 不能代替值。SPM 当前接口接收客户端密码，不提供证书凭据上传向导。见[微软凭据文档](https://learn.microsoft.com/en-us/entra/identity-platform/how-to-add-credentials)。
4. 在 Cloud 填入对应字段，校验、保存；换过公网地址时，还要同步修改微软登记的回调地址，必要时先重启让新公网地址生效。
5. 用具有可用 Minecraft Java 游戏档案的专用账号走一遍真实登录/绑定。流程会访问微软、Xbox 和 Minecraft 服务；应用登记成功不等于这些服务都已授权可用。

出现回调不匹配、无效客户端、服务拒绝或过期时，检查应用类型、密钥值/有效期、完整回调地址、服务器访问外网能力；从原浏览器重新开始登录，不要复制一次性回调链接给别人使用。

**当前验证边界：** 本项目 2.4.0 已有认证及回传自动测试，但未完成真实微软账号授权全链路；LittleSkin 也未有专用账号实测。服务检查不能替代这一步验收。

## A7. 标签三：模型与内容安全

### 先决定“标记”还是“拦截”

| 设置组合 | 实际结果 |
|---|---|
| 全部关闭（默认） | 不主动做内容识别，也不按这组策略拦截 |
| 只开“识别模型内容” | 尝试生成识别结果，不因识别结果自动阻止访问 |
| 开“阻止成人内容” | 已有自动/人工 R18 结果会用于阻止上传、展示、下载和使用；该开关本身不负责生成识别结果 |
| 再开“识别模型贴图”并配置扫描服务 | 在可解析范围内把模型内嵌贴图交给扫描服务 |
| 开“拒绝未识别模型” | 连未能完成识别的模型也拒绝；检测关闭、超时、加密无法解析或预算不足都可能导致拒绝 |

建议先识别、检查真实结果和误判，再决定拦截。对已上传的模型手动“重新识别”。不要把“未识别”当成“安全”。

| 网页字段（JSON 路径） | 默认 / 允许范围 | 怎么选 / 注意什么 |
|---|---|---|
| 单个模型大小上限（`limits.max_asset_bytes`） | 128 MiB；1 字节～4096 MiB | 网页填 MiB，JSON 用字节；反代也要允许相应大小。调高会增加磁盘/内存压力 |
| 识别模型内容（`moderation.detection_enabled`） | 关 | 开启自动识别；关掉不删除已有结果和人工标记 |
| 阻止成人内容（`moderation.block_r18`） | 关 | 对已识别/人工标记 R18 的内容实施拦截 |
| 拒绝未识别模型（`moderation.block_unclassified`） | 关 | 严格策略；先确认扫描服务、预算及旧模型处理流程可用 |
| 识别模型贴图（`moderation.texture_detection_enabled`） | 关 | 要先开内容识别；开启后必须有扫描地址 |
| 贴图识别服务地址（`moderation.scanner_url`） | 空 | 填完整接口，如 `http://127.0.0.1:8080/infer`；同 Compose 扫描容器用 `http://nudenet:8080/infer`。这里允许内网 HTTP(S)，与第三方认证地址规则不同 |
| 裸露内容识别置信度（`moderation.score_threshold`） | 70%；0～100% | 网页 70 对应 JSON 0.7。阈值更低更容易判 R18，也可能增加误判 |
| 识别等待上限（`moderation.timeout_seconds`） | 30 秒；1～300 秒 | 超时是未识别，不是通过 |
| 单模型贴图预算（`moderation.max_textures`） | 64 张；1～256 张 | 预算不足可能无法完成识别；不要为了省资源再同时强制拒绝所有未识别内容 |
| 单张贴图大小预算（`moderation.max_texture_bytes`） | 8 MiB；1 KiB～32 MiB | 网页 MiB、JSON 字节；限制读取/解码预算 |
| 加密模型解析程序（`moderation.ysm_parser_path`） | 空 | **服务器上** YSMParser 可执行文件路径；需自行安装适合服务器系统的程序，填浏览器电脑上的路径无效 |

这些设置保存后即时用于后续操作。部分扫描字段要同时开启内容识别与贴图识别才能编辑；灰色表示依赖条件未满足，不代表旧值已被清空。点击“检查扫描服务连接”只是连通性检查，仍应上传专用测试模型确认识别结果。

### 扫描服务还没安装怎么办

原生部署可使用发行包的 `nudenet_service.py` 与 `requirements-moderation.txt`；源码目录则在 `tools/` 下。安装和启动命令见 B6。首次安装/下载模型可能较慢，要等待服务真正启动。

Docker 可叠加 `docker-compose.moderation.yml`，但镜像必须提供本程序要求的 `/infer` JSON 接口；**不能只因为容器名叫 NudeNet 就认为接口兼容**。不兼容时使用项目附带适配脚本自行运行/容器化，不要反复放宽拦截策略来掩盖扫描错误。

## A8. 标签四：性能与缓存

### 同时处理几个任务

| 字段（JSON 路径） | 默认 / 范围 | 填法 |
|---|---|---|
| 服务器性能预设（`performance.preset`） | 中配 `medium` | 低配 `low`：上传 2 / 处理 1；中配：4 / 2；高配 `high`：8 / 4；自定义 `custom` 才能编辑下面两项 |
| 同时上传数量（`performance.upload_concurrency`） | 4；0～32 | 自定义模式生效，0 表示该项不限制，不是关闭上传 |
| 扫描与解码并发（`performance.model_processing_concurrency`） | 2；0～16 | 模型扫描、解码和预览共用；0 不限制。内存较少时可先选低配 |

保存后即时调整；降低限制会等待已开始任务结束，不中断它们。提高并发不保证更快，扫描大贴图可能先耗尽内存。不是自定义预设时，以预设解析后的实际并发为准。

### 缓存不是数据库

| 字段（JSON 路径） | 默认 / 范围 | 怎么选 |
|---|---|---|
| 目录缓存方式（`cache.backend`） | 内存 `memory` | 小型单机保留默认即可；`redis` 要有可用 Redis 服务；`none` 关闭目录缓存，每次查询数据库 |
| Redis 连接与凭据（`cache.redis_url_file`） | 空 | 按 A6 的凭据规则替换或使用已有文件；Redis 模式需要有效连接 |
| 缓存有效时间（`cache.ttl_seconds`） | 30 秒；1～3600 秒 | 缓存多久再重新查询；不是账号/模型的删除时间 |
| 最多缓存页数（`cache.max_pages`） | 256 页；1～4096 页 | 控制缓存占用；不是模型库总容量 |
| Redis 等待上限（`cache.timeout_ms`） | 300 毫秒；50～5000 毫秒 | 超时使用有界内存回退；注意单位不是秒 |

**启用 Redis：**

1. 先准备专用 Redis，确认 Cloud 服务器能够连接。
2. 缓存方式选 Redis；凭据选替换。
3. 主机只填域名/IP，不加 `redis://` 或路径；端口常用 6379。
4. 用户名按 Redis 的实际 ACL 设置填写；没有则按部署情况留空。密码填真实密码；数据库索引通常先用 0。只有 Redis 服务实际支持 TLS 才勾选加密连接。
5. 点检查连接。通过后校验草稿并保存，确认正在使用的缓存方式。
6. Redis 离线会回退到有界内存，持久账号/模型数据不会因缓存故障消失。

切换模式保留未编辑的其他模式参数。缓存关闭时 TTL/页数不可编辑；Redis 超时仅 Redis 模式可编辑。缓存不代替数据库，也不绕过当前权限验证。

## A9. 标签五：存储与数据库

**没有明确需求，保留 SQLite。** 它不需要安装数据库服务。改成 MySQL 是部署选择，不是“升级后必须做的一步”。

| 字段（JSON 路径） | 默认 | 填什么 / 何时生效 |
|---|---|---|
| 数据库类型（`storage.backend`） | `sqlite` | SQLite / MySQL；保存后重启 |
| 数据目录（`storage.data_dir`） | `data` | 数据根目录；相对路径以配置目录为基准。重启 |
| SQLite 文件位置（`storage.database_path`） | 空，即数据目录里的 `spm-cloud.db` | 填文件路径，不是文件夹。显式设置后，改数据目录不会自动改这个路径。重启 |
| 模型文件目录（`storage.object_dir`） | 空，即数据目录里的 `objects` | 填目录，不能填数据库文件；Docker 填容器内路径。重启 |
| MySQL 连接与凭据（`storage.mysql_url_file`） | 空 | MySQL 选替换后分项填写；或引用服务器已有 URL 文件。实际连接切换重启生效 |

配置页面会显示**当前实际数据库和模型目录**。只改路径不会搬迁已有文件；填了新目录后出现“模型全没了”，可能只是连到了新空库，不要立即删除原目录。

### 从 SQLite 切到 MySQL，完整顺序

1. 先记录当前 SQLite 文件路径和模型目录；安排维护时间。
2. 在 MySQL 服务端创建**专用数据库**和有建表、读写、事务等所需权限的服务账号。网页不会替你安装 MySQL、创建目标数据库或账号。
3. 网页选 MySQL、凭据选替换。主机填数据库服务器域名/IP，端口通常 3306；库名只填已创建的数据库名，不填文件路径。
4. 输入用户名和密码。特殊字符密码直接填写，网页会组装连接，不需要自己拼 URL。
5. 检查目标 MySQL。失败时按提示核对网络、认证、库名或协议。检查不创建业务表、不导入数据。
6. 下载当前数据备份，确认文件真的已落地、可解压，并另外备份配置和秘密目录。
7. 保存时阅读当前/目标库说明，勾选备份确认，再“确认保存，重启后生效”。
8. 看到“保存为 MySQL、正在使用 SQLite、待重启”是正常的；按 B7 重启。
9. 重启后检查账号、绑定、权限、模型列表和实际下载，再做一次新库备份；保留旧 SQLite 和原模型目录，直到确认迁移正确。

**自动导入的边界：** 只在启动时，从存在、有效且已有账号的 SQLite 源导入**没有账号和模型的 MySQL 目标**。因此原 SQLite 路径必须仍指向原库，模型目录也必须保留。非空 MySQL 不会被覆盖或自动合并。没有自动 MySQL→SQLite 反向迁移。

MySQL 模式的 WebUI 完整备份会输出可恢复的 SQLite 快照。恢复成 SQLite 的步骤见 B8；不要把它误理解成修改下拉框即可来回搬库。当前 MySQL 适配层使用同步连接串行执行 SQL，大规模部署应按自己的负载验证。

## A10. 标签六：客户端功能

只有**服务端和所用模组版本都支持**时，开关才有意义。它们不负责安装/更新模组，也不替玩家创建游戏实体。

| 网页字段（JSON 路径） | 默认 / 范围 | 解释 |
|---|---|---|
| 实体显示动作（`features.entity_motion`） | 关 | 已绑定女仆/假玩家等受支持目标的动作时间线 |
| 玩家显示状态（`features.player_display_state`） | 关 | 受支持客户端的玩家显示状态与变量 |
| 载具模型绑定（`features.vehicle_bindings`） | 关 | 已绑定载具的显示信息 |
| 弹射物快照（`features.projectile_snapshots`） | 关且不可启用 | 当前未实现；即使手改 JSON 为 true 也会校验失败 |
| 单条协议消息上限（`limits.max_message_bytes`） | 64 KiB；1～1024 KiB | 普通协议消息预算，不是模型上传限制 |
| 单次实体查询数量（`limits.max_entity_query_count`） | 64；1～256 | 一次查询最多带多少实体 |
| 显示状态大小上限（`limits.max_visual_state_bytes`） | 8 KiB；1～64 KiB | 不能超过单条协议消息上限 |
| 显示变量数量上限（`limits.max_visual_variables`） | 32；0～128 | 0 表示不允许携带变量，不是不限制 |
| 显示状态持续发布频率（`limits.visual_publish_requests_per_second`） | 20 次/秒；1～1000 | 控制持续请求频率 |
| 显示状态突发预算（`limits.visual_publish_burst`） | 40；1～2000 | 至少等于持续发布频率 |
| 每个发布者的弹射物预算（`limits.max_projectile_snapshots_per_publisher`） | 256；预留 1～4096 | 当前只读、不启用；不能超过世界预算 |
| 每个世界的弹射物预算（`limits.max_projectile_snapshots_per_world`） | 4096；预留 1～65536 | 当前只读、不启用 |
| 弹射物闲置保留时间（`retention.projectile_idle_ttl_seconds`） | 600 秒；预留 30～86400 | 当前只读；不能超过最长保留时间 |
| 弹射物最长保留时间（`retention.projectile_max_lifetime_seconds`） | 86400 秒；预留 60～604800 | 当前只读 |
| 临时显示数据清理间隔（`retention.cleanup_interval_seconds`） | 60 秒；1～3600 | 清理过期临时显示状态，不删除模型；不能超过闲置保留时间 |

可用项保存后即时应用。预留项有默认值和范围，仅为协议/校验兼容，不表示功能已开放。不要把其他设置里的“0 不限制”套用到这里。

## A11. 标签七：日志

| 字段（JSON 路径） | 默认 | 怎么选 |
|---|---|---|
| 日志详细程度与模块过滤（`logging.level`） | `info` | error 只看错误，warn 加警告，info 适合日常；debug/trace 更详细，排障后改回 |
| 日志输出格式（`logging.format`） | `text` | 文本方便人读；`json` 是给日志采集工具读取的逐条结构化记录 |

两项都**重启生效**。模块过滤用于只提高某个 Rust 模块的日志等级，例如一行全局 `info`，另一行 `spm_cloud=debug`；最终过滤表达式对应 `info,spm_cloud=debug`。不知道模块名时只选全局等级。已有复杂合法过滤表达式未编辑时会保留，不要求手工拆开。

“更新与备份 → 导出运行日志”仅导出**当前进程最近 2 MiB**。重启前的日志要去 Docker/systemd/你自己的进程日志里找；导出文件不是整台服务器全部历史日志。

## A12. 标签八：更新与备份

此页仅服主可用。管理员要导出操作记录请去 A13。

| 配置 / 操作 | 意思 |
|---|---|
| 自动检查并安装更新（`updates.auto_update`） | 默认关。原生部署在支持的环境中检查正式版并尝试安装；容器依部署平台更新，网页不会把镜像替换成新版本 |
| 更新检查间隔（`updates.check_interval_hours`） | 默认 24 小时，范围 1～168 小时；自动更新开启后可编辑，保存即时采用 |
| 检查更新 | 只查版本，不安装；有新版本也不会因点击检查就重启 |
| 安装更新 | 会处理草稿、等待在途任务、校验包和版本、备份并安排重启；需要程序安装目录可写。容器环境不提供原生安装操作 |
| 完整备份 | 数据库一致性快照 + 模型对象 ZIP；保存好下载文件 |
| 模型库导出 | 原格式模型及清单，不是完整账号/权限数据库备份 |
| 操作记录导出 | 全部或按查询条件导出；也可在操作记录页筛选 |
| 运行日志导出 | 当前进程最近 2 MiB，不含进程重启前的历史日志 |

**“下载备份”操作步骤：** 点击按钮 → 有草稿时确认本次使用运行中配置 → 等待浏览器完成下载 → 在下载目录找到 ZIP → 尝试打开/解压并确认有 `manifest.json`、`spm-cloud.db` 和存在时的对象文件 → 另存配置、秘密目录和部署文件。

备份**不包含** `config.json`、`secrets/`、`.managed-secrets/`、TLS 证书或 Compose 环境配置。只保留 ZIP 可能无法找回登录凭据。恢复不是网页上传 ZIP，按 B8 在停服状态执行。

## A13. 标签九：操作记录

服主和管理员都可查看；保留原有权限，不因为合并标签就把管理员提升成服主。

1. 填搜索条件并搜索，或清空条件后重新搜索查看全部。
2. 用分页按钮、每页数量和页码跳转查找。
3. 导出搜索结果时使用同一条件；不要把导出结果数量误认为只限当前页。
4. 记录可帮助核对谁做了配置、账号或模型变更，但不代替数据备份，也不是网页上的“撤回”按钮。

## A14. 查找设置、导入导出与关闭网页

**查找设置：** 在顶部搜中文名称、说明或技术名，如“缓存”、`Redis`、`max_asset_bytes`，点击结果定位。横向标签栏可滚动，支持方向键、Home/End；窄屏保存栏固定在底部。

**导入/导出配置：** 展开独立的“导入/导出配置”工具。导出的是配置草稿，含文件引用、不含秘密内容；不是全数据备份。导入会修改同一草稿，仍需校验和保存；把某字段省略可能恢复其默认值，因此不要把一小段 JSON 当完整配置导入。不确定时用正常表单。

**关闭网页：** 在 A5 关闭开关，保存前复制实际恢复命令并核对配置路径。关闭后管理网页与相关管理 API 返回不可用。到服务器执行：

~~~sh
spm-cloud --config /实际配置路径/config.json --enable-web-ui
~~~

Windows 需使用实际 exe 路径，写法见 B7。命令只把网页开关改回开启，不重置账号、不创建新数据库。运行中的文件监听会采用修改；没有运行时下次启动采用。

---

# B. 命令行：安装、文件配置和运维

## B0. 怎样照着命令操作

- Windows 示例使用 **PowerShell**；Linux/macOS 示例使用 **Bash/终端**。不要把 Bash 的 `sudo`、`\` 换行符原样粘进 PowerShell。
- 命令前的“当前目录”很重要。`./spm-cloud` 表示当前目录里的文件；`.\spm-cloud.exe` 是 PowerShell 的写法。
- 带空格的路径要加引号。PowerShell 调用带引号的 exe 路径，前面加 `&`。
- 文中要求替换的域名/路径/账号必须替换；代码块中的 `<配置路径>` 是占位说明，不能连尖括号一起运行。
- 同一段里设置的 `$变量` 需要在**同一个终端窗口**继续执行。新开窗口要重新设置。
- 不要把一整篇手册全部粘贴执行。只选自己的部署方式，并在每一步核对结果。

正式部署前准备：固定域名、可访问的服务器、数据保存位置；公网 HTTPS 通常需要正确 DNS 和可到达的 TCP 80/443。若已有网站占用端口，复用现有反代配置，不能再启动另一个抢占相同端口的 Caddy。

**怎样打开并保存配置：** Windows 可按 Win+R，输入 `notepad C:\SPMCloud\config.json` 后回车；确认打开的是已经初始化的文件，修改后按 Ctrl+S。Linux 示例可用 `sudo nano /etc/spm-cloud/config.json`，Docker 示例则是 `sudo nano /opt/spm-cloud/deploy/runtime/config.json`；nano 中按 Ctrl+O、回车保存，再按 Ctrl+X 退出。没有 nano 时先用系统包管理器安装，或使用你熟悉的编辑器。不要用 Word/WPS 把配置另存成富文本文档；文件仍须叫 `config.json`，不能变成 `config.json.txt`。

**域名记录在哪里填：** 到域名的 DNS 管理页面，为例如 `cloud.example.com` 建 A 记录指向服务器公网 IPv4；只有服务器真的有可用公网 IPv6 时才加 AAAA。域名正确还不够，云服务器安全组、系统防火墙和家庭路由端口转发也可能分别阻挡访问。不要把 `127.0.0.1`、`0.0.0.0` 或 Docker 内网 IP 填成公网 DNS 记录。

## B1. Windows 安装和第一次启动

### 方法一：下载 ZIP、解压，适合第一次使用

1. 打开 [Releases](https://github.com/sdf123098/spm-cloud/releases/latest)，在附件中下载 `spm-cloud-x86_64-pc-windows-msvc.zip`。不要下载 Docker 镜像，也不要把“Source code”当成可直接双击运行的程序。
   同时下载同名的 `.zip.sha256` 校验文件。在 ZIP 所在目录打开 PowerShell，执行 `Get-FileHash -LiteralPath .\spm-cloud-x86_64-pc-windows-msvc.zip -Algorithm SHA256`；用记事本打开 `.sha256`，确认其中第一段 64 位十六进制值与命令输出的 Hash 完全相同（大小写不影响）。不同时重新下载，先不要运行。
2. 解压到 `C:\SPMCloud`；确认 `C:\SPMCloud\spm-cloud.exe` 确实存在。不要在 ZIP 预览窗口里运行。
3. 在这个文件夹打开 PowerShell，执行：

~~~powershell
Set-Location C:\SPMCloud
.\spm-cloud.exe --version
.\spm-cloud.exe --init-config C:\SPMCloud\config.json
~~~

应看到版本和“配置已初始化”提示。将生成 `config.json`、`secrets\bootstrap-token.txt`。**已有配置不要重复初始化；该命令不是恢复密码。**

4. 如果只在同一台电脑试用，先保留默认监听和本机地址。检查后启动：

~~~powershell
.\spm-cloud.exe --config C:\SPMCloud\config.json --check-config
.\spm-cloud.exe --config C:\SPMCloud\config.json
~~~

看到监听日志后让终端保持运行，打开 `http://127.0.0.1:8787/admin/`，按 A1 登录。需要另一条命令时另开一个 PowerShell。

5. 正式供玩家使用前，用文本编辑器打开 `C:\SPMCloud\config.json`，设置真实 HTTPS 根地址/固定实例 ID，按 B4 配置 HTTPS 反代并重启。填写地址本身不会让公网自动连通。
6. 前台试用停止：回到运行窗口按 Ctrl+C。关掉窗口也可能停止服务。需要开机自动运行时，应自行配置 Windows 服务管理器或计划任务，明确 exe、`--config`、工作目录和运行账号；程序不会自动注册 Windows 服务。

ZIP 中的 `start-cloud.ps1` 是可选启动脚本；放在 exe 同目录，缺配置时会初始化，随后检查并启动：

~~~powershell
.\start-cloud.ps1 -ConfigPath C:\SPMCloud\config.json
~~~

如果脚本被执行策略阻止，先使用上面的 exe 命令即可，不需要为了启动程序修改全局执行策略。

### 方法二：官方安装器

安装器安装程序，不会替你配置域名、创建系统服务或完成数据初始化。需要时可先下载检查脚本，再在自己的 PowerShell 中运行。安装器入口：

`https://github.com/sdf123098/spm-cloud/releases/latest/download/spm-cloud-installer.ps1`

常用安装目录为 `$env:USERPROFILE\.cargo\bin\spm-cloud.exe`；可通过安装器支持的 `SPM_CLOUD_INSTALL_DIR` 改变前缀。若不确定安装位置，优先用方法一，后文统一使用实际 exe 路径。

## B2. Docker Compose 部署

本节用于 **Linux 服务器 + Docker Engine + Compose 插件**。Windows Docker Desktop、NAS 和面板也能部署容器，但路径、权限、域名接入方式需按其平台调整；不要把 Linux 命令直接贴进 Windows PowerShell。

先确认：

~~~bash
docker version
docker compose version
~~~

没有相应命令时先按 [Docker 官方安装文档](https://docs.docker.com/engine/install/) 安装。下面示例从源码构建本地镜像；预编译 Docker 镜像加载方法见 B7。

### 第一步：准备目录和镜像

~~~bash
sudo git clone https://github.com/sdf123098/spm-cloud.git /opt/spm-cloud
cd /opt/spm-cloud
sudo docker compose -f docker-compose.json.yml build spm-cloud
sudo install -d -o 10001 -g 10001 -m 700 deploy/runtime
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --init-config /etc/spm-cloud/config.json
~~~

构建需要网络和时间，出现完成提示再继续。`run --rm` 是一次性初始化容器，执行完会移除容器，挂载目录中的配置/凭据仍保留。已有 `deploy/runtime/config.json` 的部署跳过初始化。

### 第二步：编辑真正会被读取的配置

用编辑器打开主机上的 `/opt/spm-cloud/deploy/runtime/config.json`，逐项调整，其他生成内容保留：

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

这是**新实例的完整最小示例**；没有列出的字段使用默认值。不要用它覆盖已有实例的服务列表、密钥引用或其他配置。

| 主机上 / Docker 中的东西 | 实际对应 |
|---|---|
| `./deploy/runtime` | 容器 `/etc/spm-cloud`；保存配置、`secrets/` 和 `.managed-secrets/` |
| 命名卷 `spm-cloud-data` | 容器 `/var/lib/spm-cloud/data`；数据库 |
| 命名卷 `spm-cloud-objects` | 容器 `/var/lib/spm-cloud/objects`；模型文件 |
| `./Caddyfile.compose` | HTTPS 站点与上游配置 |
| `caddy-data` / `caddy-config` | Caddy 的证书/状态卷；不是 Cloud 的模型目录 |

**不要把容器里的 bind 写成 127.0.0.1。** Caddy 是另一个容器，无法通过 Cloud 容器自己的 loopback 访问它。也不要在容器配置里填主机的 `/opt/spm-cloud/...`，除非你明确挂载了该路径。

### 第三步：域名和 HTTPS

打开 `/opt/spm-cloud/Caddyfile.compose`，把第一行示例域名改成与 `instance.origin` 相同的域名，保留 `reverse_proxy spm-cloud:8787` 等其余内容。

为域名设置正确的 A/AAAA 记录；只有 IPv6 实际可达时才保留 AAAA。允许互联网到达该服务器的 TCP 80/443。默认 Compose 不把 Cloud 的 8787 映射到主机公网，只让代理经容器网络访问。

默认代理 IP 为 `172.30.10.3`、子网为 `172.30.10.0/24`。与现有网络冲突时，要同时调整 Compose 的子网/代理 IP 和 JSON 的可信代理 IP，不能只改一处。

### 第四步：检查、启动、登录

~~~bash
cd /opt/spm-cloud
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo docker compose -f docker-compose.json.yml up -d
sudo docker compose -f docker-compose.json.yml ps
sudo docker compose -f docker-compose.json.yml logs --tail 100 spm-cloud caddy
~~~

成功标准：服务运行、没有配置/权限错误，浏览器可打开 `https://你的域名/admin/`。读取令牌：

~~~bash
sudo cat /opt/spm-cloud/deploy/runtime/secrets/bootstrap-token.txt
~~~

再按 A1 设置服主密码。Caddy 的自动证书依赖域名和网络满足条件，见 [Caddy HTTPS 文档](https://caddyserver.com/docs/automatic-https)。

日常管理保持同一 Compose 文件和同一卷名；**不要执行 `docker compose down -v` 来升级/修复**，`-v` 会移除相关数据卷。也不要在不同目录意外创建另一套项目或更换持久卷名。

## B3. Linux 原生服务

本节示例为使用 systemd 的 Linux。macOS 手动运行见本节末尾。

### 下载与第一次验证

1. 到 Release 选 Linux x64 的 `spm-cloud-x86_64-unknown-linux-gnu.tar.xz` 或 ARM64 的 `spm-cloud-aarch64-unknown-linux-gnu.tar.xz`，以及同名 `.sha256`。
2. 用 `uname -m` 查架构：`x86_64` 选 x64；`aarch64/arm64` 选 ARM64。此 Linux 包为 GNU 目标，不是任意 Alpine/musl 系统通用包。
3. 将下载文件放进同一工作目录，校验并解压。例如 x64：

~~~bash
sha256sum -c spm-cloud-x86_64-unknown-linux-gnu.tar.xz.sha256
mkdir -p unpacked
tar -xJf spm-cloud-x86_64-unknown-linux-gnu.tar.xz -C unpacked
~~~

校验必须显示 OK。到解压出的程序目录，找到 `spm-cloud` 后继续；不同解压工具可能多包一层目录。

~~~bash
./spm-cloud --version
sudo install -m 755 ./spm-cloud /usr/local/bin/spm-cloud
sudo useradd --system --home /var/lib/spm-cloud --create-home --shell /usr/sbin/nologin spm-cloud
sudo install -d -o spm-cloud -g spm-cloud -m 750 /etc/spm-cloud /var/lib/spm-cloud/data /var/lib/spm-cloud/objects
sudo -u spm-cloud /usr/local/bin/spm-cloud --init-config /etc/spm-cloud/config.json
~~~

服务账号已存在时跳过 `useradd`，不要改用另一个账号制造权限混乱。

编辑 `/etc/spm-cloud/config.json`，设置实际域名、固定 ID、本机监听；存储路径明确填：

~~~json
{
  "data_dir": "/var/lib/spm-cloud/data",
  "database_path": "/var/lib/spm-cloud/data/spm-cloud.db",
  "object_dir": "/var/lib/spm-cloud/objects",
  "backend": "sqlite",
  "mysql_url_file": null
}
~~~

上面是替换 **storage 对象的内容**，不是整个配置文件。保留初始化生成的令牌引用和其余配置。

### 安装 systemd 模板

`deploy/spm-cloud-json.service` 来自**源码仓库**，不保证在原生二进制归档里。先取源码/部署模板，示例：

~~~bash
sudo git clone https://github.com/sdf123098/spm-cloud.git /opt/spm-cloud
sudo cp /opt/spm-cloud/deploy/spm-cloud-json.service /etc/systemd/system/spm-cloud-json.service
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --check-config
sudo systemctl daemon-reload
sudo systemctl enable --now spm-cloud-json.service
sudo systemctl status spm-cloud-json.service
~~~

已有 `/opt/spm-cloud` 时使用已有工作树，不要再次 clone 覆盖。模板使用服务账号 `spm-cloud`、程序 `/usr/local/bin/spm-cloud`、配置 `/etc/spm-cloud/config.json`。路径不同时先修改 unit，**启动命令里的配置路径必须与编辑的文件一致**。

看日志和重启：

~~~bash
sudo journalctl -u spm-cloud-json.service -n 100 --no-pager
sudo systemctl restart spm-cloud-json.service
~~~

模板限制可写目录为 `/var/lib/spm-cloud` 与 `/etc/spm-cloud`。需要存放到其他位置时，既要设置文件权限，也要相应调整 unit 的 `ReadWritePaths`。完成 B4 的代理后再对公网使用。

### macOS 手动运行

Apple Silicon 选 `aarch64-apple-darwin`，Intel Mac 选 `x86_64-apple-darwin` 的 tar.xz；解压，在终端进入程序目录，运行 `./spm-cloud --version`，然后按 B5 初始化 JSON 并以 `--config` 启动。macOS 不使用上面的 systemd 命令；长期运行自行配置 launchd/进程管理及 HTTPS 代理。

## B4. HTTPS、代理、端口与外网访问

一个能用的公网部署需要这条路径全部通：

**玩家/浏览器 → 你的 HTTPS 域名:443 → Caddy/Nginx → Cloud 的 HTTP 监听端口**

`instance.origin` 是告诉客户端“我的公网地址是什么”；`instance.bind` 才决定程序监听哪里。它们不会替你完成域名解析、端口转发或 TLS。

### 同机原生 Caddy 示例

先安装并运行 Caddy。参考仓库 `deploy/Caddyfile.native`，站点配置：

~~~caddyfile
cloud.example.com {
    reverse_proxy 127.0.0.1:8787 {
        header_up X-SPM-Client-IP {http.request.remote.host}
    }
}
~~~

Cloud 的配置对应 `origin=https://cloud.example.com`、`bind=127.0.0.1:8787`、可信代理 `["127.0.0.1"]`。这段配置应交给你实际运行的 Caddy 服务，不是贴进 config.json。代理与 Cloud 不在同一台机器/网络空间时，重新核对上游地址和网络边界，不能继续照填本机地址。

### Nginx、上传限制和 WebSocket

仓库 `nginx-cloud.conf.example` 是 HTTPS server 块内的**片段**，不是完整含证书的 nginx.conf。它保留 HTTP/1.1、Upgrade/Connection 与真实 IP 头。需要同时转发网页、`/v1` 和 `/v1/realtime`，不能只代理管理网页。

默认模型限制 128 MiB，对应片段 `client_max_body_size 128m;`。提高 Cloud 上限后也要调整反代；否则请求在到达 Cloud 前就被拒绝。

反代限制的是整个请求，上传表单还有少量额外开销；接近 128 MiB 的模型仍可能碰到 128m 请求上限。要允许完整 128 MiB 的模型，可给代理留余量，例如设为 `client_max_body_size 136m;`，Cloud 自身仍保留 128 MiB 单文件限制。修改 Nginx 后先检查配置，再通过实际服务的方式重新加载。

### 按由近到远检查

1. 在 Cloud 所在机器查 `http://127.0.0.1:8787/health`（仅适用于本机监听）。Docker 未发布主机 8787 时，不要用主机访问失败来判断容器已坏。
2. 查看 Cloud 日志，确认配置加载和监听成功。
3. 查看代理日志，确认能连上上游并获得证书。
4. 在外部网络访问实际 HTTPS 域名的 `/health` 和 `/admin/`。
5. 最后让模组添加根地址并验证登录/目录。

## B5. 配置文件、路径和秘密文件

### CLI 参数只有这些用途

以下使用 `spm-cloud` 代表已经能从 PATH 运行的程序；Windows 未加 PATH 时，用 `& "C:\SPMCloud\spm-cloud.exe"` 替代。

| 命令 | 会做什么 | 不会做什么 |
|---|---|---|
| `spm-cloud --version` | 显示程序版本 | 不启动服务 |
| `spm-cloud --help` | 显示参数帮助 | 不修改配置 |
| `spm-cloud --init-config /路径/config.json` | 为新实例生成配置和随机初始令牌 | 不覆盖现有配置/凭据，不是改密 |
| `spm-cloud --config /路径/config.json --check-config` | 读取、校验选定配置和所引用凭据 | 不监听、不创建数据库或迁移；也不证明外部服务能连接 |
| `spm-cloud --config /路径/config.json --print-effective-config` | 输出脱敏有效值、路径和来源 | 不启动服务；输出仍可能暴露内部路径/服务地址，不宜无检查地公开 |
| `spm-cloud --config /路径/config.json` | 按该文件启动服务 | 不是后台注册服务命令；前台会持续运行 |
| `spm-cloud --config /路径/config.json --enable-web-ui` | 将选定有效配置的网页开关设为 true | 不重置账号、创建库或迁移；不能修复其他无效配置 |

`--check-config` 与 `--print-effective-config` 二选一。初始化、帮助、版本和恢复操作不要随意与其他模式混用。没有“重置所有账号”“自动修复数据库”“设置服主密码”的额外 CLI 开关。

### 哪个文件才是当前配置

选择顺序：

1. 显式 `--config PATH`。
2. 环境变量 `SPM_CLOUD_CONFIG` 指定的文件。
3. 进程当前工作目录里的 `config.json`。
4. 都没有时才使用旧 ENV 模式。

选中了缺失/无效文件会报错，**不会偷偷回退到环境变量**。使用 JSON 时，业务环境变量和 `RUST_LOG` 不覆盖该文件；保留未写字段的程序默认值。部署识别等环境变量（如容器标识）不等于业务字段覆盖。

新部署建议始终显式 `--config`，避免从另一个目录启动时读取错文件。

### 相对路径是相对谁

假设配置为 `C:\SPMCloud\config.json`：

| JSON 中的值 | 真正指向 |
|---|---|
| `"data_dir": "data"` | `C:\SPMCloud\data` |
| `"database_path": null` | 数据目录里的 `spm-cloud.db` |
| `"object_dir": null` | 数据目录里的 `objects` |
| `"bootstrap_access_token_file": "secrets/bootstrap-token.txt"` | `C:\SPMCloud\secrets\bootstrap-token.txt` |
| `"database_path": "data/spm-cloud.db"` | `C:\SPMCloud\data\spm-cloud.db`，不会再额外拼一次 data |
| `"object_dir": "D:/CloudObjects"` | 指定绝对目录，不受配置目录变化影响 |

JSON 的 Windows 路径可用 `"C:/SPMCloud/data"`，或把反斜线转义成 `"C:\\SPMCloud\\data"`。不要直接写未转义的 `\`。Docker 用容器内路径；浏览器本机路径与服务器路径不是一回事。

### 读取和备份初始令牌

Windows：

~~~powershell
Get-Content -LiteralPath C:\SPMCloud\secrets\bootstrap-token.txt
~~~

本文 Linux 原生配置：

~~~bash
sudo -u spm-cloud cat /etc/spm-cloud/secrets/bootstrap-token.txt
~~~

本文 Docker：

~~~bash
sudo cat /opt/spm-cloud/deploy/runtime/secrets/bootstrap-token.txt
~~~

轮换后可能已改为 `.managed-secrets/` 中的新文件；此时查看当前配置的 `auth.bootstrap_access_token_file` 引用，而不是一直读取旧 bootstrap-token.txt。

### 手改 JSON 的步骤

1. 备份当前 `config.json` 及秘密文件。
2. 使用文本编辑器打开文件，按 A5～A12 表中的 JSON 路径找到字段；例如“缓存方式”就是 `cache` 对象里的 `backend`。
3. 只改需要的字段；保存为 UTF-8 JSON。键/字符串用双引号，无注释、无尾逗号；布尔值写 `true/false`，不要写 `"true"`。
4. 执行 `--check-config`，通过后再启动/重启。运行中的程序每两秒检查有效文件变化；无效更新保留上一次有效运行设置，不能因为网页还活着就认为坏文件已被采用。
5. 看待重启状态。网络身份、存储、日志仍需真正重启。

`schema_version` 保留整数 `1`，它表示配置格式，不是程序版本号，不能改成 `2.4.0`。`null` 表示该字段没有显式值，和 `""` 空字符串、`0`、`false` 不一样；是否允许留空以对应字段说明为准。配置键名区分大小写。

**单位区别：** 网页 128 MiB = JSON `134217728` 字节；网页 64 KiB = JSON `65536`；网页 70% = JSON `0.7`。秒、毫秒、小时按字段名和 A 部分表格，不能混用。只有明确说明的并发字段 0 才代表“不限制”。

### 在命令行准备凭据文件

网页可自动生成受管文件；纯文件配置时，`*_file` 字段填的是**文件路径**，文件内容才是秘密。各文件只放一行正确值，不写 JSON、引号说明或注释：

| 配置引用 | 文件内容 |
|---|---|
| `auth.bootstrap_access_token_file` | 管理初始令牌 |
| `auth.bootstrap_password_hash_file` | 完整 Argon2id 密码摘要；不是明文密码 |
| `storage.mysql_url_file` | 完整 MySQL URL，例如格式 `mysql://USER:ENCODED_PASSWORD@HOST:3306/DATABASE` |
| `cache.redis_url_file` | `redis://.../0`，支持 TLS 时用 `rediss://.../0` |
| `web_ui.microsoft_client_secret_file` | 微软客户端密码的值 |

MySQL/Redis URL 中用户名和密码的 `@`、`:`、`/`、`#` 等保留字符需要正确百分号编码；不要对整个 URL 一次性编码。不熟悉编码时用网页结构化连接表单。

在 POSIX 上限制秘密目录为服务账号可访问、文件常用 600；Windows 为运行 Cloud 的账号授予必需权限，避免给所有用户读取权。服务账号还要能在配置目录创建临时文件和 `.managed-secrets/`；仅 config.json 文件可写但目录不可写，原子保存也会失败。

A 部分各字段表覆盖全部 56 个业务字段和服务子项；[config.schema.json](config.schema.json) 是精确类型/边界参考，[config.example.json](config.example.json) 是示例，不要用示例覆盖生产文件。

## B6. 安装扫描服务、外置 MySQL 和 Redis

### 本机 NudeNet 适配服务

这些 Python 脚本需要另行安装 Python，Cloud 原生程序本身不需要它。Windows 先在 PowerShell 确认 `py --version` 可用；Linux 确认 `python3 --version`，并安装 pip/venv 支持（Debian/Ubuntu 对应 `python3-pip`、`python3-venv` 包）。如果提示找不到命令，先完成 Python 安装，不要把后面的报错当成 Cloud 配置损坏。B8 的备份脚本也使用同一套 Python。

在**运行 Cloud 的服务器**上执行。源码根目录下：

~~~bash
python3 -m venv .venv-moderation
.venv-moderation/bin/python -m pip install -r tools/requirements-moderation.txt
.venv-moderation/bin/python tools/nudenet_service.py --bind 127.0.0.1 --port 8080
~~~

Windows PowerShell，使用源码目录：

~~~powershell
py -m venv .venv-moderation
.\.venv-moderation\Scripts\python.exe -m pip install -r tools/requirements-moderation.txt
.\.venv-moderation\Scripts\python.exe tools\nudenet_service.py --bind 127.0.0.1 --port 8080
~~~

如果使用**解压的发行包**，把 `tools/requirements-moderation.txt`、`tools/nudenet_service.py` 换成包根目录的 `requirements-moderation.txt`、`nudenet_service.py`；不要凭空新建一个 tools 目录。

另开终端检查 `http://127.0.0.1:8080/health`，再在 Cloud 填 `http://127.0.0.1:8080/infer`。原生 Cloud 与扫描器在同一主机时可这样用。Cloud 在容器、扫描器在主机时，容器中的 127.0.0.1 仍指容器自己；需配置实际可达的受控地址。

不要把扫描服务开放到公网当成模型文件服务器；该适配器没有面向公网的登录流程。长期运行需另外安排进程管理和适合机器的资源限制。

### 外部数据库与缓存

Cloud 不会安装 MySQL/Redis，也不会替你创建 MySQL 的数据库和账号。应先完成这些服务自身的部署、网络和权限，再使用 A8/A9 的连接检查。

SQLite 是持久数据库，Redis 是可丢弃缓存：重启/清空 Redis 不应被当成账号数据库恢复方法。外部 MySQL 的账号/权限/表与 Cloud 模型对象都要备份，不能只备份 Redis。

MySQL URL 兼容模式可使用底层驱动支持的连接参数；网页的“TLS”勾选针对 Redis，**不是一个通用 MySQL TLS 配置按钮**。远端数据库的加密、证书与网络隔离按数据库部署设置，别以为 HTTPS 网站会自动把数据库链路也变成 HTTPS。

## B7. 停止、重启、更新与网页恢复

### 先认清谁在启动程序

| 部署方式 | 停止 / 重启 |
|---|---|
| 前台 Windows/Linux/macOS | 回原终端 Ctrl+C；再执行原 `--config` 命令 |
| 本文 Linux systemd | `sudo systemctl stop spm-cloud-json.service` / `sudo systemctl restart spm-cloud-json.service` |
| 本文 Compose | 在原部署目录执行 `sudo docker compose -f docker-compose.json.yml stop spm-cloud` / `sudo docker compose -f docker-compose.json.yml restart spm-cloud` |
| 自己配置的服务/任务 | 用实际服务名/任务名；不要照抄本文 systemd 名称假设它存在 |

停止服务会影响玩家连接，先安排维护时间。不要同时从终端和后台服务启动两个实例争用同一端口/数据库。

### 关闭网页后的恢复命令

Windows：

~~~powershell
& "C:\SPMCloud\spm-cloud.exe" --config "C:\SPMCloud\config.json" --enable-web-ui
~~~

Linux 原生：

~~~bash
sudo -u spm-cloud /usr/local/bin/spm-cloud --config /etc/spm-cloud/config.json --enable-web-ui
~~~

Docker，从原 Compose 目录：

~~~bash
sudo docker compose -f docker-compose.json.yml run --rm --no-deps spm-cloud --config /etc/spm-cloud/config.json --enable-web-ui
~~~

原运行进程仍在时，等待文件监听采用修改，再刷新原网址；没有进程时先正常启动。配置本身错误、路径错或不可写时，该命令会失败，需要先修复原因。

### 手动更新原生程序

1. 记录旧版本、实际 exe/程序路径、启动命令和配置路径。
2. 做 B8 的备份，并保留旧程序。
3. 从 Release 下载**正确平台**的包及 `.sha256`，核对 SHA-256。
4. 停止原服务。Windows 正在运行的 exe 通常不能直接覆盖。
5. 替换程序，不覆盖现有配置、秘密和数据；不要再次初始化。保留新包的 schema 和辅助脚本供对应版本使用。
6. 先执行新程序 `--version` 和 `--config ... --check-config`，再用原启动方式启动。
7. 检查健康状态、网页登录、权限、模型下载；确认后再恢复更新计划。

回滚旧程序不等于回滚数据库。新版本若已迁移数据库且旧程序不兼容，需要一并恢复升级前的数据库/模型备份，不能只换回 exe。

### 使用发布的 Docker 镜像归档

Release 提供 `spm-cloud-docker-linux-amd64.tar.gz`、`spm-cloud-docker-linux-arm64.tar.gz` 及校验文件。**这是 docker load 用的镜像归档，不是解压后运行的程序包。**

以下载到当前目录的 amd64 包为例：

~~~bash
sha256sum -c spm-cloud-docker-linux-amd64.tar.gz.sha256
sudo docker load -i spm-cloud-docker-linux-amd64.tar.gz
~~~

查看加载提示中的标签，例如 `spm-cloud:v2.4.0`。在原 Compose 的 `.env` 设置 `SPM_CLOUD_IMAGE=spm-cloud:v2.4.0`（将版本改成实际加载值，保留文件其他已有内容），然后：

~~~bash
sudo docker compose -f docker-compose.json.yml up -d --no-build spm-cloud
sudo docker compose -f docker-compose.json.yml logs --tail 100 spm-cloud
~~~

`restart` 只重启旧容器，不会采用新镜像；需要 `up -d` 重新创建。不要换卷名、删除卷或重新初始化。Compose 的 .env 在这里用于替换镜像/挂载等部署参数，不是用它覆盖 JSON 业务设置。

### 可选自动更新：选一种维护方式

网页原生更新、外部 systemd 定时更新器、手工更新，是三种可能入口；不要让多个更新器同时争用同一程序。

- **网页原生更新：** A12。需要程序所在目录可写，Unix 需 tar/xz；本文加固 systemd 服务账号通常不能写 `/usr/local/bin`，此时用受控外部更新器或手动更新，不要直接把安装目录改成所有人可写。
- **Linux 原生定时器：** 源码 `deploy/native-auto-update.sh`、`spm-cloud-native-update.service`、`spm-cloud-native-update.timer`。依赖 curl、jq、tar/xz、sha256sum、flock、systemd。默认假定服务 `spm-cloud.service`，本文却安装的是 `spm-cloud-json.service`，**必须显式改对服务名**。
- **Compose 定时器：** `deploy/spm-cloud-json-auto-update.service`、`spm-cloud-json-auto-update.timer` 配合 `deploy/docker-auto-update.sh`；默认仓库 `/opt/spm-cloud`、Compose 文件 `docker-compose.json.yml`，只支持其脚本明确支持的 Linux amd64/arm64。
- **Windows 外部脚本：** 源码 `deploy/windows-auto-update.ps1` 针对已有计划任务工作，不会替你注册运行 Cloud 的任务；按其 `ApplicationTaskName`、`ExecutablePath`、`HealthUrl` 参数对照实际环境。没有对应计划任务就不要照跑。

Linux 原生定时器安装示例（先确认路径/服务名）：

~~~bash
sudo install -d -m 755 /usr/local/libexec
sudo install -m 755 /opt/spm-cloud/deploy/native-auto-update.sh /usr/local/libexec/spm-cloud-native-update.sh
sudo cp /opt/spm-cloud/deploy/spm-cloud-native-update.service /opt/spm-cloud/deploy/spm-cloud-native-update.timer /etc/systemd/system/
sudo systemctl edit spm-cloud-native-update.service
~~~

在编辑器中填入并保存：

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

先 `sudo systemctl daemon-reload`，再 `sudo systemctl start spm-cloud-native-update.service` 并用 `sudo journalctl -u spm-cloud-native-update.service -n 100 --no-pager` 检查；确认正确后 `sudo systemctl enable --now spm-cloud-native-update.timer`。停用定时检查用 `sudo systemctl disable --now spm-cloud-native-update.timer`，不需要停 Cloud 本身。

Compose 模板可分别复制为 `/etc/systemd/system/spm-cloud-auto-update.service` 与 `spm-cloud-auto-update.timer`；路径不同要先改 unit 中的目录/脚本。先手动执行服务检查日志，再启用 timer。更新器会应用最新正式版，不是固定停留在 2.4.0；备份与回滚策略应先准备好。

## B8. 备份与恢复：一步都不要少

### 一份能恢复的备份包含什么

| 要保留 | 为什么 |
|---|---|
| 数据库 + 模型对象的一致性备份 | 账号、权限、绑定、版本和模型文件需要对应 |
| 当前 config.json | 记录使用哪个库、哪个目录、哪些认证服务 |
| secrets 和 .managed-secrets | 保留初始令牌、数据库/Redis/微软凭据；网页完整备份不会打包它们 |
| 部署配置、代理配置、证书/卷及旧程序版本 | 能重新启动原环境；程序回滚可能需要配套数据 |

先复制到另一处安全位置，再确认下载完成和备份可读取；把文件移到同一块即将更换/删除的磁盘上不算充分保留。

### 离线备份 SQLite

先按 B7 停止 Cloud，然后在**源码根目录**执行：

~~~bash
python3 tools/backup_restore.py backup --database /实际数据/spm-cloud.db --objects /实际模型/objects --destination /新备份目录/cloud-backup-20261016
~~~

Windows 的**解压发行包目录**：

~~~powershell
py .\backup_restore.py backup --database C:\SPMCloud\data\spm-cloud.db --objects C:\SPMCloud\data\objects --destination D:\CloudBackups\cloud-backup-20261016
~~~

目标用新的备份目录。工具检查 SQLite、一致性、对象哈希/字节数和路径；不要在运行中用普通文件复制拼凑数据库与模型目录。此离线工具读取 SQLite 文件，**不能把 MySQL URL 传给 --database**；MySQL 可用网页在线完整备份或数据库自身的受控备份流程。

### 从网页 ZIP 或离线备份恢复

1. 停止 Cloud 和可能重启它的更新器/计划任务。
2. 保留当前故障数据，不覆盖唯一一份副本。
3. 网页 ZIP 先解压；找到直接包含 `manifest.json`、`spm-cloud.db` 和（有对象时）`objects` 的备份目录。
4. 选择**新的**数据库文件和模型目录，例如 `/srv/cloud-restore/spm-cloud.db` 与 `/srv/cloud-restore/objects`。
5. 源码根目录执行：

~~~bash
python3 tools/backup_restore.py restore --backup /解压后的备份根目录 --database /srv/cloud-restore/spm-cloud.db --objects /srv/cloud-restore/objects
~~~

Windows 发行包根目录对应：

~~~powershell
py .\backup_restore.py restore --backup D:\CloudBackups\unpacked --database C:\CloudRestore\spm-cloud.db --objects C:\CloudRestore\objects
~~~

6. 工具成功后，把 config.json 指向**恢复出的数据库和模型目录**；这些恢复结果是 SQLite，`storage.backend` 应为 `sqlite`。还原配套配置、秘密及权限，路径不同则调整引用。
7. 先 `--check-config` 再启动，检查登录、绑定、权限和模型原始文件下载。
8. 确认恢复成功后才决定怎样归档旧数据，最后恢复更新计划。

默认拒绝覆盖已有目标。`--force` 会覆盖数据库并重建目标模型目录，可能删除原内容；仅在停服、保留原副本且明确要覆盖时使用，本文默认命令不带它。备份目录与恢复目标不能相互包含；符号链接/路径重叠会被拒绝。

### Docker 数据怎样处理

本文数据库/模型在命名卷中，主机的 `deploy/runtime` 主要是配置和凭据；仅复制它不能备份模型和账号数据库。网页在线完整备份通常更直观，随后单独备份 runtime、Compose 和代理/证书卷。需要离线卷备份时停止写入后按 Docker 的卷备份流程处理，保持数据库与对象来自同一时点；不要把 `down -v` 当作备份命令。

## B9. 旧 ENV 模式与迁移

### 为什么改了 .env 却没有效果

- Rust 程序**不会自动读取 .env/.env.native 文件**。
- Docker Compose 可以读取 .env 来替换 Compose 变量，但只有模板实际传入容器的变量才会到程序里。
- 已选 JSON 时，业务环境变量不覆盖 JSON，所以改旧 .env 不会改变已选 JSON 的字段。
- 纯 ENV 模式的配置页只读，网页不能把环境变量写回原系统服务、容器或终端。

当前兼容的主要映射：

| ENV 变量 | JSON 对应 / 说明 |
|---|---|
| `SPM_CLOUD_CONFIG` | 选择配置文件；不是字段内容覆盖 |
| `SPM_CLOUD_INSTANCE_ID` / `SPM_CLOUD_ORIGIN` / `SPM_CLOUD_BIND` | `instance.instance_id/origin/bind` |
| `SPM_CLOUD_TRUSTED_PROXY_IPS` | `instance.trusted_proxy_ips`；ENV 用逗号分隔 IP |
| `SPM_CLOUD_DATA_DIR` / `SPM_CLOUD_DATABASE` / `SPM_CLOUD_OBJECT_DIR` | `storage.data_dir/database_path/object_dir` |
| `SPM_CLOUD_ACCESS_TOKEN` | JSON 的 `auth.bootstrap_access_token_file` 引用文件，文件里放同一令牌 |
| `SPM_CLOUD_BOOTSTRAP_ACCOUNT` | `auth.bootstrap_account_id`；保留原服主 ID |
| `SPM_CLOUD_BOOTSTRAP_PASSWORD_HASH` | `auth.bootstrap_password_hash_file` 引用文件，放原 Argon2id 摘要 |
| `SPM_CLOUD_ALLOW_SELF_REGISTRATION` | `auth.allow_self_registration` |
| `SPM_CLOUD_MAX_ASSET_BYTES` / `SPM_CLOUD_MAX_MESSAGE_BYTES` | `limits.max_asset_bytes` / `limits.max_message_bytes`；ENV 与 JSON 均以字节为单位 |
| `SPM_CLOUD_IDENTITY_PROVIDERS` / `SPM_CLOUD_HAS_JOINED_URL` | 旧多服务列表 / 单网关；二选一，迁移到 `auth.identity_providers` 并保留稳定 ID |
| `SPM_CLOUD_MYSQL_URL` | 在读取该兼容 URL 的配置场景中对应 `storage.mysql_url_file`；仅设置 URL 不代表自动切换到 MySQL |
| `SPM_CLOUD_REDIS_URL` | 对应 `cache.redis_url_file`；仅设置 URL 不代表自动开启 Redis 模式 |
| `SPM_CLOUD_MICROSOFT_CLIENT_SECRET` | 兼容秘密来源；JSON 用 `web_ui.microsoft_client_secret_file`。只有秘密、不配置应用 ID 不能开启微软登录 |
| `RUST_LOG` | 旧 ENV 日志过滤；JSON 模式使用 `logging.level` |
| `SPM_CLOUD_CONTAINER` | 部署标记，影响是否允许原生更新，不是业务配置字段 |
| `SPM_CLOUD_IMAGE`、`SPM_CLOUD_CONFIG_DIR`、`SPM_CLOUD_DATA_VOLUME`、`SPM_CLOUD_OBJECTS_VOLUME`、`SPM_CLOUD_NETWORK_SUBNET`、`SPM_CLOUD_PROXY_IP`、`SPM_CLOUD_HOST` | 部署模板变量，分别控制镜像、挂载、网络或旧 Caddy 域名；不是可任意覆盖 JSON 的“高级设置” |

不要根据字段名自行发明 ENV 变量；完整高级设置使用 JSON 或 WebUI。新部署选 `docker-compose.json.yml`，旧 `docker-compose.yml` / `.env.example` 保留作兼容用途。

### 把旧实例迁到 JSON，保持原数据

1. 记录旧版本、启动命令、实际工作目录、ENV 来源、服主 ID、令牌、数据库/模型绝对路径和持久卷名。
2. 停服并做一致性备份。不要把“升级程序”与“换数据库”一起做。
3. 在**另一个空配置位置**初始化 JSON，随后将存储路径改回原实例的绝对路径；不要误用新初始化的空 data 目录。
4. 将旧令牌/摘要放入受保护文件，让 JSON 引用它们；保留固定服主 ID、认证服务 ID、域名、代理和注册策略。
5. 以运行服务的账号执行 `--check-config`。
6. 修改服务/Compose 启动命令为显式 `--config`；Compose JSON 模板还要保留原卷名和正确的容器路径。
7. 正常启动，确认原账号、绑定、权限、模型和文件下载都在，再确认网页可保存。
8. 保留旧 ENV 和备份供回退，不立即清理。新文件中的初始化摘要不会覆盖已有账号密码。

旧模型文件盘点工具 `tools/migrate_legacy.py` 是另一件事，不是自动 ENV→JSON 转换器，也不能证明游戏身份。先 `--help`；只读盘点例如：

~~~bash
python3 tools/migrate_legacy.py /旧模型根目录 --output /安全目录/legacy-inventory.json
~~~

后续 `--import-manifest` 默认生成计划，明确 `--apply-import` 才执行复制；目标是分阶段导入目录，不要直接指向正在运行的生产目录。按实际清单核对归属/身份，不能把历史名称自动当作已验证身份。

## B10. 开发者构建、API 与资料

普通服主下载发行包即可，不需要安装 Rust/Node。开发者需要 Rust **1.88 或更新版本**：

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

修改预览源码 `web/preview.js` 才需 Node.js 22、进入 web 后执行 `npm ci` 和 `npm run build`；预览产物已入库，普通 Cargo 构建不需要现场打包前端。`web` 中的 `npm test` 包含预览与设置测试。

`SPM_TEST_MYSQL_URL`、`SPM_TEST_REDIS_URL` 仅供专用隔离服务的集成测试；缺少时部分用例会提前返回，测试框架显示通过不代表真实外置服务验收。不要向生产数据库运行这些测试。编译后可用 `tools/service_contract.py --binary 实际程序路径` 验证隔离 HTTP 合约；`--model` 为可选测试模型参数。

API 简表：`/v1/instance` 实例发现、`/v1` 业务接口、`/v1/realtime` 实时 WebSocket、`/health` 健康检查。JSON 配置 API 使用 revision 防冲突，支持 validate、固定类型连接检查与 provider 检查；不清楚 API 时使用网页即可。原始模型保存字节、哈希和版本，下载支持 ETag/单范围请求；协议详细资料以文档为准。

| 资料 | 源码仓库中 | 解压发行包中 |
|---|---|---|
| 协议说明 | `docs/PROTOCOL_V1.md` | `PROTOCOL_V1.md` |
| 备份恢复脚本 | `tools/backup_restore.py` | `backup_restore.py` |
| 旧模型盘点工具 | `tools/migrate_legacy.py` | `migrate_legacy.py` |
| 扫描适配与依赖 | `tools/nudenet_service.py`、`tools/requirements-moderation.txt` | 同名文件位于包根目录 |
| Windows 启动脚本 | `deploy/start-cloud.ps1` | `start-cloud.ps1` |
| systemd / 外部更新器 / Compose | `deploy/` 与仓库根目录 | 需要另外从源码仓库获取，不是每个原生包都包含 |
| 配置模板 / schema / 许可 | 仓库根目录 / `web/credits.txt` | 包根目录 / `credits.txt` |

源码协议：[docs/PROTOCOL_V1.md](https://github.com/sdf123098/spm-cloud/blob/main/docs/PROTOCOL_V1.md)。项目使用 [MIT](LICENSE)；界面字体/图标许可在网页“字体与图标许可”及随包 credits 文件中。离线认证测试夹具仅含合成公钥/证书/公开签名，不含私钥或真实游戏凭据；生产信任根不能用请求或 ENV 替换。

---

# 故障排查：按现象处理

按顺序检查，每一步确认结果再继续；不要一上来删库、删卷或重新初始化。

| 现象 | 先检查什么 | 怎样判断修好 |
|---|---|---|
| 双击 exe 一闪而过 | 从 PowerShell 运行，读完整错误；确认配置路径和端口 | 终端持续运行，有监听日志 |
| 本机网页能开，朋友打不开 | 127.0.0.1 只指访问者自己；检查公网域名、代理、DNS/防火墙 | 外部网络能打开 HTTPS 的 /health 和 /admin/ |
| Docker 一直 502 | Cloud 是否监听 0.0.0.0；Caddy 上游是否 spm-cloud:8787；日志/容器网络 | Caddy 可到达上游，公网页返回正常 |
| 提示端口占用 | 是否已有服务/前台第二实例；不要随便结束陌生进程 | 只保留原计划的实例或选择正确新端口 |
| 令牌框空白 | 这是输入框，到实际服务器读取当前引用文件 | 用完整令牌成功登录 |
| 原令牌突然不行 | 是否刚生成/替换过令牌，仍在读旧 secrets 文件 | 用最新引用的值或已设置的账号密码登录 |
| 忘记服主密码 | 还有初始令牌则用令牌改密；两者都丢失则停服恢复可信凭据备份 | 能登录且账号 ID/数据正确；不要创建新空实例冒充恢复 |
| 管理员按钮灰色 | 正常权限；需要服主账号 | 使用服主后可改允许修改项 |
| 服主也无法保存 | ENV 只读？是否选对 JSON？服务账号能否写文件及父目录？依赖条件是否满足？ | 校验和保存成功，实际值/待重启提示正确 |
| 409 冲突 | 文件被网页/外部编辑同时修改 | 按 A2 比较并保留草稿重试成功 |
| 保存了端口/日志却没变 | 需要重启，刷新页面无效 | 重启后正在使用值等于保存值 |
| 413 / 文件过大 | 每个模型字节数、Cloud 上限和代理 body 上限 | 在两处限制内的模型上传成功 |
| 私有模型看不到 | 当前实际关联 Cloud 账号、归属、共享权限与内容阻止状态 | 正确账号能下载，未授权账号仍不能 |
| 模型全没了 | 是否改到新空数据库/对象目录、误换卷名或工作目录 | 指回经核对的原路径/卷后原数据存在 |
| 扫描失败 / 全部未识别 | /infer 协议、扫描进程、地址、超时、贴图预算、YSMParser | 真实测试模型完成识别；仅健康接口响应不够 |
| 开严格拦截后无法上传 | block_unclassified 与检测关闭/失败组合 | 先修扫描及策略，明确允许/拒绝的测试结果 |
| 第三方服务检查失败 | 真正 API 入口、HTTPS、公网 DNS、显式端口、跳转；不要绕过地址检查 | 检查正确报告；再单独验证真实登录/绑定 |
| 微软回调失败 | Client ID、密钥值及到期、个人账号支持、完整回调地址与外网访问 | 从原浏览器重新开始能完成身份验证 |
| MySQL 失败 | 实际主机/端口、库是否存在、权限、密码、TLS/网络；不是拿 SQLite 路径当库名 | 连接检查成功，重启后实际后端正确 |
| Redis 失败 | 实际网络、ACL/密码、TLS 与索引，容器 localhost 是否填错 | 连接检查及正常使用成功；离线时内存回退正常 |
| 更新按钮不可用 | 容器本来由平台更新；原生还需目录可写、平台包匹配与解压工具 | 正确更新方式完成，--version 和健康状态正确 |
| 旧日志没有导出来 | 网页只有当前进程最近 2 MiB | 在 Docker/systemd/自己的日志存储中找到历史 |
| 备份恢复拒绝覆盖 | 目标已存在、路径重叠、哈希不符或符号链接 | 改用新的合法目标并核对原备份，不能直接强制跳过校验 |
| 网页被关了 | 先找实际 config.json，再按 B7 离线恢复 | /admin/ 重新可访问；不需要重置账号 |
