# 页面检查记录（2026-10-26）

以下为首次检查时的历史记录。2026-10-27 后续开发已补全实例设置、Mod 管理、外观及第三方资源 GUI 安装；当前用法和限制见[实例管理说明](instance-management.md)。续做已接通常规设置保存、窗口行为、常规导入导出、数据目录、可恢复缓存移动及版本检查，见[常规设置说明](general-settings.md)。环境配置复用、高级设置占位、完整备份/重置等其余缺项仍保留。下方表格记录首次检查状态，不代表当前状态。

根目录已将 `apps/desktop` 设为 workspace 默认成员，直接运行 `cargo run` 启动桌面客户端。MCP 服务仍使用 `cargo run -p mc-mcp`；整个工作区的构建检查使用 `--workspace`。

## 本次实测

| 检查 | 结果 |
| --- | --- |
| Cargo 默认成员 | `cargo metadata` 返回 `apps/desktop` |
| desktop 自动测试 | 73 通过，0 失败，11 跳过 |
| 完整页面截图检查 | 单独运行 `capture_pages`，1 通过；输出 42 张截图 |
| 原生 Windows 桌面冒烟 | 不指定 `-p` 的 `cargo run --locked --offline -- --ui-smoke target/page-audit/native` 成功；8 个场景全部通过 |
| 差异检查 | `git diff --check` 通过 |

截图检查在 1024×680、1536×921 两种逻辑窗口尺寸下，渲染四个主页面，点击八个设置页签、五个下载分类、四个版本详情页签，并断言控件发出对应的路由消息。截图使用 tiny-skia，实例和版本为测试数据，第三方资源分类注入空结果；已有资源控件测试另外覆盖非空搜索结果、项目选择和分页。原生冒烟使用隔离配置，无已安装实例。

全部截图位于 `target/page-audit/screenshots/`，原生截图位于 `target/page-audit/native/`。抽查了原生主页、最小尺寸游戏设置、外观、高级设置、资源分类等截图。此次没有执行真实 Microsoft 登录、游戏启动或资源下载安装。

首次运行遇到构建脚本 `0xc0000142`；在正常 Windows 执行环境重跑后可构建。首次全量测试另有 4 项因临时目录访问受限失败；将 `TEMP`、`TMP` 指向工作区专用目录后，4 项全部通过，没有修改业务代码。

## 已有页面

主页、AI 对话、游戏下载、设置四个主页面均存在，主导航控件测试通过。

| 设置页签 | 当前状态 |
| --- | --- |
| 常规 | 有界面；启动行为、语言、地区、日期和测试版更新选项仅保留会话状态 |
| 游戏设置 | 有界面与持久化；实例集、Java、内存配置已接入 |
| 下载安装 | 有界面与持久化；下载源和并发配置已接入 |
| 环境配置 | 直接复用游戏设置视图，尚无独立页面内容 |
| AI 设置 | 模型连接与 MCP 服务器配置已接入 |
| 界面外观 | 占位页；只有介绍和返回按钮 |
| 高级设置 | 占位页；只有介绍和返回按钮 |
| 账户登录 | 客户端 ID、回调地址和保存配置界面已接入 |

游戏下载中的游戏版本、Mod、整合包、资源包、光影包入口均存在；游戏版本的介绍、文件、配置要求和相关资源页签可点击。第三方四类资源目前提供目录和版本文件预览。

## 当前入口内的缺项

| 入口 | 缺少的页面内容或行为 | 源码依据 |
| --- | --- | --- |
| 设置 → 界面外观 | 壁纸、主题、界面缩放的实际控件及保存 | `apps/desktop/src/settings.rs` 的占位分支；截图 `settings-5-*` |
| 设置 → 高级设置 | 日志、诊断、实验功能的实际控件及保存 | 同上；截图 `settings-6-*` |
| 设置 → 环境配置 | 独立的环境配置内容，目前与游戏设置完全相同 | `settings.rs` 将页签 1、3 路由到同一 `game_settings::view` |
| 主页 → 实例设置 | 实例独立配置编辑和保存；当前弹窗只展示版本、加载器、路径 | `apps/desktop/src/shell.rs` 的 `Modal::Instance` |
| 下载 → 第三方资源详情 | GUI 安装、目标实例选择、依赖部署；加载器安装也未接入桌面流程 | `apps/desktop/src/resources.rs` 详情提示及 `docs/development-progress.md` |
| 设置 → 常规 | 通用选项持久化及实际启动/托盘/语言行为 | `settings.rs` 的会话内状态和预览提示 |
| 设置 → 常规 → 数据及维护 | 备份恢复、打开数据目录、缓存管理、检查更新、设置导入导出、完全重置 | `settings.rs` 对应按钮仅发送未接入提示 |
| 编辑布局 → 下载任务 | 暂停、恢复及统一任务管理 | `apps/desktop/src/app.rs` 的 `ToggleTask`、`ToggleAllTasks` 只更新提示 |

这里的 GUI 安装缺项不包含已实现的外部 MCP 受限写入能力；MCP 权限配置页已有独立 Mod 来源/JAR、配置和 FTB 写入权限控件。

## 后续规划中尚无完整 GUI 的工作流

结合当前路由、[开发进度](development-progress.md)及[原始规格](MC_AI_Launcher_Codex_Spec.md)，以下工作流仍需界面与服务闭环；不要求恢复原始规格里的旧导航布局：

- 实例内已安装 Mod 管理、依赖冲突分析。
- 整合包导入导出、锁定状态、快照与恢复。
- 汉化扫描、覆盖率、翻译预览与独立资源包生成。
- 统一任务与日志查看；目前只有下载进度、日志路径和 AI 日志附件等局部入口。
- 逐计划预览、GUI 审批、执行结果与操作历史。
- AI 自动组包与汉化执行；目前内置 AI 对话不执行本地工具。

## 复测命令（PowerShell）

```powershell
$auditRoot = Join-Path $PWD 'target/page-audit'
New-Item -ItemType Directory -Force "$auditRoot/temp", "$auditRoot/data", "$auditRoot/screenshots" | Out-Null
$env:TEMP = "$auditRoot/temp"
$env:TMP = $env:TEMP
$env:MEMORIA_DATA_DIR = "$auditRoot/data"
$env:ICED_TEST_BACKEND = 'tiny-skia'
$env:MEMORIA_UI_CAPTURE_DIR = "$auditRoot/screenshots"
cargo test --locked --offline -p desktop -- --test-threads=1
cargo test --locked --offline -p desktop capture_pages -- --ignored --test-threads=1
cargo run --locked --offline -- --ui-smoke target/page-audit/native
```
