# Memoria Launcher：Microsoft / Minecraft Java 正版登录

## 本次实现范围

iced 桌面端的侧边栏玩家档案与「设置 → 常规 → 账户与数据」可打开正版账号管理弹窗。使用系统默认浏览器进行 Microsoft OAuth 2.0 授权码 + PKCE 登录；启动器仅在本地回环接口 `127.0.0.1` 接收回调，随后由 `mc-core` 完成 Xbox Live、XSTS、Minecraft Services、正版权益和 Java 玩家档案验证。认证成功后可以在同一会话中添加多个账户、切换当前账户、退出并清理 refresh token。

桌面实例列表扫描用户配置的本地 Minecraft 目录，启动按钮通过 `GameLauncher` 使用所选账号 ID；准备启动时调用 `minecraft_session` 刷新并验证游戏会话。Java、游戏文件或依赖缺失时会显示实际错误。

## 面向玩家：一键登录

安装由 Memoria Launcher 发行方提供的正式构建，打开侧边栏账号管理，点击 **「使用 Microsoft 登录」**，在系统默认浏览器完成 Microsoft 授权；本机回调会让启动器自动完成 Minecraft Java 正版权益及档案验证。

普通玩家 **不需要注册 Azure/Entra 应用，也不用提供客户端 ID 或回调 URI**。首次认证仍需在微软官方页面确认身份、可能需要同意权限。Windows 构建将 refresh token 保存到系统凭据管理器，账号名称、UUID 和当前选择单独保存在 `accounts.json`；重新打开启动器可恢复账号，并在启动游戏时验证会话。其他平台尚未接入系统凭据存储。

## 面向发行方：只注册一次应用

1. 在 [Microsoft Entra 应用注册](https://entra.microsoft.com/) 创建应用，支持个人 Microsoft 账户，选择 **Mobile and desktop applications（公共客户端）**；为目前的登录回调配置与程序**路径完全一致**的自定义回调 URI，例如 `http://localhost/oauth/callback`（或 `http://localhost:53682/oauth/callback`，取决于 Entra 页面允许的录入格式）。Entra 对 `localhost` 的端口匹配有特殊规则，但不会忽略路径；**仅配置 `http://localhost` 而启动器使用 `/oauth/callback` 会导致回调不匹配**。启用公共客户端流程。不能使用 client secret 嵌入桌面应用。
2. 记录这 **一个** 应用的 Application (client) ID，构建 Memoria Launcher 时通过环境变量 `MEMORIA_MICROSOFT_CLIENT_ID` 注入。例如：

   ```sh
   MEMORIA_MICROSOFT_CLIENT_ID=your-registered-client-id cargo build -p desktop --release
   ```

   PowerShell 示例：

   ```powershell
   $env:MEMORIA_MICROSOFT_CLIENT_ID = "your-registered-client-id"
   cargo build -p desktop --release
   ```

3. 在自动打包流水线（GitHub Actions 等）中，把 `MEMORIA_MICROSOFT_CLIENT_ID` 设置成仓库变量并传入 **编译步骤**。Client ID 是公共标识符，不需要保存为密码；**不要**把 Microsoft client secret 放在发行包或 GitHub 仓库里。
4. 将已嵌入 ID 的构建发布给玩家。玩家无需填写任何配置。**当前仓库未附带真实 Microsoft 应用 ID，因此未经发行方配置的普通构建会禁用登录按钮并提示发行方配置缺失。**

**开发者专用覆盖：** 本机测试可用运行时环境变量 `MC_LAUNCHER_CLIENT_ID` 暂时覆盖嵌入的客户端 ID；`MC_LAUNCHER_REDIRECT_URI` 可以覆盖回调。普通玩家没有这些设置入口。固定本机端口默认为 `53682`，端口冲突时仅开发者可改动并确保与应用注册要求一致。


## 安全边界与限制

- 登录使用 PKCE S256 和一次性 OAuth `state`；完成登录时核对回调 URI 与 `state`。OAuth 查询参数必须在交换前 URL 解码。
- 只接受绑定在 `127.0.0.1` 的 HTTP 回调；不接收公网 HTTP 回调。请勿将回调地址改为 `0.0.0.0`。
- 认证结果必须经 Minecraft Services 权益及 Java profile 端点验证；昵称不是正版身份凭据。
- Windows refresh token 保存在 Credential Manager 的 `MemoriaLauncher/Microsoft/<account-id>` 条目中；`accounts.json` 不包含令牌。其他平台目前明确报告安全存储不可用。
- 登出操作删除系统凭据管理器中的 refresh token 和本地账号记录，不撤销微软服务器上的全部会话；服务器端撤销需走 Microsoft 账号的应用权限管理。
- 客户端 ID 是公共标识符，不是客户端密钥；应使用 Memoria 自己注册并管理的 Microsoft 应用 ID；不能借用其他启动器应用的 ID。Minecraft Services/Xbox Live 对应用的授权或资格要求也必须单独满足。
- 目前没有完整集成测试可使用真实的微软账号与 Minecraft 游戏权益；需要在开发机上实际完成一次登录和令牌刷新、检查 OAuth 应用注册配置。

## 本地检查

```sh
cargo fmt --all --check
cargo test -p mc-core --locked
cargo check -p desktop --locked
cargo test -p desktop --locked
```
