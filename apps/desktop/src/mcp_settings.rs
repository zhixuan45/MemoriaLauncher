use crate::{
    app::{Launcher, Message as AppMessage},
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, checkbox, column, container, row, scrollable, Space},
    Alignment, Fill, Task,
};
use mc_core::service::mcp_policy::{policy_path, McpPolicy};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Message {
    Enabled(bool),
    Permission(usize, bool),
    Instance(String, bool),
    UseCurrent,
    Save,
    Saved(Result<McpPolicy, String>),
    Copy,
    CopyQuestExample,
    ImportRegistry(String),
    RegistryPicked {
        root: PathBuf,
        instance: String,
        path: Option<PathBuf>,
    },
    RegistryImported(Result<String, String>),
}
pub struct State {
    pub draft: McpPolicy,
    pub saved: McpPolicy,
    pub busy: bool,
    pub notice: String,
}
impl State {
    pub fn load(root: &Path) -> Self {
        let loaded = McpPolicy::read(&policy_path());
        let notice = match &loaded {
            Err(error) => error.clone(),
            Ok(Some(_)) => {
                "权限配置已加载。修改后请保存，正在运行的 MCP 进程会在下次请求时重新读取。".into()
            }
            Ok(None) => {
                "保存后，内置 MCP 服务将按此配置限制权限；未保存时继续使用原有只读服务配置。".into()
            }
        };
        let policy = loaded.ok().flatten().unwrap_or_else(|| McpPolicy {
            game_root: std::path::absolute(root).unwrap_or_else(|_| root.into()),
            ..Default::default()
        });
        Self {
            draft: policy.clone(),
            saved: policy,
            busy: false,
            notice,
        }
    }
    pub fn update(&mut self, message: Message, current_root: &Path) -> Task<Message> {
        match message {
            Message::CopyQuestExample => {
                return iced::clipboard::write(
                    include_str!("../../../docs/examples/ftb-quest-entry.json").to_owned(),
                )
            }
            Message::RegistryPicked {
                root,
                instance,
                path,
            } => {
                let Some(path) = path else {
                    self.busy = false;
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            mc_core::service::instance_write::items::import_registry(
                                &root, &instance, &path,
                            )
                            .map(|value| {
                                format!(
                                    "已导入 {} 个物品；Mod 或版本变化后请重新导入。",
                                    value["item_count"]
                                )
                            })
                            .map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::RegistryImported,
                );
            }
            Message::RegistryImported(result) => {
                self.busy = false;
                self.notice = match result {
                    Ok(message) => message,
                    Err(error) => format!("物品表导入失败：{error}"),
                };
            }
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(policy) => {
                        self.saved = policy.clone();
                        self.draft = policy;
                        self.notice = "MCP 权限已保存。已有进程会在下一次请求时读取新权限。".into();
                    }
                    Err(error) => self.notice = format!("保存失败，原有权限保持不变：{error}"),
                }
            }
            Message::Copy => match client_config() {
                Ok(config) => return iced::clipboard::write(config),
                Err(error) => self.notice = error,
            },
            _ if self.busy => {}
            Message::Enabled(value) => self.draft.enabled = value,
            Message::Permission(index, value) => match index {
                0 => self.draft.permissions.catalog = value,
                1 => self.draft.permissions.instances = value,
                2 => self.draft.permissions.logs = value,
                3 => self.draft.permissions.write_mod_list = value,
                4 => self.draft.permissions.write_mod_files = value,
                5 => self.draft.permissions.write_configs = value,
                6 => self.draft.permissions.write_quests = value,
                _ => {}
            },
            Message::Instance(id, enabled) => {
                self.draft.allowed_instances.retain(|old| old != &id);
                if enabled {
                    self.draft.allowed_instances.push(id);
                }
            }
            Message::UseCurrent => {
                self.draft.game_root =
                    std::path::absolute(current_root).unwrap_or_else(|_| current_root.into());
                self.draft.allowed_instances.clear();
                self.notice = "已切换授权实例集，请重新选择允许访问的实例并保存。".into();
            }
            Message::Save => {
                if let Err(error) = self.draft.validate() {
                    self.notice = error;
                    return Task::none();
                }
                self.busy = true;
                let policy = self.draft.clone();
                return Task::perform(
                    async move {
                        policy.save(&policy_path()).await?;
                        Ok(policy)
                    },
                    Message::Saved,
                );
            }
            Message::ImportRegistry(instance) => {
                self.busy = true;
                let root = self.draft.game_root.clone();
                return Task::perform(
                    async move {
                        let path = rfd::AsyncFileDialog::new()
                            .set_title("导入此实例实际导出的物品注册表")
                            .add_filter("Minecraft item registry", &["json"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_path_buf());
                        Message::RegistryPicked {
                            root,
                            instance,
                            path,
                        }
                    },
                    |message| message,
                );
            }
        }
        Task::none()
    }
}
pub fn server_executable() -> Result<PathBuf, String> {
    let launcher = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(launcher
        .parent()
        .ok_or("无法定位启动器目录")?
        .join(if cfg!(windows) {
            "mc-mcp.exe"
        } else {
            "mc-mcp"
        }))
}
fn client_config() -> Result<String, String> {
    let directory = std::path::absolute(policy_path().parent().ok_or("MCP 配置目录无效")?)
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&serde_json::json!({"mcpServers":{"memoria":{"command":server_executable()?, "env":{"MEMORIA_DATA_DIR":directory}}}})).map_err(|e| e.to_string())
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::McpSettings(message)
}
pub fn view(app: &Launcher) -> Element<'_, AppMessage> {
    let state = &app.mcp_settings;
    let enabled = !state.busy;
    let same = crate::local::same_root(&state.draft.game_root, &app.environment.game_root);
    let mut projects = column![].spacing(12);
    if same {
        for instance in &app.instances {
            let id = instance.instance.id.clone();
            let allowed = state.draft.allowed_instances.contains(&id);
            let registry_id = id.clone();
            projects = projects.push(
                row![
                    checkbox(allowed)
                        .label(format!(
                            "{} · {}",
                            instance.instance.name, instance.instance.mc_version
                        ))
                        .text_size(12)
                        .on_toggle_maybe(enabled.then_some(move |checked| {
                            dispatch(Message::Instance(id.clone(), checked))
                        }))
                        .width(Fill),
                    button(label("导入物品表", 11.))
                        .padding([6, 10])
                        .style(theme::quiet)
                        .on_press_maybe(
                            (enabled && allowed)
                                .then_some(dispatch(Message::ImportRegistry(registry_id)))
                        )
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        if app.instances.is_empty() {
            projects = projects.push(label("当前实例集没有已发现的实例。", 12.).color(MUTED));
        }
    } else {
        projects = projects.push(
            label(
                "授权范围属于另一个实例集。切换到当前实例集会清空原实例选择。",
                12.,
            )
            .color(MUTED),
        );
        for id in &state.draft.allowed_instances {
            projects = projects.push(label(id, 11.).color(MUTED));
        }
        projects = projects.push(
            button(label("使用当前实例集", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::UseCurrent))),
        );
    }
    let mut content = column![
        bold("Memoria MCP 服务器", 23.),
        label(
            "通过 stdio 向外部 AI 客户端开放启动器工具。此处的权限会由内置服务器执行。",
            12.
        )
        .color(MUTED),
        checkbox(state.draft.enabled)
            .label("启用内置 MCP 服务")
            .text_size(14)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Enabled(value)))),
        label("允许的权限", 14.),
        checkbox(state.draft.permissions.catalog)
            .label("查询官方游戏版本和 Modrinth 模组目录")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(0, value)))),
        checkbox(state.draft.permissions.instances)
            .label("查看已授权实例的信息")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(1, value)))),
        checkbox(state.draft.permissions.logs)
            .label("列出并读取已授权实例的日志")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(2, value)))),
        checkbox(state.draft.permissions.write_mod_list)
            .label("间接写入 Mod 列表 · 使用 Modrinth / CurseForge 项目或文件 ID")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(3, value)))),
        checkbox(state.draft.permissions.write_mod_files)
            .label("直接写入 Mod 列表 · 上传 JAR 文件（需 SHA-256）")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(4, value)))),
        checkbox(state.draft.permissions.write_configs)
            .label("写入配置文件 · config / defaultconfigs（FTB 任务除外）")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(5, value)))),
        checkbox(state.draft.permissions.write_quests)
            .label("写入 FTB 任务 · 条目、物品图标、条件、奖励与图片")
            .text_size(12)
            .on_toggle_maybe(enabled.then_some(|value| dispatch(Message::Permission(6, value)))),
        label(
            "写入权限默认关闭，且仅作用于下方选中的实例。AI 须先生成预览，再应用准确计划；覆盖前备份，文件或授权变化后重新检查。",
            11.
        )
        .color(MUTED),
        label("任务中的图标、条件和奖励使用物品 ID。请导入当前实例真实运行环境导出的注册表（registries.json 或 items 数组）；更新 Mod 后需重新导入。", 11.).color(MUTED),
        label("授权实例集", 14.),
        label(state.draft.game_root.to_string_lossy().into_owned(), 11.).color(MUTED),
        label("允许访问的实例", 14.),
        projects,
        label(
            "未选择的实例禁止读取和写入。CurseForge 需由外部客户端提供 CURSEFORGE_API_KEY；来源依赖不会自动安装。",
            11.
        )
        .color(MUTED),
    ]
    .spacing(14);
    if let Ok(executable) = server_executable() {
        content =
            content.push(label(format!("服务程序：{}", executable.display()), 10.).color(MUTED));
        if !executable.is_file() {
            content = content.push(
                label(
                    "请将 mc-mcp 服务程序放在启动器同一目录，外部客户端会负责启动它。",
                    11.,
                )
                .color(MUTED),
            );
        }
    }
    column![
        scrollable(
            container(content)
                .padding(22)
                .width(Fill)
                .style(theme::panel)
        )
        .direction(theme::scroll_direction())
        .height(Fill),
        label(&state.notice, 11.).color(MUTED),
        row![
            button(label("复制任务示例", 12.))
                .padding(12)
                .style(theme::quiet)
                .on_press(dispatch(Message::CopyQuestExample)),
            button(label("复制客户端连接配置", 12.))
                .padding(12)
                .style(theme::quiet)
                .on_press(dispatch(Message::Copy)),
            Space::new().width(Fill),
            button(label("保存 MCP 权限", 13.))
                .padding([12, 18])
                .style(theme::action)
                .on_press_maybe(enabled.then_some(dispatch(Message::Save))),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(12)
    .height(Fill)
    .into()
}
