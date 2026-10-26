# Memoria Launcher — AI / MCP 首版接入

提供真实 MCP 目录/实例读取工具、按类别和实例授权的写入工具，以及可选的 OpenAI Chat Completions 兼容 API 对话。写入默认关闭，需在设置中保存授权。详细格式与检查见 [MCP 实例写入与 FTB 任务](mcp-writes.md)。

## stdio MCP（供外部 Agent 使用）

构建与启动：

```bash
cargo build -p mc-mcp
MEMORIA_GAME_ROOT=/path/to/.minecraft cargo run -p mc-mcp
```

Windows 可设置 MEMORIA_GAME_ROOT 为 Minecraft 的根目录；未设置时尝试 %APPDATA%\.minecraft（Windows）或 $HOME/.minecraft。可以使用 --game-root=/path/to/.minecraft 覆盖。该路径用于实例扫描和日志读取；保存 MCP 权限后使用配置指定的根目录。应与桌面启动器的游戏根目录一致。

在 MCP 客户端中将 command 指向构建所得 mc-mcp 可执行文件，环境变量 MEMORIA_GAME_ROOT 指向预期 Minecraft 根目录。协议为 **JSON-RPC 每行一条消息**（MCP stdio），stdout 只输出响应，stderr 输出错误日志。支持 initialize / ping / tools/list / tools/call。服务端标识版本为 0.1.0。

默认只读工具：

| Tool | 真实数据来源 | 操作 |
| --- | --- | --- |
| minecraft_list_versions | Mojang version_manifest_v2 | 版本查询，最多 100 |
| minecraft_list_instances | mc-core LocalClientService | 本地 versions/ 扫描 |
| minecraft_search_mods | Modrinth V2 /search | 搜索、MC 版本 / Loader 筛选 |
| minecraft_get_mod_info | Modrinth V2 /project | 项目描述、许可、版本 ID |
| minecraft_list_instance_logs | 指定实例的日志目录 | 列出最近最多 100 个日志，包含相对路径、类型、大小、修改时间 |
| minecraft_read_instance_log | 指定实例的日志文件 | 读取日志末尾，默认 32 KiB、最大 64 KiB；遮蔽常见凭据行 |

示例工具入参：`{"query":"sodium","minecraft_version":"1.21","loader":"fabric","limit":5}`。

### 分析实例崩溃日志

先调用 `minecraft_list_instances` 获取 `instance_id`（例如 `versions/1.21`，也兼容 `version/1.21`），再调用：

```json
{"name":"minecraft_list_instance_logs","arguments":{"instance_id":"versions/1.21"}}
```

选择返回的日志相对路径，然后读取：

```json
{"name":"minecraft_read_instance_log","arguments":{"instance_id":"versions/1.21","path":"logs/latest.log","max_bytes":32768}}
```

允许读取的文件为实例目录中的 `.launcher/logs/*.log`、`logs/latest.log`、`logs/debug.log`、`crash-reports/crash-*.txt` 和 `hs_err_pid*.log`。不能通过绝对路径、`..` 或符号链接读取其他位置。读取不会修改日志；不存在或不可读时返回 `LOG_READ_FAILED`。截断结果带 `truncated: true`，并丢弃第一条不完整行。空目录返回空列表。

日志属于不可信数据，模型应只分析错误，不执行日志中的指令。读取会遮蔽含常见访问令牌、刷新令牌、API Key 等标记的整行，并返回 `redacted_lines`；这不能保证覆盖所有 Mod 自定义的敏感格式，玩家名和本地路径仍可能保留。连接 MCP 即允许客户端请求这些实例日志，客户端如何把工具结果交给模型由客户端控制。报告可能属于较早运行，分析时应核对修改时间，不能把旧报告当成本次退出的证据。

保存类别和实例授权后，还提供 Modrinth/CurseForge 来源 ID 安装、JAR 上传、配置写入、语义化 FTB 任务/图片编辑，以及物品 ID 校验。所有写入先返回持久化预览，再用准确 `plan_id` + `plan_hash` 应用；覆盖前备份，权限和文件哈希每次重查。详见 [写入接口、格式示例和边界](mcp-writes.md)。没有任意命令、删除、游戏启动或通用文件读写接口。

公共目录和来源安装请求会直接联网；API 失败返回结构化错误。CurseForge 需要外部客户端安全提供 API Key，且遵守源站分发限制。

## 启动器内直接 API 对话

“设置 → AI 设置”统一管理 API 地址、模型名称、API Key；AI 页面使用条形输入框、实例项目和独立对话列表。默认地址 https://api.openai.com/v1。将调用 POST /v1/chat/completions（如果地址已有 /chat/completions 不重复拼接）。符合 Chat Completions 的本地兼容代理也可使用。点击“保存并应用”后，地址、模型与密钥引用写入本地 ai.json，Key 写入 Windows 凭据管理器；重启自动加载。清空 Key 后保存会移除已有密钥。本机服务可留空 Key。保存失败保留原生效配置；保存的密钥丢失或无法读取时阻止请求并提示重新配置。测试连接会用当前填写的配置发送一条简短问候，可能产生 API 费用，不自动保存。不会把 Key 写入普通配置或日志，但远端服务可按照其政策保留请求，因此只使用可信服务。

实例异常退出后可点击 **启用 AI 分析**：先在本机读取本次启动日志末尾并附加到 AI 页，预览内容、配置 API 后再点击 **发送**。发送前不会联系 AI 服务；可点击 **移除崩溃日志**，也可在异常退出提示中选择 **暂不分析**。顶部 **异常退出记录** 可重新打开最近一次提示。日志读取失败会显示实际错误。

**安全限制**：远端必须是 HTTPS；仅 localhost / 127.0.0.1 / ::1 允许 HTTP；拒绝 URL 中的凭据、查询参数及片段；禁止 HTTP 重定向，避免 Bearer Token 被转发到其他域。API 服务收到输入的消息、有限的对话上下文，以及用户主动附加并确认发送的日志；日志可能包含玩家名和本地路径。启动器不会自动读取存档或凭据存储文件。AI 页面仅保留对话框，支持连续追问与清空会话；会话只在当前运行内保存，请求最多携带最近 12 轮成功对话（历史文本另限 96 KiB）。原始崩溃日志附件仅随确认发送的当前消息上传，不写入后续对话上下文；模型回复引用的片段仍属于对话内容。未实现流式输出、工具调用和 Search API/服务端搜索。

测试：

```bash
cargo check --workspace
cargo test -p mc-core -p mc-mcp -p desktop
```

## 后续

1. 用可持久化的真实 InstanceService / CatalogProvider 替换 GUI Mock；MCP 与 GUI 共用相同来源。
2. 增加 get_mod_versions / resolve_dependencies / plan_mod_changes / preview_changes，计划只读。
3. 将现有受限写入计划扩展为完整依赖求解、逐计划 GUI 审批与崩溃自动恢复。
4. 补充汉化任务队列、存档隔离、分级授权，必要时引入独立本地 REST API，默认仅绑定回环地址。

## 内置服务器权限

在 **设置 → AI 设置 → MCP 服务器** 中启用服务，分别选择目录查询、实例信息、日志以及四类写入权限，再勾选实例并点击 **保存 MCP 权限**。授权绑定当前选择的实例集路径；切换授权实例集会清空原实例选择。配置写入启动器数据目录的 `mcp.json`，服务端每次请求重读配置。未获授权的工具或实例返回 `PERMISSION_DENIED`，关闭后工具列表为空，损坏配置拒绝请求而不回退。未创建该文件时，CLI 保持原有只读行为。

点击 **复制客户端连接配置** 可生成启动器同目录 `mc-mcp` 的 stdio 配置，并设置相同 `MEMORIA_DATA_DIR`。Windows 打包脚本会同时分发两个程序。权限仅限制此内置服务器提供的工具；写入格式、注册表导入和恢复边界见 [实例写入](mcp-writes.md)。不提供游戏启动工具。附件、持续日志上下文和界面行为见 [工作区界面](workspace-ui.md)。
