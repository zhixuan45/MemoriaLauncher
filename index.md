# 代码库架构与接口索引 (Codebase Index)
# 纯净扁平 YAML 格式。第一级快速路由请仅读取前 35 行；第二级通过 tag 定向检索模块代码块。

index:
  project: "MC AI Launcher"
  tech_stack:
    - "Rust 1.97+ (Workspace)"
    - "Slint 1.18 (Native GUI)"
    - "Tokio 1.40 (Async Runtime)"
    - "Serde / Thiserror / Tracing"
  entrypoints:
    - "apps/desktop/src/main.rs"
    - "crates/mc-core/src/lib.rs"
  summary: "Minecraft Java 版原生启动器与 AI 平台。Rust Core 统一承载实例管理、依赖求解与任务事务；Slint 原生客户端与后续 REST/MCP 统一调用该核心。"
  modules_map:
    - id: "core"
      tag: "@core"
      path: "crates/mc-core"
      summary: "核心领域实体、服务抽象契约、任务追踪调度与统一语义化错误"
    - id: "desktop"
      tag: "@desktop"
      path: "apps/desktop"
      summary: "Slint 原生桌面客户端、声明式 UI 布局与核心服务事件绑定"
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
      path: "crates/mc-core/src/service/auth.rs"
      summary: "Minecraft 正版认证接口规范与系统凭据安全隔离机制"

# ==============================================================================
# 模块详情与核心代码块直达清单 (按需定向查阅，禁止全量盲目读取)
# ==============================================================================

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
        desc: "游戏版本元数据与模组项目摘要"
        symbols:
          - "struct GameVersion - 官方版本条目 (id, version_type, release_time, url)"
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
      - file: "crates/mc-core/src/service/catalog.rs"
        desc: "内容与版本目录提供者契约及离线 Mock"
        symbols:
          - "trait CatalogProvider - 抽象目录服务 (list_game_versions, search_mods)"
          - "struct MockCatalogProvider - 阶段 0 离线测试模拟提供者"
      - file: "crates/mc-core/src/service/task.rs"
        desc: "异步任务调度追踪服务契约与内存实现"
        symbols:
          - "trait TaskService - 抽象任务服务 (get_task, list_tasks, cancel_task)"
          - "struct InMemoryTaskService - 内存任务管理器，支持 register_task()"

  - id: "desktop"
    tag: "@desktop"
    path: "apps/desktop"
    summary: "Slint 原生桌面客户端、声明式 UI 布局与核心服务事件绑定"
    files:
      - file: "apps/desktop/ui/app.slint"
        desc: "Slint 原生声明式窗口布局文件"
        symbols:
          - "component MainWindow - 主桌面窗口，暴露 instances 列表模型、status_text、refresh_clicked、create_instance_clicked"
          - "struct InstanceItem - 界面实例数据绑定项 (id, name, mc_version, loader)"
      - file: "apps/desktop/src/main.rs"
        desc: "桌面客户端主入口与原生事件循环"
        symbols:
          - "main() -> Result<(), Box<dyn std::error::Error>> - 初始化 Tokio 运行时、绑定 MockInstanceService 并启动 Slint 窗口"

# ==============================================================================
# 核心业务调用链速查 (单行文本链式表达，严禁画流程图)
# ==============================================================================

workflows:
  - name: "sync_instances_flow"
    chain: "MainWindow.refresh_clicked -> InstanceService.list_instances -> VecModel 转换 -> MainWindow.set_instances -> 更新状态标签"
  - name: "create_instance_flow"
    chain: "MainWindow.create_instance_clicked -> InstanceService.create_instance -> tokio::spawn 异步执行 -> 触发列表刷新链路"
