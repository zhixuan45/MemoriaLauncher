# iced 迁移可行性评估

> 评估对象：`apps/desktop`（Slint 1.18.1，声明式 UI + `main.rs` 胶水）
> 目标：替换为 iced（Rust 原生，MIT）
> 评估时间：2026-10-20
>
> **结论先行：技术可行，但这是一次约 2,700 行 UI 的"重写"，不是换依赖。**
> 核心层 `crates/mc-core` 完全不用动；预计 **29–39 人日（现实值约 35 人日）**，单人 6–8 周。
> 但注意：**许可证问题很可能不需要重写就能解决**——Slint 1.18.1 提供无版税的 Royalty-free 2.0 选项，闭源/非商业桌面应用只需署名（见 §1）。即使目标协议是 **CC BY-NC-SA 4.0**，也不是必须换 iced 的充分理由；只有"整个仓库（含代码）必须单一 CC 协议覆盖，且不接受 Slint 的授权灰区/署名"时才值得迁移（见 §9）。建议先花 0.5–1 天确认 CC 的覆盖范围与 Slint 署名是否可接受。

---

## 1. 许可证事实核对（最重要的一节）

### 1.1 Slint 1.18.1 的实际许可结构（本机 crate 元数据核实）

`slint-1.18.1/Cargo.toml`：

```toml
license = "GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0"
```

许可证全文位于本机缓存：

- `C:\Users\JuziD\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\slint-1.18.1\LICENSES\GPL-3.0-only.txt`
- `...\LICENSES\LicenseRef-Slint-Royalty-free-2.0.md`
- `...\LICENSES\LicenseRef-Slint-Software-3.0.md`

| 选项 | 能否闭源 | 义务 / 成本 | 对本项目（桌面启动器） |
| --- | --- | --- | --- |
| **GPL-3.0-only** | 否 | 分发二进制即须以 GPLv3 提供整个程序源码 | 通常不可接受 |
| **Royalty-free 2.0** | **可以** | **无版税**；二选一署名：<br>(a) 在 About 页/对话框放 `AboutSlint` 组件（无 About 页则放启动画面）；<br>(b) 在公开下载页放 "Made with Slint" 徽章。<br>限制：不得脱离应用单独分发 Slint、不得用于嵌入式系统、不得对外暴露 Slint API | **可以用**。当前是桌面应用，只需加徽章即可合规 |
| **Slint Software License 3.0（商业）** | 可以 | 付费（按官方报价） | 需预算与采购 |

官方链接：

- 许可总览：<https://slint.dev/faqs> 、<https://slint.dev/pricing>
- Royalty-free 2.0 原文：<https://raw.githubusercontent.com/slint-ui/slint/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md>
- 上游 LICENSE.md：<https://raw.githubusercontent.com/slint-ui/slint/master/LICENSE.md>

> `slint-build` 同样是三选一许可，但它只在构建期执行，不随应用分发；真正约束分发的是运行时的 `slint` crate。
>
> 另：清单声明是 `slint = "1.9"`，但 `Cargo.lock` 实际锁定 **1.18.1**；分发时以实际解析到的 1.18.1 许可为准（上表即按 1.18.1 核实）。
>
> 如果应用本身打算采用 **CC BY-NC-SA 4.0**：Royalty-free 2.0 允许应用以其他协议分发（满足署名即可），但 **GPL-3.0-only 选项与 CC BY-NC-SA 不兼容**（NC 是 GPL 不允许的额外限制）。Royalty-free 与 CC 的组合仍有"不可再许可 / 附加条款"的灰区，详见 §9。

### 1.2 因此，"Slint 不符合许可证要求"可能是误判

只有以下情形才**必须**迁移：

1. 明确拒绝在应用内或下载页出现任何 Slint 署名/品牌；
2. 产品形态属于嵌入式，或需要把 Slint API 暴露给第三方；
3. 法务要求"零 copyleft + 零署名"的绝对宽松许可；
4. 要求整个仓库（含 Rust 代码）以单一 CC BY-NC-SA 4.0 分发，且不接受 Slint 授权与 CC 的兼容灰区（见 §9）。

如果真实诉求只是"不想开源"或"不想买商业授权"，那么 Royalty-free 2.0 已经覆盖：**下载页加一个徽章 = 0 代码改动**；加 About 弹窗也只需 2 小时。

如果诉求是"项目用 CC BY-NC-SA 4.0 发布"，请先区分覆盖范围：**只覆盖文档/素材/翻译** → 保留 Slint 不迁移；**覆盖代码** → Slint 的 GPL 选项不可用，Royalty-free 可用但有法律灰区，而 iced(MIT) 的组合更干净（但仍不建议把 CC 用于代码）。完整决策见 §9。

当前 UI 中没有任何"关于/About"入口（`apps/desktop/ui/*.slint` 中无相关文本），所以方案 (b) 是成本最低的合规路径。另需注意：仓库根目录目前没有 `LICENSE` 文件，建议一并补齐项目自身的许可声明。

### 1.3 iced 的许可

- iced 采用 **MIT**：无 copyleft、无署名、可闭源商用。参考仓库：<https://github.com/iced-rs/iced>
- iced 的传递依赖同样以 MIT / Apache-2.0 为主（wgpu、winit 等）；当前 Slint 方案已经引入 `wgpu 30` + `winit 30`，迁移后的许可面不会变差。
- ⚠️ **本次评估无法做联网复核**：本机 `cargo` 访问 crates.io 失败（SSL/Schannel 错误），`web_fetch` 对 docs.rs / crates.io / github.com 均被网络策略拦截。iced 的 MIT 结论来自公开项目元数据，**合入前请用 `cargo deny` / `cargo about` 生成第三方许可清单再复核一次**。

---

## 2. 现状盘点：迁移边界有多大

### 2.1 架构边界清晰

- [`Cargo.toml`](../Cargo.toml) 是 workspace：`crates/mc-core`（领域/服务，无 GUI 依赖）+ `apps/desktop`（唯一 GUI crate）。
- [`crates/mc-core/Cargo.toml`](../crates/mc-core/Cargo.toml) 只依赖 tokio/serde/thiserror 等；服务层用 `tokio::sync::RwLock` 做内存实现，**不依赖任何 UI 框架**。
- 结论：迁移被限制在 `apps/desktop` 一个 crate 内，**mc-core 与测试零改动**。

### 2.2 代码量

| 文件 | 行数 | 作用 |
| --- | ---: | --- |
| [`apps/desktop/ui/app.slint`](../apps/desktop/ui/app.slint) | 563 | 无边框主窗口、自绘标题栏、主页画布布局 |
| [`apps/desktop/ui/widgets.slint`](../apps/desktop/ui/widgets.slint) | 723 | 拖拽/缩放/网格吸附容器、侧边栏、启动面板、统计栏 |
| [`apps/desktop/ui/cards.slint`](../apps/desktop/ui/cards.slint) | 398 | 实例列表（拖拽排序）、更新日志、任务队列 |
| [`apps/desktop/ui/settings.slint`](../apps/desktop/ui/settings.slint) | 620 | 设置页：搜索、二级 tab、5 张卡片 |
| [`apps/desktop/ui/common.slint`](../apps/desktop/ui/common.slint) | 390 | 数据模型、20 种矢量图标、头像/开关/徽章等基础组件 |
| [`apps/desktop/src/main.rs`](../apps/desktop/src/main.rs) | 619 | 服务初始化、模型映射、15 个回调绑定、窗口控制 |
| **合计** | **~3,313** | |

依赖现状：`slint 1.18.1`（[`apps/desktop/Cargo.toml:9-12`](../apps/desktop/Cargo.toml)）；本机实测依赖树 **664 个 crate**，debug 二进制 35.7 MB。iced 会是同一量级（wgpu + winit 生态），不会显著变重。

> 注：本报告的行号/行数以 **2026-10-20 20:40 快照**为准。评估期间该目录正被并行整理（组件去重，`cards.slint`/`settings.slint` 刚被压缩），如后续行号漂移，请按组件名检索。

### 2.3 实际页面范围比文件量小

- 主导航 4 个 tab，**只有 tab 0（主页）和 tab 3（设置）有内容**；tab 1（游戏下载）、tab 2（AI 功能）目前点击后是空白画布（[`app.slint:404`](../apps/desktop/ui/app.slint)、[`app.slint:545`](../apps/desktop/ui/app.slint)）。
- 设置页 7 个二级 tab **只切换高亮**，正文始终是"常规"的 5 张卡片（[`settings.slint:336-351`](../apps/desktop/ui/settings.slint)、[`settings.slint:353-618`](../apps/desktop/ui/settings.slint)）。
- 需要真正复刻的只有：**主页画布（5 个可拖拽模块 + 3 张列表卡）+ 侧边栏/账号弹层 + 设置单页 + 无边框窗口壳**。

### 2.4 用到的 Slint 特性（决定迁移难度）

| 能力 | 用量 / 位置 | iced 复刻难度 |
| --- | --- | --- |
| 声明式布局（Horizontal/VerticalLayout、padding、stretch） | 全项目 | 中（iced 用 row/column/container + Length 显式表达，无"自动测量"布局） |
| 属性绑定 / 双向绑定（`<=>`，大量派生表达式） | 全项目，如 [`app.slint:30-95`](../apps/desktop/ui/app.slint) | 中（改为 Elm 单向数据流，派生值在 update/view 中重算） |
| `Flickable` + 自绘滚动条 | 4 处（[`cards.slint:70`](../apps/desktop/ui/cards.slint)、[`cards.slint:234`](../apps/desktop/ui/cards.slint)、[`cards.slint:296`](../apps/desktop/ui/cards.slint)、[`widgets.slint:443`](../apps/desktop/ui/widgets.slint)） | 中（iced `scrollable` 可用，但滚动条宽度/外观受限，必要时自绘） |
| `TouchArea` 拖拽 / `has-hover` / `pressed` | hover ~23 处、pressed ~27 处 | 中（hover 需统一包装 widget；pressed 无隐式指针捕获） |
| 16px 网格拖拽 + 缩放手柄 | [`widgets.slint:49-245`](../apps/desktop/ui/widgets.slint) | **难**（需自定义 Widget 或全局事件订阅 + 状态机） |
| 列表拖拽排序 + 槽位吸附预览 | [`cards.slint:40-44`](../apps/desktop/ui/cards.slint)、[`cards.slint:77-107`](../apps/desktop/ui/cards.slint)、[`cards.slint:162-198`](../apps/desktop/ui/cards.slint) | **难**（同上） |
| SVG 风格 `Path` 图标（fill/stroke） | ~38 处，20 种图标：[`common.slint:62-185`](../apps/desktop/ui/common.slint) | 中（转成 SVG 文件用 iced `svg` feature；或 canvas 自绘） |
| 线性渐变 | 1 处（[`widgets.slint:258`](../apps/desktop/ui/widgets.slint)） | 低（iced gradient） |
| 隐式动画 `animate` | 2 处（开关滑块、拖拽占位框） | 低（订阅 tick + 插值，或 0.14 动画能力） |
| `TextInput` + 占位符 + focus 边框 | 设置页搜索框（[`settings.slint:303-332`](../apps/desktop/ui/settings.slint)） | 低（`text_input`），但 **IME 中文输入必须验证** |
| `Image`（RGBA 皮肤头像、PNG 图标、cover/contain） | 6 个组件 / 12 处引用；头像管线在 [`main.rs:16-56`](../apps/desktop/src/main.rs) | 低（`image::Handle::from_rgba` + `ContentFit`） |
| 文本 `elide`（省略号） | 14 处 | **中/差**（iced 无省略号，需截断或自绘） |
| 系统字体/中文渲染 | 全局中文 UI | 中（cosmic-text 理论支持 fallback，需实测） |

其他量化：`Text` 62 个、`Rectangle` 108 个、`TouchArea` 26 个、`preferred-width` 5 处——说明这是"大量细粒度静态盒子 + 少量强交互"的 UI，**静态部分可批量迁移，交互部分必须逐个重做**。

另外，`main.rs` 619 行里约 300 行是事件闭包，真正的异步只有 1 处（`refresh` 的 `tokio::spawn` + `invoke_from_event_loop`，[`main.rs:519-535`](../apps/desktop/src/main.rs)）。所以**异步桥接不是瓶颈，成本集中在视觉与交互还原**。

---

## 3. iced 能力映射（逐项判定）

> 下表基于 iced 0.13/0.14 的公开 API 认知。**本环境无法编译 iced**，标注"需验证"的条目必须在 spike 中确认（见 §6 P1）。

| Slint 能力 | iced 对应方案 | 判定 | 说明 / 风险 |
| --- | --- | --- | --- |
| `no-frame` 无边框窗口 | `window::Settings { decorations: false, transparent: true, min_size, .. }` + `application().window(...)` | ✅ 可行 | 圆角/阴影需自绘；透明窗口 + wgpu 合成需实测 |
| 自绘标题栏拖动窗口 | `iced::window::drag(id)`（0.13+） | ✅ 可行 | 需先拿到 window id（`window::get_latest()` 或 `window::Event::Opened`，需验证） |
| 最小化 / 最大化 / 关闭 | `window::minimize/maximize/close` | ✅ 可行 | 与 Slint 行为等价 |
| 布局系统 | `row!` / `column!` / `container` + `Length::{Fill, Shrink, Fixed}` | ⚠️ 大致可行 | 没有 Slint 的自动 preferred-size 布局链；文本宽度依赖测量，多行/换行会改变尺寸 |
| 响应式绑定 `property: expr` | `App` 状态 + `Message` + `update` + `view` | ⚠️ 需重构 | 派生值（如 `visual_slot`、`target_index`）要在 update 里算好，不能"自动重算" |
| 双向绑定 `<=>` | 单向数据流 + 显式消息 | ⚠️ 需重构 | 15 个回调 → `Message` 枚举；`main.rs` 619 行会变成 800–1,000 行状态机 + view |
| `VecModel` 可观察列表 | `Vec<T>` 放在 `App`，view 遍历 | ✅ 可行 | iced 每次 update 重建 view；列表做了 diff，性能可接受 |
| 组件组合（`@children`、组件参数） | 普通函数返回 `Element` + 泛型/参数 | ✅ 可行 | Rust 类型系统会强迫显式抽象，可读性更好但样板更多 |
| 自定义拖拽/缩放容器 | 自定义 `Widget`（`advanced` feature）或 `event::listen_with` + `mouse_area` | ⚠️ 可做，工作量集中 | iced **没有隐式指针捕获**：按住后光标移出控件就收不到 move。必须在应用层订阅全局鼠标事件做状态机 |
| 列表拖拽排序 | 同上 + 滚动容器坐标换算 | ⚠️ 可做 | 需要自己维护 drag index / 目标槽位 / 自动滚动；参考 iced 的 `pane_grid` 思路 |
| 滚动容器 | `scrollable` + `scrollable::Style` | ✅ 可行 | 可换色/圆角；**滚动条宽度与"按进度定位的细条"受框架限制**，必要时用 overlay 自绘 |
| 悬浮层 / 账号弹层 | `stack`（0.13+，需验证）或自定义 overlay widget | ⚠️ 需验证 | iced 的 overlay 机制（`overlay`）可做；命中测试/点击外部关闭需自己处理 |
| 矢量图标 | iced `svg` feature + `Svg` widget（`Svg::style` 可覆盖颜色，需验证） | ✅ 可行（推荐） | 把 slint 的 `commands` 批量转成 16×16 SVG 文件；描边图标与动态颜色需逐个验证 |
| 或：canvas 自绘图标 | `canvas::Canvas` + `canvas::Path`（支持 line/bezier/arc） | ✅ 备选 | 适合需要动态颜色/动画的少量图标，代码量大 |
| 线性渐变 | `iced::gradient::Linear` / `Background::Gradient` | ✅ 可行 | 0.13+ |
| 动画 | `window::frames()` / `time::every` 订阅 + 插值 | ✅ 可行 | 现有只有 2 处动画，成本很低 |
| hover 状态 | `button` 自带 `Status`；自定义容器需 `HoverArea` 包装 widget | ⚠️ 需统一封装 | 约 23 处 hover，逐处手写会失控；建议只实现一个通用 `Surface`/`HoverArea` 组件 |
| 文本省略号 | 无内置 ellipsis；`Wrapping::None` 硬裁切或手动测量截断 | ⚠️ 保真风险 | 14 处 `elide`，是视觉回归最可能出现的地方 |
| 中文输入（IME） | `text_input` + winit IME | ⚠️ **必须验证** | iced 历史版本 IME 支持不完善；设置搜索框会受影响 |
| 皮肤头像 RGBA | `image::Handle::from_rgba(w, h, pixels)` | ✅ 可行 | 64×64 → 34px 显示；`FilterMethod::Nearest` 是否可用需验证 |
| 异步服务（tokio） | 保持 tokio Runtime，`Task::perform` / `Subscription` 桥接 | ⚠️ 需适配 | 当前 `#[tokio::main]` + Slint 事件循环，iced 要求 `main` 由事件循环接管 |
| 多页切换 | `App.page` 枚举 + 条件 view | ✅ 可行 | 简单 |
| DPI 缩放 | iced/winit 自动处理；像素图标建议用 SVG 而非位图 | ✅ 可行 | 位图 PNG（icon_*.png）在高 DPI 下会糊，Slint 版本同样问题；SVG 是加分项 |

### 3.1 四个真正的工作量集中点

1. **自由画布交互**：拖动 + 16px 吸附 + 缩放手柄 + 边界约束 + 编辑模式删除/恢复（[`widgets.slint:49-245`](../apps/desktop/ui/widgets.slint)）。
2. **列表拖拽排序**：按下阈值、跟手位移、其他行让位、目标槽位预览、滚动容器内坐标换算（[`cards.slint:40-44`](../apps/desktop/ui/cards.slint)、[`cards.slint:77-107`](../apps/desktop/ui/cards.slint)、[`cards.slint:162-198`](../apps/desktop/ui/cards.slint)）。
3. **像素级布局与文本**：没有 `elide`、没有 preferred-width 链，需要冻结设计 token 并逐屏比对。
4. **无边框窗口壳 + 账号弹层 + hover 系统**：这些是"到处都在用"的基础设施，必须先在基座阶段做对，否则后面每个页面都要返工。

---

## 4. 主要风险

| # | 风险 | 概率 | 影响 | 缓解 |
| --- | --- | --- | --- | --- |
| R1 | 视觉/布局保真度不足（间距、字号、截断、对齐） | 高 | 中 | 冻结设计 token；逐组件截图比对；接受 1–3px 偏差；省略号改为硬裁切或自绘 |
| R2 | 拖拽交互语义（无指针捕获） | 高 | 高 | 统一实现 `DragController`（press/move/release + 阈值 + 吸附 + 光标形状），只写一次 |
| R3 | 图标资产迁移 | 中 | 低 | 脚本把 38 条 `commands` 转 SVG（含 `A` 弧指令，SVG 原生支持）；先验证 `Svg::style` 颜色覆盖与描边 |
| R4 | 中文渲染 / IME | 中 | 高 | spike 实测；失败则内置 Noto Sans SC（+~10MB）并降级搜索框 |
| R5 | hover / 按压态样式 | 中 | 低 | 一个通用 `Surface` 组件覆盖 23 处 hover |
| R6 | 无边框窗口平台差异 | 低 | 中 | Windows 目标下 `window::drag` 可用；Wayland 不支持拖拽，若未来上 Linux 需兜底 |
| R7 | tokio 与 iced 执行器/生命周期 | 中 | 中 | 保持 tokio Runtime 句柄；服务调用封装成 `Task`；启动数据加载改成异步 `Message` |
| R8 | 开发效率下降（无 live preview） | 中 | 中 | 接受迭代变慢 20–30%；先做组件库再拼页面 |
| R9 | iced 0.x API 演进 | 低 | 中 | 锁定当时的稳定大版本（0.14+）；只用公开 API；CI 固定 toolchain |
| R10 | 迁移期双 UI 维护 | 中 | 低 | 旧 Slint UI 冻结功能，只修 bug；新 crate 并行开发到验收 |

---

## 5. 工作量估算

假设：单人熟悉 Rust、无 iced 经验但有 Slint 版可对照；目标是**像素级还原 + 交互等价**；设计稿/截图已具备。

| 阶段 | 交付内容 | 人日 |
| --- | --- | ---: |
| P0 许可证复核 | 确认是否真的必须迁移 | 0.5 |
| P1 技术验证 spike | 可运行的 8 项验证 demo（见 §6） | 2–3 |
| P2 基座与设计系统 | 主题/样式、字体、SVG 图标、图片资源、异步桥、`Surface`/hover、通用按钮/开关/徽章/头像/滚动条 | 5–7 |
| P3 窗口壳与导航 | 无边框窗口、自绘标题栏、4 tab 路由、侧边栏、账号弹层 | 3–4 |
| P4 主页画布 | `DraggableWidget`（拖/缩放/吸附/删除）、3 张卡片（含拖拽排序）、启动面板、统计栏 | 8–10 |
| P5 设置页 | 搜索框、7 个 tab、5 张卡片、下拉/数据行/快捷磁贴 | 4–5 |
| P6 打磨与回归 | hover/动画、DPI、IME、截图对比、交互验收、性能 | 6–8 |
| P7 切换与清理 | 移除 Slint、CI/打包、文档更新 | 1–2 |
| **合计** | | **29–39（现实约 35）** |

参考换算：

- 单人：**6–8 周**。
- 双人并行：**4–5 周**（P4 的拖拽画布不宜拆分，其余可按"基座/主页 vs 设置/弹层"分工）。
- 如果接受**功能等价、视觉降级**（不做像素级还原、省略号改裁切、滚动条用默认样式）：可压缩到 **15–18 人日**。

> 独立评审的交叉验证：另一位评审者给出的区间是 **38–58 人日**（图标 9、画布 7、重排 5、设置 6、滚动/窗口/字体 6、状态 4、视觉回归 8、缓冲 6），与本报告上限重叠。差异主要来自图标实现方式（SVG vs canvas 自绘）和视觉验收标准，**建议预算按 35–40 人日、进度按 6–8 周排，并把 spike 设为硬性 go/no-go 门**。
>
> 成本对比：迁移 ≈ 35 人日；若 Slint 商业授权的采购成本低于这个数，直接买授权更划算；若可接受署名，成本接近 0。

---

## 6. 推荐迁移路径（带 go/no-go 门）

### 阶段划分

1. **P0（0.5–1 天）许可证确认**——先回答 §8 的四个问题。若"署名可接受且 CC 只覆盖文档/素材"，直接结束，不迁移。
2. **P1（2–3 天）Spike，独立可运行 demo**——只验证风险最高的 8 项，**全部通过才进入正式迁移**：
   1. 无边框窗口 + `decorations: false` + min_size + 自绘标题栏拖动 + 最小化/最大化/关闭；
   2. 中文渲染（不内置字体）与 `text_input` 拼音 IME；
   3. SVG 图标：取 3 个现有 Path（fill、stroke、多色）渲染成 16/32px，验证颜色覆盖与 150% DPI；
   4. `image::Handle::from_rgba` 皮肤头像 + 最近邻过滤；
   5. 滚动容器 + 3px 细滚动条外观；
   6. 单个 `DraggableWidget`：16px 吸附、边界约束、缩放、光标形状、**按住后移出控件仍能跟手**；
   7. 列表拖拽排序 + 槽位预览（可先用瞬时重排，后加动画）；
   8. tokio 服务调用 → iced 消息桥接；空闲 CPU / 窗口缩放流畅度。
3. **P2 基座**：先做设计 token（颜色、字号、间距、圆角）与 8–10 个基础组件，再拼页面。
4. **P3–P5**：按"窗口壳 → 主页 → 设置"顺序迁移，每完成一页与 Slint 版并排截图验收。
5. **P6**：交互回归 + DPI + IME + 性能。
6. **P7**：验收通过后删除 Slint 依赖与 UI 文件，`index.md` 技术栈改为 iced。

### 工程组织

- 新建 workspace 成员 `apps/desktop-iced`（独立的 `Cargo.toml`），与现有 `apps/desktop` 并存，便于并排运行和截图对比；验收后删除旧 crate。
- `mc-core` 不改；视图模型映射函数（`map_instance_to_entry` 等）从 `main.rs` 迁移到新 crate 的 `view_model.rs`，语义保持不变。
- 旧 Slint 版本在迁移期**只修 bug、不加功能**，避免两边同时改。
- **不要一边迁移一边做"游戏下载 / AI 功能"新页面**，否则工作量会成倍增长。
- CI：迁移期同时构建两个 bin；最终只保留 iced。

### 验收标准（建议写进任务单）

- 窗口：1146×640 启动、1024×614 最小尺寸、无边框可拖动、最小化/最大化/还原/关闭正常。
- 交互：16px 网格吸附、拖拽排序、删除/恢复模块、账号切换/新增、任务暂停、设置开关与下拉、状态栏文案与 Slint 版一致。
- 视觉：1440×900 / 1024×614、100%/150% DPI 截图与 Slint 版并排比对，允许 1–3px 偏差。
- 文本：中文无方框/无截断错误；省略号行为有明确降级方案。
- 性能：空闲 CPU ≈0，拖动/缩放无明显掉帧，冷启动不慢于 Slint 版。
- 依赖：`cargo tree` 中不再出现 `slint`/`slint-build`。

---

## 7. 备选方案对比

| 方案 | 许可 | 视觉保真 | 一次性工作量 | 评价 |
| --- | --- | --- | ---: | --- |
| **继续 Slint + Royalty-free 2.0 署名** | 合规（需署名） | 100% | **≈0** | **若可接受署名/徽章，首选；这是本报告最重要的结论**。若整仓要套 CC BY-NC-SA，需先做兼容性确认（§9） |
| Slint 商业授权 | 付费 | 100% | 0 | 预算允许时最稳，无迁移风险 |
| **iced 迁移** | MIT | 85–95% | 29–39 人日（现实 ~35） | 可行；纯 Rust、无 WebView、许可干净；代价是 UI 重写 + 像素保真风险 |
| egui 迁移 | MIT/Apache | 60–75% | 18–24 人日 | 开发更快，但立即模式 UI 很难还原玻璃拟态/像素级设计与精确布局，长期维护成本高 |
| Tauri 2 + Web 前端 | MIT/Apache | 95%+ | 25–35 人日（另加 TS/前端工具链） | 原 spec 建议方案；视觉最可控，但引入 WebView 与前端栈，与"纯 Rust 原生"取向冲突 |
| GTK4 / Qt 绑定 | LGPL / LGPL+商业 | 70% | 30+ 人日 | Windows 打包与主题一致性差，不建议 |

---

## 8. 结论与需要确认的问题

**技术结论：iced 可行，但属于"中等规模 UI 重写"，风险集中在拖拽交互、像素级布局、中文 IME 三处；架构上因为 `mc-core` 与 UI 完全解耦，迁移是干净的。**

**决策建议：**

1. 先确认 Slint 到底卡在哪一条许可，以及 CC BY-NC-SA 4.0 的覆盖范围（见 §9）。若"下载页徽章/About 署名"可接受且 CC 只覆盖文档/素材 → 不迁移（0.5 天解决 6–8 周的问题）。
2. 若确实必须去掉 Slint → 先做 2–3 天 spike，通过后按 P2–P7 排 6–8 周；建议 2 人并行压到 4–5 周。
3. 无论选哪条，都先补仓库 `LICENSE`/`NOTICE`，并用 `cargo deny` 固化第三方许可清单；素材来源单独审计。

**需要你回答的四个问题：**

1. Slint 具体是哪一条不满足？（GPL 传染 / 不接受任何署名 / 采购不允许 / 法务要求零 copyleft / 其他）
2. 是否接受"像素级还原 + 迁移期冻结 UI 功能"的成本？还是可以接受视觉降级（约 15–18 人日）？
3. 目标平台只有 Windows 吗？（Linux/Wayland 下无边框窗口拖拽能力不同，会影响方案与测试范围）
4. **CC BY-NC-SA 4.0 计划覆盖到哪里？**（仅文档/素材/翻译，还是整个仓库含 Rust 代码？这直接决定"换不换"）

---

## 9. 补充：目标协议是 CC BY-NC-SA 4.0 时的决策

> **非法律意见。** CC 协议与软件、第三方组件组合的成文规则和判例都很少，正式发布前建议让律师确认一次。

先分清两种"使用 CC BY-NC-SA"：

1. **把 CC BY-NC-SA 作为项目自身协议**（整个仓库或部分内容）——下文 §9.2–§9.6 讨论的就是这种；
2. **在项目里引用 CC BY-NC-SA 的第三方资源**（例如 CFPA 汉化包）——只需遵守该资源自身的 BY/NC/SA 条件（保留署名、不商用、修改后同协议），**不要求项目整体改用 CC**，也不构成换 iced 的理由。如果只是这种情况：保留 Slint，按资源许可写 NOTICE 即可。

### 9.1 先把 CC BY-NC-SA 用对地方

CC BY-NC-SA 4.0 = 署名（BY）+ 禁止商业使用（NC）+ 改编作品同协议（SA）。它**不是 OSI 开源协议**，Creative Commons 官方也**不建议用于软件**，原因是软件场景的几个硬缺口：

1. **没有专利授权**（MIT/Apache-2.0/GPL 都有专利条款）；
2. **不区分源码/目标码**，也不定义"分发"和"对应源码"；
3. **NC 边界模糊**：赞助、广告、付费下载、企业内部使用都可能被解释成"商业"；
4. **SA 与"链接/动态库是否构成改编"**没有清晰定义，和任何第三方组件组合都有解释空间；
5. **与 GPL 不兼容**（NC 是 GPL 禁止的额外限制），与多数软件许可证也没有正式兼容声明。

结论：**文档、美术、翻译、设计稿用 CC BY-NC-SA 4.0 很合适；Rust 代码建议改用软件许可证。**

### 9.2 兼容性矩阵（应用采用 CC BY-NC-SA 4.0 时）

| 组合 | 结论 | 说明 |
| --- | --- | --- |
| CC BY-NC-SA + Slint **GPL-3.0-only** | ❌ 不可行 | GPL 要求允许商业使用、不得附加限制；NC 与 GPL 直接冲突 |
| CC BY-NC-SA + Slint **Royalty-free 2.0** | ⚠️ 可行但有灰区 | 该许可不要求应用 copyleft，允许应用以任意协议分发（需署名）；但它**不可再许可**、带有限制条款，与 CC 的 "no additional restrictions"/SA 存在解释空间。把 Slint 当作独立第三方组件、CC 只覆盖自有代码并保留 NOTICE，是常见做法，但需法律确认 |
| CC BY-NC-SA + Slint **商业授权** | ⚠️ 视合同 | 通常允许，但要确认合同里的转授权/分发条款 |
| CC BY-NC-SA + **iced（MIT）** | ✅ 最干净 | MIT 明确允许 sublicense，只需保留版权与许可声明；但 iced 依赖中的 `resvg`（MPL-2.0）、Apache-2.0 等仍需逐项保留声明，且 CC 用于代码的固有缺陷依旧存在 |
| CC BY-NC-SA + GPL/AGPL 依赖 | ❌ | 同第一行，NC 与 copyleft 冲突 |

**关键区分：你分发的是源码还是二进制？**

- **只发源码**（用户自己 `cargo build`，Slint 从 crates.io 拉取）：你的仓库里没有"分发 Slint"，CC 覆盖自有代码的摩擦最小，Slint 的署名义务通常在你分发**应用**时触发。
- **发安装包/二进制**：Slint 被静态链接进产物，上述灰区和 NOTICE 义务都适用。

### 9.3 两种推荐落法

**方案 A（推荐）：拆分许可**

- 代码：**PolyForm Noncommercial 1.0.0**（软件专用、源码可见、禁止商业使用；没有 SA）或 BUSL/Elastic（若想保留未来商业化路径）。
- 文档/美术/翻译：**CC BY-NC-SA 4.0**（与 CFPA 资源一致）。
- Slint：继续 **Royalty-free 2.0 + 下载页 "Made with Slint" 徽章**（或 AboutSlint）。
- 结果：**不需要迁移 iced，0 行 UI 改动**；代价是"非商业 + 署名"两条约束和一次法律确认。
- 局限：如果"NC + SA"两个都要，软件许可证里没有现成标准答案——PolyForm 没 SA；GPL/AGPL 有 SA 但允许商用。这是 CC 用于代码的根因。

**方案 B：整个仓库（含代码）统一 CC BY-NC-SA 4.0**

- iced(MIT) 的组合比 Slint 干净（可再许可，只需 NOTICE），所以如果**必须**单一 CC 协议覆盖全部，iced 才成为"许可证驱动"的合理选择。
- 但代价仍在：6–8 周迁移；CC 用于代码的固有问题不会消失；依赖声明（MPL/Apache）仍需处理；素材版权仍需解决。
- 结论：**不要为了"能用 CC"而迁移**；只有"整仓单一 CC + 不接受 Slint 授权灰区/署名"同时成立时，迁移才划算。

### 9.4 必须先处理的素材版权

`apps/desktop/ui/assets/` 里的图片没有来源记录：

- `icon_grass.png` 是 Minecraft 草方块风格，`bg_wallpaper.png` 是 Minecraft 风格场景，`banner_*.png` 疑似 AI 生成；
- 如果像素/贴图来自 Mojang 或第三方素材，**你不能把它们重新许可为 CC BY-NC-SA**；
- AI 生成图在部分法域可能不受版权保护，CC 许可对其没有实际意义；
- Minecraft 名称/界面风格还涉及商标与 Mojang 使用规范（与协议无关，但发布时一并注意）。

处理：素材单独列 `NOTICE` / `ASSETS.md` 说明来源与许可；来源无法确认的替换为原创/AI 生成并保留生成记录；CC 只覆盖你拥有权利的部分。

### 9.5 决策树

| 你的情况 | 建议 | 是否迁移 |
| --- | --- | --- |
| CC 只覆盖文档/素材/翻译 | 代码用 PolyForm NC，Slint 继续 Royalty-free + 徽章 | **不迁移** |
| 整仓 CC + 能接受 Slint 署名 + 愿做法律确认 | Slint 保留，NOTICE 说明第三方组件 | **不迁移** |
| 整仓 CC + 不接受署名 / 要单一协议干净 | iced(MIT) + NOTICE；先做素材审计与法律确认 | 迁移（29–39 人日） |
| 想要 OSI 开源（允许商用） | CC 用错了：改 MIT / Apache-2.0 / GPL-3.0（注意：若改用 Slint GPL-3.0 选项，应用本身必须 GPLv3；要 MIT/Apache 则走 Royalty-free 或 iced） | 与迁移无关 |

### 9.6 行动清单（无论选哪条）

- [ ] 根目录 `LICENSE`：明确覆盖范围（代码 vs 文档/素材）
- [ ] `NOTICE` / `THIRD_PARTY_LICENSES`：Slint 或 iced + 全部依赖 + 素材
- [ ] 素材来源审计（Minecraft 贴图/皮肤/壁纸/banner）
- [ ] README 写明"非 OSI 开源 / 非商业"（若采用 NC）
- [ ] 源文件 SPDX 头（可选，便于 `cargo deny`/CI 审计）
- [ ] 律师确认 Slint 与 CC 的组合（方案 A 的灰区）

---

## 附录 A：iced 落地骨架（示意，**未编译验证**）

```rust
// 仅示意 API 形态，具体签名以 iced 0.14 文档为准
fn main() -> iced::Result {
    iced::application("MC 启动器", App::update, App::view)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: iced::Size::new(1146.0, 640.0),
            min_size: Some(iced::Size::new(1024.0, 614.0)),
            decorations: false,
            transparent: true,
            resizable: true,
            ..Default::default()
        })
        .subscription(App::subscription)
        .run_with(App::new)
}
```

```rust
// 全局指针事件：用于跨控件拖拽/缩放（Slint 的 TouchArea pressed 捕获在 iced 中需自己实现）
fn subscription(&self) -> iced::Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| match event {
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::PointerDown)
        }
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::PointerMoved(position))
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::PointerUp)
        }
        _ => None,
    })
}
```

```rust
// 数据模型：VecModel -> Vec；回调 -> Message
struct App {
    instances: Vec<InstanceEntry>,
    current_instance: InstanceEntry,
    tasks: Vec<TaskEntry>,
    profiles: Vec<PlayerProfile>,
    layout: LayoutState,        // 18 个 length 属性 -> 结构体
    drag: DragState,            // dragging_index / delta / target_index
    edit_mode: bool,
    status_text: String,
    window_id: Option<window::Id>,
}
```

> 说明：iced 的 `main` 由事件循环接管，当前 `#[tokio::main]` 需要改成"手动创建 tokio Runtime（保留句柄）→ 启动 iced"；服务调用的结果通过 `Task`/`Subscription` 回到 `update`。这部分预计 1–2 人日。

---

## 附录 B：证据索引

| 结论 | 证据 |
| --- | --- |
| Slint 三选一许可 | 本机 `slint-1.18.1/Cargo.toml`；`LICENSES/` 下三份全文 |
| Royalty-free 2.0 的署名义务与限制 | `slint-1.18.1/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md` §2–§3 |
| 无 GUI 耦合 | [`crates/mc-core/Cargo.toml`](../crates/mc-core/Cargo.toml)、[`crates/mc-core/src/service/instance.rs`](../crates/mc-core/src/service/instance.rs) |
| 无边框窗口与自绘标题栏 | [`apps/desktop/ui/app.slint:20-27`](../apps/desktop/ui/app.slint)、[`app.slint:190-401`](../apps/desktop/ui/app.slint) |
| 窗口控制回调 | [`apps/desktop/src/main.rs:311-351`](../apps/desktop/src/main.rs) |
| 16px 网格拖拽/缩放 | [`apps/desktop/ui/widgets.slint:41-246`](../apps/desktop/ui/widgets.slint) |
| 列表拖拽排序 | [`apps/desktop/ui/cards.slint:40-44`](../apps/desktop/ui/cards.slint)、[`cards.slint:77-107`](../apps/desktop/ui/cards.slint)、[`cards.slint:162-198`](../apps/desktop/ui/cards.slint) |
| 矢量图标库 | [`apps/desktop/ui/common.slint:62-185`](../apps/desktop/ui/common.slint) |
| 皮肤头像管线 | [`apps/desktop/src/main.rs:16-90`](../apps/desktop/src/main.rs) |
| 异步任务/事件回主线程 | [`apps/desktop/src/main.rs:519-535`](../apps/desktop/src/main.rs)、[`main.rs:552-579`](../apps/desktop/src/main.rs) |
| 空页面（tab 1/2 无视图） | [`apps/desktop/ui/app.slint:404`](../apps/desktop/ui/app.slint)、[`app.slint:545`](../apps/desktop/ui/app.slint) |
| 设置页 7 tab 单内容 | [`apps/desktop/ui/settings.slint:336-351`](../apps/desktop/ui/settings.slint)、[`settings.slint:353-618`](../apps/desktop/ui/settings.slint) |
| iced 0.14 已发布 | <https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary>、<https://newreleases.io/project/github/iced-rs/iced/release/0.14.0> |
| iced 文档 | <https://docs.iced.rs>、<https://github.com/iced-rs/iced> |

> 复核提示：本报告对 iced 的具体 API（`window::drag`、`stack`、`Svg::style`、`FilterMethod`、`Application` builder 的 feature 开关、IME 支持）只做了文档级判断，**未做编译验证**；spike 阶段请以 `cargo doc` 生成的真实签名逐项打勾。
>
> 快照说明：仓库在评估期间正被并行整理（`cards.slint` 627→398 行、`settings.slint` 1214→620 行，组件去重为 `GlassCard`/`CardHeader`/`SlimScrollbar`），本报告数字与行号以 2026-10-20 20:40 状态为准；若继续重构，请按组件名而非行号定位。
