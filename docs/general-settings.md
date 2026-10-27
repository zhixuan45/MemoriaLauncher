# 常规设置与维护

“设置 → 常规”现已使用真实配置服务。修改后点击底部“保存常规设置”；保存成功后应用，重启读取同一份 `general.json`。撤销和恢复默认只改变草稿，保存失败保留原配置及草稿。

支持的常规选项：

- 游戏进程成功启动后最小化启动器。
- 启动器首次扫描完成后最小化到任务栏。
- 启动时检查项目发布记录，默认关闭。
- 关闭按钮或系统关闭请求（如 Alt+F4）改为最小化到任务栏。“退出启动器”始终可以退出。
- 稳定版/包含测试版的更新通道。
- 主页最近打开时间的三种日期格式，时间使用 UTC。

最小化使用任务栏入口；当前没有系统托盘或开机自启服务。界面语言固定为简体中文；地区入口转到已经实现的下载源设置，避免选项保存后不产生实际作用。

## 配置导入导出

“导出常规设置”导出**已保存的**上述开关和日期格式；“导入常规设置”先载入草稿，核对后再保存。支持的 JSON 格式带 `format: memoria-general-settings` 和 `version: 1`，最多 64 KiB；无效格式、版本、未知字段及越界日期格式会被拒绝。

此文件不包含游戏文件、实例、账户、登录应用配置、AI 密钥、MCP 授权或其他设置。完整备份恢复和全数据重置仍待实现，不再提供会暗示这些功能已完成的按钮。

## 数据目录与缓存

“打开数据文件夹”调用系统文件管理器，目录遵循现有 `MEMORIA_DATA_DIR`、Windows LocalAppData 或 Linux 用户数据路径。失败会显示错误。

“清理缓存”需要在页面内确认，仅将启动器数据目录内的 `cache/` 移至 `recovered-cache/<操作 ID>/`。不递归删除文件、不清理 Minecraft 的 assets/libraries、实例安装暂存或备份。缓存入口及恢复目录为符号链接或 Windows 重解析点时拒绝操作；缓存内部的链接随目录移动，不遍历链接目标。没有缓存时如实显示。运行游戏或安装期间不允许确认清理。

这项操作让缓存退出使用，但不会释放磁盘空间。需要恢复时，在启动器退出后将对应目录移回 `cache/`。

## 更新检查

通过 GitHub API 查询 `zhixuan45/MemoriaLauncher` 最近 100 条发布记录，按 SemVer 比较版本，排除草稿和不符合通道的测试版。仅展示新版本并打开该项目发布页，不下载或执行更新程序。切换并保存更新通道会使旧请求失效。

连接超时、HTTP 错误、无效响应和超限响应均显示失败；不会把网络失败解释为“已是最新版”。响应上限为 1 MiB，总请求超时为 20 秒。API 不可访问时，可自行访问项目发布页。

## 验证

验证覆盖实际保存 Task 与重启读取、非法导入与保存失败保留原数据、缓存恢复及链接拒绝、更新请求/HTTP 错误/版本通道/响应限制/过期回复，以及 1024×680、1536×921 两种尺寸下的真实控件点击。离线截图保存在 `target/plan-continuation/screenshots/`。

本轮项目自有测试 151 项通过、18 项按条件跳过；`cargo check --workspace --locked --offline` 通过。日志：`target/plan-continuation/final-tests.log`、`target/plan-continuation/workspace-check.log`；另行运行截图测试通过，生成六张截图。原有 `mc-core::runtime::metadata::Logging.argument` 和测试辅助方法的未使用警告仍保留。

本轮运行环境是 Linux；未执行 Windows 原生任务栏、文件对话框和浏览器冒烟，也未验证线上 GitHub API。回环 HTTP 测试验证的是协议与错误处理。

```sh
cargo check --workspace --locked --offline
ICED_TEST_BACKEND=tiny-skia cargo test --locked --offline -p desktop -p mc-core -p mc-mcp -- --test-threads=1
MEMORIA_UI_CAPTURE_DIR="$PWD/target/plan-continuation/screenshots" \
  ICED_TEST_BACKEND=tiny-skia cargo test --locked --offline -p desktop capture_general_settings -- --ignored --test-threads=1
```
