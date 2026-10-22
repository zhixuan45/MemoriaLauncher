# Memoria Launcher — AI / MCP 首版接入

本轮功能：真实 **只读** MCP 目录/实例工具 + 可选的 OpenAI Chat Completions 兼容 API 对话。均不安装游戏、修改配置、调用命令或代替用户批准计划。

## stdio MCP（供外部 Agent 使用）

构建与启动：

```bash
cargo build -p mc-mcp
MEMORIA_GAME_ROOT=/path/to/.minecraft cargo run -p mc-mcp
```

Windows 可设置 MEMORIA_GAME_ROOT 为 Minecraft 的根目录；未设置时尝试 %APPDATA%\.minecraft（Windows）或 $HOME/.minecraft。可以使用 --game-root=/path/to/.minecraft 覆盖。该路径仅用于本地**只读扫描**，不自动创建目录。

在 MCP 客户端中将 command 指向构建所得 mc-mcp 可执行文件，环境变量 MEMORIA_GAME_ROOT 指向预期 Minecraft 根目录。协议为 **JSON-RPC 每行一条消息**（MCP stdio），stdout 只输出响应，stderr 输出错误日志。支持 initialize / ping / tools/list / tools/call。服务端标识版本为 0.1.0。

提供的工具：

| Tool | 真实数据来源 | 操作 |
| --- | --- | --- |
| minecraft_list_versions | Mojang version_manifest_v2 | 版本查询，最多 100 |
| minecraft_list_instances | mc-core LocalClientService | 本地 versions/ 扫描 |
| minecraft_search_mods | Modrinth V2 /search | 搜索、MC 版本 / Loader 筛选 |
| minecraft_get_mod_info | Modrinth V2 /project | 项目描述、许可、版本 ID |

示例工具入参：`{"query":"sodium","minecraft_version":"1.21","loader":"fabric","limit":5}`。

当前 **不**提供 minecraft_create_instance、apply_changes、localize、launch、任意文件读取、终端命令等写入工具。即使模型生成了安装计划，也只是文本建议。后续实现必须先建立真实计划持久化、实例 revision + plan_hash 校验、GUI 显式审批、事务式部署回滚与审计记录；不能靠在请求里填 approved=true 来授权。

只有公共 Modrinth/Mojang 请求会联网；读取本地实例时不会上传实例目录或账户令牌。API 失败返回结构化错误而非假数据。应当考虑 API 限流和无网络的情形。

## 启动器内直接 API 对话

AI 页面有 API 地址、模型名称、API Key 输入框；默认地址 https://api.openai.com/v1。将调用 POST /v1/chat/completions（如果地址已有 /chat/completions 不重复拼接）。符合 Chat Completions 的本地兼容代理也可使用。Key 只保存在本次内存；重启需要重新填入。无持久化、不会写入日志，但无法防止远端服务按照其政策保留请求，因此只使用可信服务。

**安全限制**：远端必须是 HTTPS；仅 localhost / 127.0.0.1 / ::1 允许 HTTP；拒绝 URL 中的凭据、查询参数及片段；禁止 HTTP 重定向，避免 Bearer Token 被转发到其他域。API 服务收到的是输入的提示词、目标版本与输出语言，不包含本地目录、存档或认证信息。未实现流式输出、工具调用、Search API/服务端搜索、对话历史和密钥持久化，页面的联网搜索开关仍为未接入状态。

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
