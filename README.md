![Memoria Launcher — Minecraft Java Edition Launcher](docs/assets/readme-hero.png)

# Memoria Launcher

**一个使用 Rust 与 iced 构建的 Minecraft Java Edition 桌面启动器。**

管理本地游戏版本、通过 Microsoft 账户登录并启动游戏，也为外部 AI 客户端提供只读 MCP 工具。项目处于开发阶段，优先接入真实数据，并明确展示尚未完成的功能。

An independent Minecraft Java Edition launcher built with Rust and iced. It provides local instance discovery, Microsoft account authentication, authenticated game launching, and optional read-only MCP tools. This project is under active development and is not affiliated with or endorsed by Microsoft or Mojang Studios.

[Microsoft 登录说明](docs/microsoft-login.md) · [API 申请与审核说明 / API review](docs/api-application.md) · [数据来源与配置](docs/real-data-integration.md) · [AI / MCP](docs/mcp.md)

## 已实现

| 功能 | 当前行为 |
| --- | --- |
| 本地游戏库 | 扫描 `versions/` 与兼容的 `version/` 目录，读取版本、加载器、Java 要求与客户端 JAR 状态 |
| 账户管理 | 系统浏览器 Microsoft OAuth 授权码 + PKCE 登录，支持添加、切换与退出账户 |
| 登录应用设置 | 在 **设置 → 账户登录** 填写客户端 ID 和回调地址，保存后立即生效，重启自动读取 |
| 游戏启动 | 使用所选账户刷新并验证会话，检查 Java 与本地依赖，启动 Java 进程并显示日志路径与退出状态 |
| 官方版本目录 | 从 Mojang 官方版本清单读取真实版本、发布时间与元数据地址 |
| 本地统计 | 统计实例隔离目录内的 Mod JAR 数量；尚未记录的游玩时长显示“未记录” |
| AI 对话 | 可选的 Chat Completions 兼容接口，由用户提供地址、模型与 API Key；不自动执行本地操作 |
| MCP 服务 | 通过 stdio 查询 Mojang 版本、Modrinth 项目与本地实例，当前仅提供只读工具 |

**尚未完成：** 游戏下载与自动安装、桌面端第三方资源目录、AI 自动组包与汉化、安装变更审批与事务部署、非 Windows 系统的安全凭据存储。界面不会为这些功能生成虚构的成功结果或下载进度。

## 本地运行

安装 Rust stable 工具链。Windows 构建使用 MSVC 工具链，需要 Visual Studio Build Tools 的 C++ 桌面开发组件；启动 Minecraft 另需对应版本的 Java 与完整游戏文件。

```powershell
git clone https://github.com/zhixuan45/MemoriaLauncher.git
cd MemoriaLauncher
cargo run --locked -p desktop
```

首次使用：

1. 在 **设置 → 环境配置** 选择包含 `versions`、`libraries`、`assets` 的游戏根目录，填写 Java 路径，点击 **保存并重新扫描**。
2. 如果构建尚未内置登录应用 ID，在 **设置 → 账户登录** 填写自己注册的 Microsoft 应用客户端 ID；默认回调为 `http://localhost:53682/oauth/callback`，应与应用注册一致。点击 **保存登录配置**，无需反复设置环境变量。
3. 点击左下角账户区域，选择 **添加 Microsoft 账户**，在系统浏览器完成授权。
4. 选择完整的本地实例，点击 **开始游戏**。Java、凭据或依赖问题会显示实际错误。

普通玩家使用已配置应用 ID 的发行版时无需注册 Microsoft 应用。开发者可以通过 `MEMORIA_MICROSOFT_CLIENT_ID` 在编译时内置自己的应用 ID；本机设置优先于运行时环境变量和发行版默认值。完整流程见 [Microsoft 登录说明](docs/microsoft-login.md)。

## Microsoft / Minecraft API 用途

本项目希望让拥有 Minecraft Java Edition 权益的用户在独立桌面启动器中登录自己的账户，读取自身的 Java 玩家档案，并启动本地游戏。认证链路为：

`系统浏览器 → Microsoft OAuth + PKCE → Xbox Live → XSTS → Minecraft Services → 权益与玩家档案校验 → 本地启动`

OAuth 请求的 scope 为 `XboxLive.signin offline_access`。Microsoft 账户密码在微软登录页面输入，启动器不收集密码；应用不使用嵌入式客户端密码。Windows 刷新令牌保存在系统凭据管理器，账号索引与登录应用配置文件不包含令牌。AI 对话与只读 MCP 工具不接收 Microsoft/Minecraft 凭据。

**项目没有声称已获 Microsoft、Xbox 或 Minecraft API 批准。** 应用注册、相应服务的访问资格及真实账户端到端测试仍需由维护者完成。供审核人员查看的用途、权限、数据处理和代码入口见 [API 申请与审核说明](docs/api-application.md)。

## AI 与 MCP

AI 页面可填写兼容接口地址、模型名称与 API Key。API Key 仅在当前进程内存中保留；提示词、目标版本与输出语言会发送给用户选定的服务。当前 AI 对话不调用本地工具，也不会代替用户安装或修改实例。

单独构建只读 MCP 服务：

```powershell
cargo build --locked -p mc-mcp
.\target\debug\mc-mcp.exe --game-root=C:\Games\.minecraft
```

提供 `minecraft_list_versions`、`minecraft_list_instances`、`minecraft_search_mods`、`minecraft_get_mod_info` 四个工具。客户端配置与参数示例见 [MCP 文档](docs/mcp.md)。

## 配置与凭据

| 内容 | Windows 默认位置 |
| --- | --- |
| Microsoft 应用客户端 ID、回调地址 | `%LOCALAPPDATA%\MemoriaLauncher\microsoft-login.json` |
| 账户名称、UUID、当前账户 | `%LOCALAPPDATA%\MemoriaLauncher\accounts.json` |
| 游戏目录与 Java 路径 | `%LOCALAPPDATA%\MemoriaLauncher\environment.json` |
| Microsoft 刷新令牌 | Windows Credential Manager：`MemoriaLauncher/Microsoft/<account-id>` |

`MEMORIA_DATA_DIR` 可覆盖启动器配置目录。退出某个账户会删除对应的本地刷新令牌和账户记录；微软服务器端的权限撤销由用户在 Microsoft 账户中管理。

## 开发与验证

```powershell
cargo check --workspace --locked
cargo test --locked -p desktop -p mc-core -p mc-mcp -- --test-threads=1
cargo build --locked -p desktop
```

Mojang 官方清单的联网检查单独执行：

```powershell
cargo test --locked -p mc-core official_catalog_is_reachable -- --ignored
```

离线测试不代表真实 Microsoft 登录、第三方 API 访问资格或游戏进程端到端验证已完成。

| 路径 | 职责 |
| --- | --- |
| `apps/desktop` | iced 界面、账户与设置、本地启动、可选 AI 对话 |
| `crates/mc-core` | 领域模型、认证、目录查询、游戏扫描与启动服务 |
| `crates/mc-mcp` | 只读 MCP stdio 服务 |
| `crates/vendor/mc-launcher-core` | 随仓库保留的第三方启动库及本地修改 |
| `docs` | 认证、API 审核、MCP、架构与配置说明 |

## 反馈与许可

问题与反馈请提交到 [GitHub Issues](https://github.com/zhixuan45/MemoriaLauncher/issues)。提交日志时请先移除访问令牌、刷新令牌与 API Key。

主项目目前尚未指定统一的开源许可证。第三方组件的许可与版权声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 及对应组件目录。头图为 AI 生成的项目宣传插画，不是应用截图或游戏内截图。

Minecraft 是 Mojang Studios 的商标。Memoria Launcher 是独立项目，与 Microsoft 或 Mojang Studios 没有隶属或官方背书关系。
