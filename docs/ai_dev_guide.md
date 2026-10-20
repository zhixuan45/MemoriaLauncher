# MC AI Launcher 后续 AI 协作与开发规范

本文档面向后续参与本项目的 AI 编程助手，旨在固化当前桌面端与核心服务层已经验证的架构约束、界面组件复用规范以及前后端联调准则。进入任何开发或排查任务前，请优先读取工作区根目录下的 `index.md` 前 35 行路由区，按标签定位目标模块后再定向检索对应代码块，严禁盲目全量扫描代码库；完成任何文件新增、拆分或接口变更后，必须同步维护 `index.md`（保持纯净扁平 YAML 格式，严禁绘制 Mermaid 或 ASCII 流程图）。

## 核心架构与职责边界

本项目采用 Rust Workspace 架构，`crates/mc-core` 承载领域实体、统一错误类型与服务抽象契约（如 `InstanceService`、`TaskService`、`CatalogProvider`），`apps/desktop` 负责基于 Slint 的原生桌面视图渲染与用户交互事件绑定。所有业务状态必须由 Rust 后端服务层驱动并通过 `VecModel` 注入 Slint 视图模型，禁止在 `.slint` 文件中硬编码静态业务列表数据。当单文件代码量接近或超过 1000 行时，必须按职责边界拆分为独立子模块文件。

## Slint 界面开发与公共组件复用规范

Windows 平台默认中文字体在 Slint 渲染管线下不支持 Emoji 与特殊 Unicode 符号（如播放三角、设置符号、三点菜单、箭头、重做符号等），直接写入文本会导致界面显示为方框乱码。因此所有图标与装饰符号必须统一使用 `apps/desktop/ui/common.slint` 中封装的 `VectorIcon` 或原生 `Path` / `Rectangle` 进行纯矢量绘制，严禁在文本中混入特殊字符。

在编写新页面或新卡片前，必须优先复用 `apps/desktop/ui/common.slint` 中已抽离的原子组件与共享结构体，不得重复造轮子。所有深色毛玻璃卡片应直接继承或包裹 `GlassCard` 以保持圆角、底色与边框视觉一致；卡片顶部标题栏统一使用 `CardHeader`；开关控件统一使用 `ToggleSwitch`；实例标签统一使用 `TagPill`；超出可视区域需要滚动的列表统一使用 `Flickable` 搭配 `SlimScrollbar`，并在子项 `TouchArea` 上转发 `scroll-event` 以保证鼠标滚轮顺滑滚动。

## 无边框窗口、网格布局与自定义外观准则

主窗口运行于 `no-frame: true` 无边框模式，顶部 40px 区域与左侧导航栏顶部品牌区共同作为内化标题栏，通过 `drag_window`、`minimize_window_clicked`、`maximize_window_clicked` 与 `close_window_clicked` 回调交由 `apps/desktop/src/main.rs` 操控原生窗口位置与状态。`MainWindow` 根节点已预留 `custom_bg_image`、`bg_mask_color` 与 `bg_mask_opacity` 属性及对应背景遮罩图层，后续在「设置 -> 界面外观」中扩展自定义壁纸与遮罩浓度时，应直接双向绑定这组属性，不要另行叠加背景图层。

主页采用基于 `16px` 隐形网格的自由画布布局，所有模块均包裹在 `DraggableWidget` 容器内。开启「编辑布局」后，组件的拖拽位移与右下角宽高拉伸（`resizable: true`）均以 `16px` 为步进自动吸附对齐。其中「当前实例与启动/编辑按钮区 (`LaunchPanelCard`)」与「实例列表 (`InstanceListCard`)」属于核心受保护组件，必须保持 `deletable: false`，编辑模式下仅允许移动或缩放而不允许删除；其余统计、日志、任务队列等辅助卡片保持 `deletable: true`，且必须在 `MainWindow.reset_layout()` 中注册默认坐标与默认宽高以便一键恢复。

## 玩家档案同步与正版皮肤头像规范

玩家身份状态由 `common.slint` 中的 `PlayerProfile` 结构体统一承载，并通过 `MainWindow.current_profile` 与 `MainWindow.profiles` 在左侧导航栏底部档案选择器（最多展示 5 项高度，超出后自动启用滚轮与拖拽下滑）和「设置 -> 账户与数据」卡片之间保持双向实时同步。任何一处触发档案切换或新增，都必须同步更新 `MainWindow.current_profile`。

玩家头像渲染统一使用 `PlayerAvatar` 组件：对于 Microsoft 正版账号，后端应通过 `apps/desktop/src/main.rs` 中的 `extract_head_from_skin_rgba` 从标准 `64x64` 皮肤 RGBA 纹理中裁剪 `(8..16, 8..16)` 面部基础层与 `(40..48, 8..16)` 帽子覆盖层合成高清像素头像，并将 `has_avatar` 置为 `true` 以启用 `pixelated` 像素采样渲染；内置的纯矢量像素头像仅作为离线档案或皮肤纹理尚未拉取完成时的回退方案。
