# MC AI Launcher 项目索引

本项目是一个面向 Minecraft Java Edition 的模块化桌面启动器与 AI 整合包管理平台。系统设计支持普通玩家通过本地原生界面管理游戏，同时开放本地 API 与 MCP Server 供外部 AI Agent 自主进行版本构建、Mod 依赖求解、汉化与运行诊断。

## 模块划分与代码组织

工作区采用 Rust Workspace 管理，各子模块职责边界清晰：

- `crates/mc-core`：核心业务领域库，包含实例生命周期管理、Mod 依赖图解析、下载任务与事务执行、统一数据模型以及错误类型。不依赖任何 UI 框架。
- `crates/mc-installer`：Mojang 官方原版元数据解析、下载校验引擎以及 Fabric 等 Mod Loader 安装器（阶段 1 逐步接入）。
- `crates/mc-mods`：Modrinth 内容平台接入、版本筛选、依赖关系分析与安装计划生成。
- `crates/mc-modpack`：.mrpack 格式导入导出、事务式部署与锁定文件管理。
- `apps/desktop`：基于 Slint 的纯 Rust 原生桌面客户端，通过强类型接口直接调用 mc-core，免除 WebView 与 Node 前端依赖。
- `crates/mc-api`：面向本地回环地址开放的 REST 服务，提供 OpenAPI 契约（阶段 2 接入）。
- `crates/mc-mcp`：基于标准输入输出的 stdio MCP 服务，供外部 AI Agent 编排整合包（阶段 2 接入）。

## 核心接口与服务契约

所有调用入口（桌面 GUI、REST API、MCP Server）统一调用由 `mc-core` 导出的服务层接口，杜绝各自实现独立的下载或安装逻辑：

1. `InstanceService`：管理实例的新增、查询、配置与本地存储隔离。
2. `CatalogProvider`：抽象 Mojang 官方源及 Modrinth 的版本查询与元数据检索。
3. `TaskService`：统一长耗时任务的生命周期追踪，支持状态查询与安全取消。

## 关键安全与开发约束

- 严格执行“先计划再执行”原则，所有涉及写入的变更操作必须首先生成 InstallPlan，经由用户批准后方可执行。
- 实例严格隔离于专属目录，所有文件路径操作必须进行规范化校验，严防路径穿越。
- 遵循官方协议与第三方合规约束，不内置游戏客户端资源，正版认证通过 AuthProvider 抽象接入，严禁使用非授权凭据或破解验证。
