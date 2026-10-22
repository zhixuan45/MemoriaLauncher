# Memoria Launcher — AI / MCP 首版接入

本轮功能：真实 **只读** MCP 目录/实例工具 + 可选的 OpenAI Chat Completions 兼容 API 对话。均不安装游戏、修改配置、调用命令或代替用户批准计划。

## stdio MCP（供外部 Agent 使用）

构建与启动：

```bash
cargo build -p mc-mcp
MEMORIA_GAME_ROOT=/path/to/.minecraft cargo run -p mc-mcp
```

Windows 可设置 MEMORIA_GAME_ROOT 为 Minecraft 的根目录；未设置时尝试 %APPDATA%\.minecraft（Windows）或 $HOME/.minecraft。可以使用 --game-root=/path/to/.minecraft 覆盖。该路径用于本地**只读扫描和实例日志读取**，不自动创建目录；应与桌面启动器的游戏根目录一致。

在 MCP 客户端中将 command 指向构建所得 mc-mcp 可执行文件，环境变量 MEMORIA_GAME_ROOT 指向预期 Minecraft 根目录。协议为 **JSON-RPC 每行一条消息**（MCP stdio），stdout 只输出响应，stderr 输出错误日志。支持 initialize / ping / tools/list / tools/call。服务端标识版本为 0.1.0。

提供的工具：

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

当前 **不**提供 minecraft_create_instance、apply_changes、localize、launch、任意文件读取、终端命令等写入工具。即使模型生成了安装计划，也只是文本建议。后续实现必须先建立真实计划持久化、实例 revision + plan_hash 校验、GUI 显式审批、事务式部署回滚与审计记录；不能靠在请求里填 approved=true 来授权。

只有公共 Modrinth/Mojang 请求会由 MCP 服务直接联网；实例和日志工具仅把读取结果返回 MCP 客户端。API 失败返回结构化错误而非假数据。应当考虑 API 限流和无网络的情形。

## 启动器内直接 API 对话

“设置 → AI 设置”统一管理 API 地址、模型名称、API Key；AI 页面显示当前生效的服务，并提供设置入口。默认地址 https://api.openai.com/v1。将调用 POST /v1/chat/completions（如果地址已有 /chat/completions 不重复拼接）。符合 Chat Completions 的本地兼容代理也可使用。点击“保存并应用”后，地址、模型与密钥引用写入本地 ai.json，Key 写入 Windows 凭据管理器；重启自动加载。清空 Key 后保存会移除已有密钥。本机服务可留空 Key。保存失败保留原生效配置；保存的密钥丢失或无法读取时阻止请求并提示重新配置。测试连接会用当前填写的配置发送一条简短问候，可能产生 API 费用，不自动保存。不会把 Key 写入普通配置或日志，但远端服务可按照其政策保留请求，因此只使用可信服务。

实例异常退出后可点击 **启用 AI 分析**：先在本机读取本次启动日志末尾并附加到 AI 页，预览内容、配置 API 后再点击 **发送**。发送前不会联系 AI 服务；可点击 **移除崩溃日志**，也可在异常退出提示中选择 **暂不分析**。顶部 **异常退出记录** 可重新打开最近一次提示。日志读取失败会显示实际错误。

**安全限制**：远端必须是 HTTPS；仅 localhost / 127.0.0.1 / ::1 允许 HTTP；拒绝 URL 中的凭据、查询参数及片段；禁止 HTTP 重定向，避免 Bearer Token 被转发到其他域。API 服务收到输入的提示词、目标版本、输出语言，以及用户主动附加并确认发送的日志；日志可能包含玩家名和本地路径。存档和凭据存储文件不作为附件读取。未实现流式输出、工具调用、Search API/服务端搜索和对话历史，页面的联网搜索开关仍为未接入状态。

测试：

```bash
cargo check --workspace
cargo test -p mc-core -p mc-mcp -p desktop
```

## 后续

1. 用可持久化的真实 InstanceService / CatalogProvider 替换 GUI Mock；MCP 与 GUI 共用相同来源。
2. 增加 get_mod_versions / resolve_dependencies / plan_mod_changes / preview_changes，计划只读。
3. 持久化变更计划 + 用户审批和事务、审计后才暴露 apply_changes。
4. 补充汉化任务队列、存档隔离、分级授权，必要时引入独立本地 REST API，默认仅绑定回环地址。
