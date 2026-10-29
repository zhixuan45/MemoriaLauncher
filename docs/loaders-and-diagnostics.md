# 加载器安装与本地日志解析

## 加载器安装

主页或游戏下载页顶部的“安装加载器”提供 Fabric、Quilt、Forge 和 NeoForge。先安装完整的独立原版客户端，再选择基础实例与加载器，查询兼容版本，选择 Java，生成预览并确认安装。完成后实例列表自动刷新。

新实例 ID 为 `memoria-<加载器>-<加载器版本>-<Minecraft>`，使用独立游戏目录及保存的 Java 覆盖项，原版依赖继续共享。同名目录拒绝覆盖，不将已有实例就地转换，也不复制原实例的存档、Mod 或设置。

Fabric/Quilt 使用按 Minecraft 版本过滤的官方 Meta API，验证 profile 父版本与加载器坐标，固定库文件 SHA-1 后下载。缺少有效哈希、路径不安全或不受支持的库规则会停止安装。单库最多 128 MiB，直接下载合计最多 1 GiB。

现代 Forge/NeoForge 使用官方 Maven 目录和安装器。预览阶段校验安装器 SHA-1、读取 `version.json` 并展示库清单；确认后重新校验，以所选 Java 执行 `-jar <安装器> --installClient <暂存目录>`。安装器可能自行联网下载、处理依赖。暂存目录隔离本次文件布局，**不构成操作系统沙箱**。主进程超过 10 分钟终止；输出限量保留并遮蔽常见凭据行。

只有安装器输出与预览 profile 一致、所需库存在且通过已声明的哈希/大小检查，才发布实例。生成库的空哈希、零大小占位换成实际产物值，供后续启动检查使用；另外记录实际 SHA-256。提交前复查基础 JSON/JAR 快照；持有集合锁和基础实例写锁，GUI 禁止并发启动或修改实例。已有共享库内容不同则停止，绝不覆盖。发布使用临时文件与不覆盖目标的硬链接，需要文件系统支持硬链接；实例 JSON 最后写入。失败不会发布可启动的新 profile，已新增的校验库可能作为缓存保留。

记录保留在游戏根目录 `.memoria/loader-installs/<操作 ID>/`，包含 `work/`、`manifest.json`、`result.json`，以及 Forge/NeoForge 的 `installer.log`。

边界：旧式 Forge 安装器缺少 `version.json` 时拒绝安装；不猜测 NeoForge 快照或旧版特殊坐标。Java 主版本遵循实例元数据，未声明时由启动检查负责。加载器使用各自官方源，现有下载源继续处理 Mojang 文件。不自动安装 Fabric API 等 Mod。后续新增的[新实例与整合包流程](packs-and-new-instances.md)可从空集合安装原版和加载器，供用户单独使用或由 mrpack 清单驱动；原有基于已安装原版的入口仍保留。

## 通用日志自动解析

“日志诊断”浏览当前实例的启动器日志、`latest.log`、`debug.log`、Minecraft 崩溃报告及 JVM `hs_err` 日志，选择后自动本地解析；也可导入普通 `.log` / `.txt` 文件。没有实例时仍可导入。

游戏退出后自动解析**退出实例本次启动的确切日志**，不使用当前选中实例或历史报告冒充本次证据。异常退出弹窗展示摘要；正常退出也保留本次诊断。解析失败显示错误，原退出状态仍保留。

类别包括依赖缺失、加载器/Mod 版本不兼容、Java 版本错误、内存不足、哈希错误、Mixin、类加载、图形环境、网络/证书、JVM 原生崩溃及非零退出码。结果包含严重级别、置信度、证据和建议；常见 Fabric 依赖格式还返回 Mod/依赖 ID 和版本要求。未匹配显示“未识别到已知异常模式”，不宣称运行正常。Mixin、类加载冲突需人工确认。

规则现已独立为 19 条声明式内置规则，支持上下文、排除、命名字段提取、自定义 JSON 导入和恢复内置。GUI 与 MCP 使用相同配置；格式、预算和新增前置/冲突求解见[详细说明](dependency-resolution-and-rules.md)。

规则解析不调用 AI、不上传日志、不执行日志中的指令或自动应用修复。原有“启用 AI 分析 → 预览附件 → 用户发送”流程保留。

实例日志限尾部 64 KiB，导入文件限尾部 1 MiB；截断时丢弃不完整首行。遮蔽常见凭据整行和常见本地绝对路径，每类最多三段证据、每段最多 500 字符。压缩/二进制文件需先解压或转成文本，遮蔽不保证覆盖所有自定义敏感格式。

MCP 新增 `minecraft_analyze_logs`，沿用日志权限与实例白名单，不增加写入或运行权限：

```json
{"name":"minecraft_analyze_logs","arguments":{"instance_id":"versions/1.21.1","path":"logs/latest.log"}}
```

结果位于 `analysis.findings` 等字段。MCP 客户端如何使用返回结果由客户端控制。

## 验证

回环 HTTP 覆盖四类目录、profile 兼容性、哈希错误、过期快照和共享库冲突。受控 Java 进程验证安装器参数、暂存输出、失败及日志遮蔽；产物经过真实扫描、依赖检查及启动 argv 构造。日志测试覆盖分类、未知/干净/超限日志、导入、遮蔽、链接及路径拒绝；控件和截图覆盖 1024×680、1536×921。

本轮 `cargo check --workspace --locked --offline` 通过，项目自有测试 162 项通过、19 项按条件跳过；另行运行截图测试生成四张图片。日志为 `target/loader-diagnostics/final-tests.log`、`workspace-check.log`、`screenshots.log`，截图位于该目录的 `screenshots/`。

```sh
cargo check --workspace --locked --offline
ICED_TEST_BACKEND=tiny-skia cargo test --locked --offline -p desktop -p mc-core -p mc-mcp -- --test-threads=1
MEMORIA_UI_CAPTURE_DIR="$PWD/target/loader-diagnostics/screenshots" \
  ICED_TEST_BACKEND=tiny-skia cargo test --locked --offline -p desktop capture_loader_and_diagnostics -- --ignored --test-threads=1
```

日志和截图位于 `target/loader-diagnostics/`。本轮为 Linux 环境，加载器官方域名不在环境网络允许列表中，未验证官方源完整在线安装、实际 Minecraft 启动或 Windows 原生冒烟。受控安装器测试不等同于官方安装器端到端验证。
