# 代码库架构与接口索引 (Codebase Index)
# 纯净扁平 YAML 格式。第一级快速路由请仅读取前 35 行；第二级通过 tag 定向检索模块代码块。

index:
  project: "Memoria Launcher"
  tech_stack:
    - "Rust 1.97+ (Workspace)"
    - "Slint 1.9 (Declarative Native GUI)"
    - "Tokio 1.40 (Async Runtime)"
    - "Serde / Thiserror / Tracing"
  entrypoints:
    - "apps/desktop/src/main.rs"
    - "crates/mc-core/src/lib.rs"
  summary: "Minecraft Java 版原生启动器与 AI 平台。Rust Core 统一承载实例管理、依赖求解与任务事务；桌面客户端与后续 REST/MCP 统一调用该核心。"
  modules_map:
    - id: "core"
      tag: "@core"
      path: "crates/mc-core"
      summary: "核心领域实体、服务契约、客户端扫描、共享依赖启动适配与统一错误"
    - id: "desktop"
      tag: "@desktop"
      path: "apps/desktop"
      summary: "Slint 原生桌面客户端、16px 吸附自由画布与核心服务模型事件绑定"
    - id: "installer"
      tag: "@installer"
      path: "crates/mc-installer"
      summary: "Mojang 官方版本清单解析、资源下载校验与 Loader 安装器（阶段 1）"
    - id: "mods"
      tag: "@mods"
      path: "crates/mc-mods"
      summary: "Modrinth 检索、依赖关系图分析与变更安装计划生成（阶段 1）"
    - id: "auth"
      tag: "@auth"
      path: "crates/mc-core/src/runtime/auth.rs"
      summary: "Microsoft PKCE 登录、Xbox/XSTS/Minecraft 交换与凭据存储抽象"

# 模块详情与核心代码块直达清单 (按需定向查阅，禁止全量盲目读取)

modules:
  - id: "core"
    tag: "@core"
    path: "crates/mc-core"
    summary: "核心领域实体、服务抽象契约、任务追踪调度与统一语义化错误"
    files:
      - file: "crates/mc-core/src/error.rs"
        desc: "统一语义化错误枚举与通用 Result 别名"
        symbols:
          - "enum LauncherError - 包含 InvalidVersion, InstanceNotFound, DependencyConflict, DownloadHashMismatch, AuthorizationRequired, PlanStale, AuthUnavailable 等错误码"
          - "type LauncherResult<T> = Result<T, LauncherError> - 领域层统一结果类型"
      - file: "crates/mc-core/src/model/instance.rs"
        desc: "游戏实例与模组加载器领域实体定义"
        symbols:
          - "struct Instance - 实例基础信息 (id, name, mc_version, loader, game_dir, created_at, last_played_at)"
          - "struct CreateInstanceRequest - 实例新建请求参数 (name, mc_version, loader)"
          - "enum LoaderType - 加载器分类 (Vanilla, Fabric, NeoForge, Forge, Quilt)"
      - file: "crates/mc-core/src/model/task.rs"
        desc: "异步任务生命周期与进度实体"
        symbols:
          - "struct Task - 统一任务状态对象 (id, task_type, status, progress, error, created_at, updated_at)"
          - "enum TaskStatus - 任务状态 (Pending, Running, Succeeded, Failed, Cancelled)"
          - "enum TaskType - 任务类别 (DownloadVersion, InstallLoader, InstallMod, Localize, Launch)"
      - file: "crates/mc-core/src/model/catalog.rs"
        desc: "游戏版本完整元数据与模组项目摘要"
        symbols:
          - "struct GameVersion - 官方版本完整元数据 (id, title, version_type, release_time, release_date, java_version, supported_loaders, summary, highlights, version_files, system_requirements, related_resources, changelog_url, url)"
          - "struct ModProject - 模组项目摘要 (id, slug, title, description, loaders, game_versions)"
      - file: "crates/mc-core/src/model/plan.rs"
        desc: "安装与变更计划实体（先计划后执行原则）"
        symbols:
          - "struct InstallPlan - 安装计划 (plan_id, instance_id, operations, risk_warnings, approved, plan_hash)"
          - "struct PlanOperation - 单项变更操作 (action, item_name, version, reason)"
      - file: "crates/mc-core/src/service/instance.rs"
        desc: "实例管理服务契约与内存 Mock 实现"
        symbols:
          - "trait InstanceService - 抽象服务接口 (list_instances, get_instance, create_instance)"
          - "struct MockInstanceService - 内存实例服务，支持 with_sample_data()"
      - file: "crates/mc-core/src/runtime/"
        desc: "本地 Minecraft 版本扫描、共享 libraries/assets 校验与隔离目录 Java 启动后端"
        symbols:
          - "LocalClientService.scan() - 扫描 versions/ 并兼容 version/，读取父版本继承链和加载器信息"
          - "GameLauncher.check() / prepare() - 校验 Java 与本地依赖，使用 MIT mc-launcher-core 生成标准 versions 启动参数"
          - "GameLayout - 共享根目录下的 versions、libraries、assets 布局；游戏目录按版本隔离"
          - "MicrosoftAuthProvider - PKCE 回调、Microsoft→Xbox Live→XSTS→Minecraft 登录与 refresh token 更新"
          - "CredentialStore / MemoryCredentialStore - 令牌存储接口与测试用内存实现；生产环境需接入系统安全存储"
          - "THIRD_PARTY_NOTICES.md - mc-launcher-core MIT 许可与版权声明"
      - file: "crates/mc-core/src/service/catalog.rs"
        desc: "内容与版本目录提供者契约及完整模拟版本库实现"
        symbols:
          - "trait CatalogProvider - 抽象目录服务 (list_game_versions, get_game_version, search_mods)"
          - "struct MockCatalogProvider - 模拟目录提供者，内置 1.21 至 1.12.2 及快照版 24w14a 的完整版本介绍、文件清单、配置要求与关联资源数据"
      - file: "crates/mc-core/src/service/task.rs"
        desc: "异步任务调度追踪服务契约与内存实现"
        symbols:
          - "trait TaskService - 抽象任务服务 (get_task, list_tasks, cancel_task)"
          - "struct InMemoryTaskService - 内存任务管理器，支持 register_task() 与 with_sample_data()"

  - id: "desktop"
    tag: "@desktop"
    path: "apps/desktop"
    summary: "Slint 原生桌面客户端、16px 吸附自由画布、下载/AI/设置视图与事件模型绑定"
    files:
      - file: "apps/desktop/src/main.rs"
        desc: "桌面端主入口、Tokio 运行时、Slint 模型注入与全事件回调绑定"
        symbols:
          - "main() -> Result<(), Box<dyn std::error::Error>> - 启动事件循环并挂载服务"
          - "fn extract_head_from_skin_rgba() -> Option<Image> - 正版皮肤头部合成管线"
          - "fn build_player_profiles() / fn build_sample_instances() - 样本数据构建"
      - file: "apps/desktop/src/ui_models.rs"
        desc: "核心领域模型到 Slint 界面展示条目的转换映射"
        symbols:
          - "fn map_instance_to_entry() / fn map_task_to_entry() / fn map_game_version_to_entry()"
      - file: "apps/desktop/ui/app.slint"
        desc: "顶层应用窗口骨架、无边框外壳、自由画布与多视图切换路由"
        symbols:
          - "component MainWindow inherits Window - 顶层主窗口组件"
      - file: "apps/desktop/ui/common.slint"
        desc: "通用毛玻璃卡片、纯矢量 SVG 图标、玩家头像与数据结构定义"
      - file: "apps/desktop/ui/widgets.slint"
        desc: "自由拖拽缩放容器、网格吸附引擎、侧边栏导航与启动面板"
      - file: "apps/desktop/ui/cards.slint"
        desc: "实例列表组件、版本更新日志卡片与后台任务队列卡片"
      - file: "apps/desktop/ui/download.slint"
        desc: "游戏下载中心视图、5 大分类卡片、海报轮播与 4 页签详情面板"
      - file: "apps/desktop/ui/ai.slint"
        desc: "AI 功能交互视图、需求输入卡片、2x4 快捷提问磁贴与任务队列"
      - file: "apps/desktop/ui/settings.slint"
        desc: "系统设置视图、7 个二级 Tab 与 5 大毛玻璃配置卡片"
      - file: "docs/ai_dev_guide.md"
        desc: "面向后续 AI 代理的架构、UI 组件复用与交互开发规范文档"
        symbols:
          - "UI 模块分层与 common.slint 原子组件复用准则"
          - "无 Emoji 纯矢量图标规范、Flickable 滚轮转发与玩家头像双模渲染规范"

# 核心业务调用链速查 (单行文本链式表达，严禁画流程图)
workflows:
  - name: "select_and_reorder_instance_flow"
    chain: "InstanceListCard 拖拽或点击 -> MainWindow.move_instance_clicked / select_instance_clicked -> VecModel 重排或更新 current_instance -> 同步 LaunchPanelCard 与 InstanceStatsCard"
  - name: "task_queue_sync_flow"
    chain: "InMemoryTaskService.list_tasks -> map_task_to_entry -> MainWindow.set_tasks -> TaskQueueCard 暂停/继续触发 TaskService.cancel_task"
  - name: "ai_task_dispatch_flow"
    chain: "AiView 输入或选择快捷提问/实用示例 -> MainWindow.submit_ai_prompt_clicked -> VecModel 插入分步 AiTaskEntry -> 右侧任务列表支持暂停/恢复与移除"
  - name: "widget_grid_snap_flow"
    chain: "顶部工具栏切换 edit_mode -> DraggableWidget 按 16px 隐形网格自动吸附更新 pos_x/pos_y -> 可删模块触发 delete_clicked 隐藏 -> reset_layout() 一键恢复"
