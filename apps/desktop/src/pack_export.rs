use crate::{
    app::{Launcher, Message as AppMessage, Modal},
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, checkbox, column, row, scrollable, text_input},
    Fill, Task,
};
use mc_core::service::packs::{
    exchange::{self, ExportPlan},
    lockfile::{self, Lock},
};
use std::path::PathBuf;
#[derive(Debug, Clone)]
pub enum Message {
    Name(String),
    Configs(bool),
    Capture,
    Verify,
    ExportPreview,
    SaveExport,
    ExportPicked(u64, Option<PathBuf>),
    Captured(u64, Result<Lock, String>),
    Prepared(u64, Result<ExportPlan, String>),
    Done(u64, Result<String, String>),
}
#[derive(Default)]
pub struct State {
    pub root: PathBuf,
    pub key: String,
    pub generation: u64,
    pub busy: bool,
    pub name: String,
    pub configs: bool,
    pub lock: Option<Lock>,
    pub export: Option<ExportPlan>,
    pub notice: String,
}
impl State {
    pub fn open(&mut self, root: PathBuf, key: String, name: String) {
        *self = Self {
            root,
            key,
            name,
            generation: self.generation + 1,
            notice: "锁定记录包含内容哈希及来源；导出前可选择是否包含配置。".into(),
            ..Default::default()
        };
    }
    pub fn update(&mut self, message: Message, blocked: bool) -> Task<Message> {
        match message {
            Message::Captured(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(lock) => {
                        self.lock = Some(lock);
                        self.notice = "锁定记录已保存；手动变更后可验证或重新生成。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Prepared(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(export) => {
                        self.export = Some(export);
                        self.notice = "核对下载引用及嵌入文件后，选择新文件名导出。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Done(g, result) if g == self.generation => {
                self.busy = false;
                self.notice = match result {
                    Ok(message) => message,
                    Err(e) => format!("操作失败：{e}"),
                };
            }
            Message::ExportPicked(g, path) if g == self.generation => {
                let Some(path) = path else {
                    self.busy = false;
                    return Task::none();
                };
                let Some(export) = self.export.clone() else {
                    self.busy = false;
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        exchange::export(export, &path)
                            .await
                            .map(|_| format!("整合包已导出：{}", path.display()))
                    },
                    move |result| Message::Done(g, result),
                );
            }
            Message::Captured(..)
            | Message::Prepared(..)
            | Message::Done(..)
            | Message::ExportPicked(..) => {}
            _ if self.busy || blocked => {}
            Message::Name(name) => {
                self.generation += 1;
                self.name = name;
                self.export = None;
            }
            Message::Configs(value) => {
                self.generation += 1;
                self.configs = value;
                self.export = None;
            }
            Message::Capture => {
                self.busy = true;
                let (root, key, g) = (self.root.clone(), self.key.clone(), self.generation);
                return Task::perform(
                    async move {
                        let lock = lockfile::capture(&root, &key, true).await?;
                        lockfile::save(&root, &key, &lock).await?;
                        Ok(lock)
                    },
                    move |result| Message::Captured(g, result),
                );
            }
            Message::Verify => {
                self.busy = true;
                let (root, key, g) = (self.root.clone(), self.key.clone(), self.generation);
                return Task::perform(
                    async move {
                        let lock = lockfile::read(&root, &key)?;
                        let report = lockfile::verify(&root, &key, &lock).await?;
                        Ok(if report.matches {
                            "锁定验证通过；环境和内容一致。".into()
                        } else {
                            format!("锁定不一致：缺失 {}、变化 {}、额外 {}、环境变化 {}；不会自动修改文件。",report.missing.len(),report.changed.len(),report.extra.len(),report.environment_changed)
                        })
                    },
                    move |result| Message::Done(g, result),
                );
            }
            Message::ExportPreview => {
                self.busy = true;
                let (root, key, name, configs, g) = (
                    self.root.clone(),
                    self.key.clone(),
                    self.name.clone(),
                    self.configs,
                    self.generation,
                );
                return Task::perform(
                    exchange::preview_export(root, key, name, configs),
                    move |result| Message::Prepared(g, result),
                );
            }
            Message::SaveExport => {
                if self.export.is_none() {
                    return Task::none();
                }
                self.busy = true;
                let g = self.generation;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("导出为新的 Modrinth 整合包文件")
                            .set_file_name("instance.mrpack")
                            .add_filter("Modrinth 整合包", &["mrpack"])
                            .save_file()
                            .await
                            .map(|f| f.path().to_owned())
                    },
                    move |path| Message::ExportPicked(g, path),
                );
            }
        }
        Task::none()
    }
}
pub fn open_app(app: &mut Launcher) -> Task<AppMessage> {
    if app.instance_writing()
        || app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
    {
        return Task::none();
    }
    if let Some(entry) = app.page_instance() {
        let (key, name) = (entry.instance.id.clone(), entry.instance.name.clone());
        app.pack_export
            .open(app.environment.game_root.clone(), key, name);
        app.open_modal(Modal::PackExport);
    }
    Task::none()
}
pub fn update_app(app: &mut Launcher, message: Message) -> Task<AppMessage> {
    let blocked = app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
        || app.instance_settings.busy
        || app.resource_install.busy
        || app.loader_install.installing
        || app.change_plans.busy
        || app.instance_wizard.installing
        || !crate::local::same_root(&app.pack_export.root, &app.environment.game_root);
    app.pack_export
        .update(message, blocked)
        .map(AppMessage::PackExport)
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::PackExport(message)
}
pub fn view(state: &State, blocked: bool) -> Element<'_, AppMessage> {
    let enabled = !state.busy && !blocked;
    let mut body = column![
        label(
            "锁定校验覆盖版本声明及游戏内容目录；共享依赖完整性仍由启动检查负责。",
            11.
        )
        .color(MUTED),
        label(format!("{} · {}", state.key, state.root.display()), 11.).color(MUTED),
        row![
            button(label("生成或刷新锁定文件", 12.))
                .style(theme::quiet)
                .padding(9)
                .on_press_maybe(enabled.then_some(dispatch(Message::Capture))),
            button(label("验证现有锁定", 12.))
                .style(theme::quiet)
                .padding(9)
                .on_press_maybe(enabled.then_some(dispatch(Message::Verify)))
        ]
        .spacing(8),
        label("整合包名称", 12.),
        text_input("整合包名称", &state.name)
            .padding(9)
            .on_input_maybe(enabled.then_some(|v| dispatch(Message::Name(v)))),
        checkbox(state.configs)
            .label("包含配置与脚本（请检查其中的私人设置）")
            .on_toggle_maybe(enabled.then_some(|v| dispatch(Message::Configs(v))))
    ]
    .spacing(12);
    if let Some(lock) = &state.lock {
        body = body.push(label(
            format!(
                "已记录 {} 个文件 · Minecraft {}",
                lock.files.len(),
                lock.minecraft
            ),
            12.,
        ));
    }
    if let Some(plan) = &state.export {
        body = body
            .push(bold(
                format!(
                    "{} · {}",
                    plan.name,
                    if plan.bytes < 1048576 {
                        format!("{} KiB", plan.bytes.div_ceil(1024))
                    } else {
                        format!("{:.1} MiB", plan.bytes as f64 / 1048576.)
                    }
                ),
                16.,
            ))
            .push(label(
                format!(
                    "{} 个下载引用 · {} 个本地覆盖文件",
                    plan.download_files, plan.embedded_files
                ),
                12.,
            ));
        for warning in &plan.warnings {
            body = body.push(label(warning, 11.).color(MUTED));
        }
        for file in &plan.lock.files {
            body = body.push(label(
                format!("{} · {} KiB", file.path, file.size.div_ceil(1024)),
                11.,
            ));
        }
    }
    column![
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
        row![
            button(label("生成导出预览", 12.))
                .style(theme::quiet)
                .padding(10)
                .on_press_maybe(enabled.then_some(dispatch(Message::ExportPreview))),
            button(label("选择文件并导出", 12.))
                .style(theme::action)
                .padding(10)
                .on_press_maybe(
                    (enabled && state.export.is_some()).then_some(dispatch(Message::SaveExport))
                )
        ]
        .spacing(10),
        label(&state.notice, 11.).color(MUTED)
    ]
    .spacing(12)
    .height(Fill)
    .into()
}
