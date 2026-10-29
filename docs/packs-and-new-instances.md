# 锁定文件、整合包与独立加载器实例

## 新建带加载器的实例

打开“游戏下载 → 新建实例 / 导入整合包”，输入新实例 ID 和 Minecraft 版本，选择 Fabric、Quilt、Forge 或 NeoForge，查询并选择加载器版本，指定 Java，再生成预览并确认安装。也可选择原版。游戏下载页已有选中的官方版本时，向导会优先使用该版本。

无需事先安装原版。预览只读取官方元数据、资源索引、加载器 profile 或安装器清单，固定版本与声明哈希，不创建安装目录。确认后先检查 Java，再在暂存实例集中安装原版、加载器及整合包内容，完成检查后发布。Forge/NeoForge 确认后运行已校验的官方 Java 安装器；暂存目录不是操作系统沙箱。

目标实例 ID 使用英文、数字和 `._-+`，最长 160 字节，不允许连续点；不能与共享原版 ID 相同。同名目录拒绝覆盖。完成后刷新实例列表，并将主页启动选择保存为新实例；AI 项目选择与最近启动记录维持各自的含义。

原有“安装加载器”入口继续支持基于已安装原版创建独立实例。四类加载器的历史版本与官方源支持范围仍见[加载器说明](loaders-and-diagnostics.md)。

## 整合包导入

本轮支持 Modrinth `.mrpack`：

- 本地文件：在新建向导点击“导入 .mrpack”，自动读取 Minecraft 和加载器要求，再选择新实例 ID、Java 和是否包含可选客户端文件。
- Modrinth 目录：选择整合包版本，点击“安装此资源版本”，再选“新建整合包实例（自动安装游戏与加载器）”。读取并校验源站文件后进入同一向导。
- 已有实例：原资源安装向导仍可部署到匹配的目标。不会自动清除额外文件，依赖和冲突检查失败时要求处理或改用新实例。

清单只允许一个受支持的加载器；未知运行依赖、多个加载器、不一致版本、危险路径、链接、重复/大小写冲突路径和无效客户端环境声明均会拒绝。跳过仅服务端文件；可选客户端文件由复选框控制。`client-overrides` 按标准覆盖同名的共享 `overrides`。下载校验 SHA-1，有 SHA-512 时同时校验；Mod JAR 继续做结构及静态前置/冲突检查，未知约束明确提示。

导入本轮导出的包时仍按实际清单、下载哈希和覆盖文件检查，不使用包内的 `memoria.lock.json` 绕过验证；新实例会重新生成自己的锁定记录。Forge 清单使用短版本（如 `47.4.0`），调用官方目录时自动补上 Minecraft 前缀。

## 锁定与验证

“管理实例 → 锁定与导出”提供“生成或刷新锁定文件”和“验证现有锁定”。记录保存到实例的 `.memoria/instance-lock.json`，格式为 `memoria-instance-lock`、版本 1。记录包含：

- Minecraft 与加载器的准确版本，以及继承链 JSON 的摘要。
- 内容文件的相对路径、大小、SHA-1、SHA-256、SHA-512。
- 与实际字节匹配时可复用的来源项目、版本 ID 和下载地址。

覆盖 `mods/`（包括禁用 JAR）、`resourcepacks/`、`shaderpacks/`，以及生成锁定记录时的配置、脚本和 `options.txt` / `servers.dat`。不记录账户令牌、存档、截图、日志或启动器备份目录。校验返回缺失、内容变化、额外文件和版本声明变化，不自动修复。

新建实例以及 GUI 资源安装会自动生成或更新锁定文件；资源内容、来源记录和锁定文件共同进入已有文件事务。已有实例可手动生成。手动编辑或 MCP 写入之后可能出现漂移，应验证并按需重新生成。

锁定记录描述版本声明和内容目录，未逐项重算共享 `libraries/`、`assets/` 和客户端 JAR；这些文件的完整性仍由启动检查负责。锁定一致也不证明任意 Mixin 或运行期行为兼容。

## 导出

同一页面填写整合包名称，按需勾选“包含配置与脚本”，生成预览后选择一个新的 `.mrpack` 文件名。默认不包含配置；勾选时还会包含 `options.txt` 和 `servers.dat`，应检查私人设置。Java 路径、JVM/窗口覆盖设置、自定义版本 JSON、用户账户和存档均不随包导出。

有可信来源记录、内容哈希一致且 URL 可复用的文件导出为标准下载引用；其他文件（包括本地或已修改的 JAR）放入 `overrides/`，预览列出对应提示。带查询参数的来源地址不会复用。导出包含标准 `modrinth.index.json` 和补充的 `memoria.lock.json`，可通过本轮导入流程恢复内容文件。

确认导出前重新校验快照；内容已变化则拒绝并要求重新预览。已有文件不覆盖。导出只在本机生成文件，不会发布到源站。

## 独立 Core 安装接口

公共 Rust API 位于 `mc_core::service::packs`，桌面与其他可信本地客户端均可调用，无需整合包或已安装基础实例：

```rust
use mc_core::{DownloadClient, ProgressCallback, runtime::loaders::Kind};
use mc_core::service::packs::{
    Loader, NewInstanceRequest, preview_new_instance, install_new_instance,
};
use std::{path::PathBuf, sync::Arc};

async fn install_fabric(
    client: DownloadClient, root: PathBuf, java: PathBuf,
) -> Result<String, String> {
    let request = NewInstanceRequest {
        root,
        target: "my-fabric-world".into(),
        minecraft: "1.20.1".into(),
        loader: Some(Loader { kind: Kind::Fabric, version: "0.16.10".into() }),
        java,
        pack: None,
        optional: false,
    };
    let plan = preview_new_instance(request, client.clone()).await?;
    // 调用方展示 plan 的目标、版本、Java 要求、文件数量、体积和警告，并取得本次确认。
    let progress: ProgressCallback = Arc::new(|_| {});
    install_new_instance(plan, client, progress).await
}
```

`NewInstancePlan` 的字段通过只读方法访问，编辑输入后应重新预览。`loader: None` 创建独立原版实例。通过 `exchange::read_pack` / `parse` 读取包后，将其 `minecraft`、`loader` 以及 `pack: Some(pack)` 放入同一个请求即可自动准备环境。

其他接口包括 `lockfile::{capture, save, read, verify}`、`exchange::{preview_export, export}`，以及针对已有实例的 `resource_install::prepare_pack`。`runtime::{preview_vanilla, install_vanilla_plan}` 也可单独复用。当前新增的是 Rust Core API 和桌面入口，MCP 工具集继续遵守原有类别权限与实例名单。

## 发布与边界

暂存记录保存在实例集 `.memoria/instance-installs/<操作 UUID>/`，包含 `plan.json`、`result.json` 和 `work/`。安装只复用经过声明哈希检查的缓存；发布共享文件时，已有内容不同则停止，绝不覆盖。新实例的内容、配置与锁定记录先发布，版本 JSON 最后发布，避免展示可启动的半成品。

普通失败或进程中断可能留下暂存、已验证共享缓存、原版基础版本或没有版本 JSON 的预留目录；这些保留供检查，不会覆盖旧实例。新建实例的跨目录发布尚未全部迁入公共自动恢复事务，不声称断电原子性或自动续传。发布及无覆盖导出需要同一文件系统支持硬链接。

限制：包压缩文件 64 MiB、清单 4 MiB、最多 4094 个内容文件；单个内容文件 64 MiB、内容总量 1 GiB。锁定记录最多 4096 项、4 MiB；本地导出覆盖文件总量最多 128 MiB，最终压缩包仍最多 64 MiB。共享依赖发布预算为 20 万文件、8 GiB，单文件 128 MiB。来源下载沿用现有主机白名单，不支持任意下载地址、所有整合包格式或服务端部署。

## 验证（2026-10-29）

204 项项目自有测试通过、24 项按条件跳过；工作区编译检查通过。回环 HTTP 与受控 Java 覆盖四类加载器从空集合创建实例，扫描和启动前依赖检查，整合包导出再导入的文件哈希一致、客户端覆盖优先级、禁用 Mod、来源引用、共享缓存复用、锁定漂移、危险路径及旧文件保护。

桌面控件运行实际的新建、导入、锁定验证和导出任务；两种窗口尺寸检查加载器及导出控件，另行生成四张截图。记录位于 `target/packs/`。本轮为 Linux tiny-skia；未验证真实官方 Forge/NeoForge 安装器、实际 Minecraft 启动或 Windows 原生流程。
