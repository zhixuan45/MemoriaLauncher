# 实例管理、外观与第三方资源安装

## 实例设置和 Mod 管理

主页的“实例设置”或工具栏“管理实例”打开所选实例，弹窗包含“实例设置”和“Mod 管理”。

实例设置可保存显示名称、独立 Java 路径、最大内存、窗口宽高和 JVM 参数。Java 和内存留空时继承全局设置，窗口宽高同时留空使用游戏默认。每行 JVM 参数作为一个 argv 条目直接传给 Java；含空格的值放在同一行，无需 Shell 引号。内存和类路径由启动器管理。

修改保存到实例内 `.memoria/instance-settings.json`，不改写原版本 JSON。下次启动读取覆盖项；实例显示名称在保存后刷新。

Mod 管理扫描当前实例的 `mods/`，支持名称/文件名筛选、批量导入 JAR、启用、禁用及移除。禁用通过 `.jar` 与 `.jar.disabled` 切换。移除需要在弹窗中确认，文件保留到 `.memoria/removed-mods/`，可手动移回。导入不覆盖同名文件；单文件最多 32 MiB，一次最多 32 个、合计 128 MiB。游戏运行和安装期间禁止修改。

“检查前置与冲突”读取启用的 JAR 并展示版本要求与依赖链；“选择候选 JAR 求解”检查拟新增/替换的组合，只返回建议。支持范围和结果状态见[规则与依赖求解](dependency-resolution-and-rules.md)。

## 外观

设置 → 界面外观支持森林深灰、石墨深灰、午夜蓝底色，独立的自定义主题色或跟随系统强调色、80%–140% 缩放、PNG 背景及背景遮罩。自定义颜色使用 `#RRGGBB`。点击“保存外观”后立即应用，重启继续使用；“撤销更改”恢复已保存值。

Windows 系统颜色通过 [DwmGetColorizationColor](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetcolorizationcolor) 读取，启用跟随后每两秒更新。其他平台使用自定义颜色。按钮、选中项、强调图标和文字使用当前主题色；按钮文字根据颜色亮度调整。

背景仅支持有效 PNG，最多 16 MiB、8192×8192 像素；导入后复制到启动器数据目录，原图移动不影响已保存背景。外观配置在启动器数据目录的 `appearance.json`，支持 `MEMORIA_DATA_DIR` 覆盖。

## 第三方资源安装

1. 在游戏下载中选择 Mod、整合包、资源包或光影包，搜索并打开项目。
2. 在详情底部选择具体版本，点击“安装此资源版本”。
3. 选择目标实例，点击“生成安装预览”。Mod 默认选择最近启动实例（包含仍配置的其他实例集），不存在时回退到主页选择；可手动切换。预览固定资源版本、目标实例、必要依赖和文件清单；解析失败显示具体原因。
4. 核对文件及覆盖提示后点击“确认安装”，查看下载校验进度和完成或失败结果。安装后自动刷新本地实例和 Mod 数量。

Mod、资源包、光影分别部署到实例内的 `mods/`、`resourcepacks/`、`shaderpacks/`。Mod 必需依赖使用回溯选版，同时保留其他已安装来源项目；预览下载并校验候选 JAR，与保留文件一起检查前置和冲突。可选依赖提示后由用户另选。未知来源文件可检查 Mod ID 声明，但不会猜测源站项目身份或自动升级。

整合包按 [Modrinth mrpack 格式](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack) 解析，读取清单下载文件并校验哈希，跳过仅服务端文件，可选客户端文件由复选框控制；`client-overrides` 在共享 `overrides` 之后应用。目标实例必须已安装清单要求的 Minecraft 与相同版本加载器。建议准备独立目标实例；此资源部署流程不自动运行加载器安装器；可先通过专用的[加载器安装入口](loaders-and-diagnostics.md)创建匹配的独立实例。

远程单文件最多 64 MiB、单次计划合计最多 1 GiB；Mod JAR 同时受结构检查的 32 MiB 上限限制。整合包覆盖文件仅允许游戏内容目录（如 `config/`、`mods/`、`resourcepacks/`、`kubejs/` 等），拒绝目录穿越、链接和启动器元数据路径。

全部文件先下载暂存并校验 SHA-1，源站提供 SHA-512 时同时校验。提交前复查目标文件、全部启用依赖 JAR 内容和父版本元数据链；发生变化则要求重新预览。替换保留备份，普通提交失败回滚；若外部程序同时修改文件，保留备份并显示需要手动恢复的路径。

新增实例数据布局：

```text
实例目录/
  .memoria/
    instance-settings.json
    removed-mods/
    resource-install.json
    resource-installs/<操作 ID>/
      plan.json            # 文件路径与编号备份的对应关系
      stage/
      backup/
    mcp/write.lock         # 与 MCP 写入共用实例锁
```

旧 `.launcher/` 数据保持原样；已有游戏日志、原生库及 MCP 历史记录继续使用各自原路径。GUI 安装不改变外部 MCP 客户端的权限。

## 验证（2026-10-27）

- Rust 1.99.1：`cargo test --locked --offline -p desktop -p mc-core -p mc-mcp -- --test-threads=1`，144 项通过、17 项按条件跳过。完整日志：`target/instance-features/final-tests.log`。
- Windows 原生冒烟：10 个场景通过，使用隔离的磁盘实例、110% 缩放、午夜蓝和当前系统强调色；截图：`target/instance-features/native/`。
- 两种窗口尺寸（1024×680、1536×921）的实例设置、Mod 管理、外观、安装向导控件检查和截图通过；截图：`target/instance-features/final-screenshots/`。
- 实际 GUI 控件联网安装 Fabric API：项目 `P7dR8mSH`、版本 `Mys3P7lK`（`0.116.17+1.21.1`），成功完成预览、确认、CDN 下载、哈希验证和 JAR 写入，生成 `.memoria` 来源记录。测试使用隔离实例并在结束后清理该实例；截图：`target/instance-features/live-screenshots/`。
- 回环 HTTP 覆盖 Mod 必需依赖、资源包、光影、mrpack 覆盖文件、过期快照、校验失败及提交中断回滚。此次未启动实际 Minecraft 游戏，也未更改 Windows 系统主题设置。
