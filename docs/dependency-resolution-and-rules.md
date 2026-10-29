# 通用日志规则与 Mod 依赖求解

## 日志规则

“日志诊断”使用声明式规则匹配已经限量读取、遮蔽凭据及常见绝对路径的日志。内置 19 条规则覆盖依赖、Java、内存、Mixin、类加载、网络、图形、JVM 崩溃，以及重复 Mod、损坏归档、磁盘、权限、配置、文件缺失、启动参数和端口占用等错误。每条结果包含规则 ID、严重级别、置信度、证据、片段内行号和可提取的结构化字段。行号相对于实际分析的日志尾部片段；不是原文件的绝对行号。

“导入诊断规则”接受如下 JSON，将自定义规则追加到内置规则；ID 不可重复。保存位置为启动器数据目录的 `diagnostic-rules.json`，支持 `MEMORIA_DATA_DIR`。游戏退出摘要、手动分析和 MCP 使用同一配置。“恢复内置规则”将原文件改名为带 UUID 的 `.bak` 备份。导入前检查格式和编译正则，失败保留原配置；已保存配置损坏时明确报错。

```json
{
  "version": 1,
  "rules": [{
    "id": "example_registry_error",
    "category": "registry_error",
    "title": "注册表初始化失败",
    "action": "核对该 Mod 与前置的版本，并查看完整异常链。",
    "severity": "error",
    "confidence": "medium",
    "priority": 10,
    "any": ["Registry initialization failed for (?P<mod_id>[a-z0-9_-]+)"],
    "all": ["(?i)exception|error"],
    "exclude": ["(?i)recovered successfully"],
    "context_lines": 2,
    "capture_fields": ["mod_id"]
  }]
}
```

`any` 任一正则触发锚点；`all` 必须在锚点上下文全部匹配；`exclude` 任一匹配则排除。优先级只控制结果顺序，多条规则可同时报告。严重级别为 `info/warning/error`，置信度为 `low/medium/high`。命名提取字段限 `mod_id`、`required_mod_id`、`required_version`、`exception`、`class_name`、`required_java`、`java_version`、`file`。

文件最多 128 KiB，合计最多 64 条规则、128 个正则；每条 `any/all/exclude` 各最多 4 个，单表达式最多 2048 字节，上下文最多前后 8 行。正则有编译预算，每规则最多检查 256 个锚点，上下文最多 16 KiB，最多保留三段各 500 字符证据。达到预算时返回 `rule_limit_reached`。规则不能执行代码、联网、上传日志或自动修复；未匹配不表示运行正常。

## 本地前置与冲突检查

打开“管理实例 → Mod 管理 → 检查前置与冲突”。检查当前启用的 `.jar`，忽略 `.jar.disabled`，读取 Fabric、Quilt、Forge/NeoForge 元数据和声明的嵌套 JAR。检查结果保留依赖边、版本要求、冲突来源、循环和变更建议。“选择候选 JAR 求解”可选择最多 16 个文件，检查新增或替换后的组合；此入口只给建议，文件不会被安装、禁用或删除。

求解同时处理必需前置、版本范围、实际/提供的 Mod ID、双向冲突及客户端环境。可选和推荐依赖仅提示，Fabric `conflicts` 等软冲突保留警告；已满足的循环允许存在。同一 JAR 声明的多个 Mod 作为整体选择，Quilt 提供的能力使用各自版本。

Fabric 支持常见比较、空格 AND、`||`、`~`、`^` 和通配符；Forge/NeoForge 支持 Maven 区间、精确区间和区间并集及常见版本限定词。无法可靠解释的表达式或元数据返回未知。Minecraft、加载器和 Java 使用实例声明；Java 事实不是检测所选 Java 可执行文件的结果。Quilt 对 Fabric 的兼容能力、不确定的条件依赖和未模拟的嵌套激活会提示未知。

结果状态为 `resolved`、`conflict`、`unknown`、`limit_reached`。无解仅针对当前有限候选集；未知和预算耗尽不证明兼容或全局无解。最多 2048 个候选、256 个根和已选节点、20000 个搜索状态，并限制候选检查工作量。实例最多读取 1024 个启用文件、累计 512 MiB；单 JAR 最多 32 MiB。嵌套归档另有深度、展开大小和条目预算，不执行 JAR 或加载器。

## 在线安装与只读计划

GUI 的 Modrinth Mod 安装预览先构造候选依赖图，再回溯选择满足全部必需约束的组合，保留其他已有来源项目的版本。不是直接选择第一个兼容版本。项目最多检查 32 个候选版本，整个图最多 128 个项目、512 次元数据请求；截断或来源失败会显式提示。

预览还下载并校验所选 JAR，将其声明与保留的本地 Mod 一起检查，硬冲突会阻止预览。来源记录必须与实际文件哈希一致。提交前复查全部依赖 JAR 内容和父版本 JSON 链，变化后要求重新预览；通过检查后才进入原有确认、备份和失败回滚流程。未记录来源的 JAR 可以检查本地 Mod ID，但不会猜测它对应哪个 Modrinth 项目。

MCP 增加只读 `minecraft_resolve_dependencies`。仅传 `instance_id` 返回本地检查；可附固定 Modrinth 项目/版本的 `roots` 请求远程计划：

```json
{"name":"minecraft_resolve_dependencies","arguments":{"instance_id":"versions/fabric-test","roots":[{"project":"YL57xq9U","version":"zsoi0dso"}]}}
```

示例版本须匹配目标实例。返回 `local_report`、`remote_plan`、Mod 哈希和环境摘要；不创建安装计划文件或写入 JAR。需要实例读取权限及实例白名单，提供非空 `roots` 还需要目录读取权限。远程计划仅针对源站项目约束，本地报告单独返回；MCP 此接口不下载候选 JAR 做安装后组合验证。它不是安装授权，也不替代 GUI 的最终预览和确认。

尚未覆盖任意运行期 Mixin/类加载行为、自动识别未知文件的源站项目、完整加载器语义仿真、自动修复/组包。内容锁定及 mrpack 交换已在后续补齐，见[新实例与整合包](packs-and-new-instances.md)。GUI 资源安装和 MCP 写入的公共崩溃恢复已在后续补齐，见[计划与恢复](plans-and-recovery.md)。

## 验证（2026-10-27）

回环 HTTP 测试覆盖真实安装链路的版本回溯、保留已安装依赖、JAR 自声明冲突、同名依赖内容变化及继承版本变化拒绝。核心还覆盖版本表达式、循环、别名、多 Mod JAR、嵌套元数据、禁用文件、规则预算、无效更新保护和恢复备份。MCP 测试验证只读执行及动态撤权；控件与四张截图覆盖 1024×680 和 1536×921。

只读在线测试成功解析 Minecraft 1.21.1 Fabric 的 Iris `zsoi0dso`，选出两个项目，读取四次元数据；未下载或启动游戏。项目自有测试 180 项通过、21 项按条件跳过，工作区编译检查通过；截图与在线测试另行通过。完整记录在 `target/dependency-rules/`。本次使用 Linux tiny-skia，未进行 Windows 原生验证。

```sh
cargo check --workspace --locked --offline
ICED_TEST_BACKEND=tiny-skia cargo test --locked --offline -p desktop -p mc-core -p mc-mcp -- --test-threads=1
cargo test --locked --offline -p mc-core live_modrinth_dependency_graph_is_resolvable -- --ignored --nocapture
MEMORIA_UI_CAPTURE_DIR="$PWD/target/dependency-rules/screenshots" ICED_TEST_BACKEND=tiny-skia cargo test --locked --offline -p desktop capture_dependency_and_rule_matching -- --ignored --test-threads=1
```
