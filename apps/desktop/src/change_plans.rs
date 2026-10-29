//! Local, explicit approval of persisted instance plans and transaction history.
use crate::{
    app::Message as AppMessage,
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, column, row, scrollable},
    Fill, Task,
};
use mc_core::service::{
    file_transaction,
    instance_write::{self, LocalPlan, LocalReview},
};
use std::path::PathBuf;
#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Review(String),
    Approve,
    CancelPlan,
    Recover,
    Listed(
        u64,
        Result<(Vec<LocalPlan>, Vec<file_transaction::Summary>), String>,
    ),
    Reviewed(u64, Result<LocalReview, String>),
    Done(u64, Result<String, String>),
}
#[derive(Default)]
pub struct State {
    pub root: PathBuf,
    pub instance: String,
    pub generation: u64,
    pub busy: bool,
    pub plans: Vec<LocalPlan>,
    pub history: Vec<file_transaction::Summary>,
    pub selected: Option<LocalReview>,
    pub notice: String,
}
impl State {
    pub fn open(&mut self, root: PathBuf, instance: String) -> Task<Message> {
        *self = Self {
            root,
            instance,
            generation: self.generation + 1,
            ..Default::default()
        };
        self.notice = "请选择计划审阅；批准只作用于当前计划，不修改 MCP 授权。".into();
        self.refresh()
    }
    fn refresh(&mut self) -> Task<Message> {
        self.busy = true;
        let (root, key, g) = (self.root.clone(), self.instance.clone(), self.generation);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    Ok((
                        instance_write::list_local_plans(&root, &key).map_err(|e| e.to_string())?,
                        file_transaction::history(&root, &key).map_err(|e| e.to_string())?,
                    ))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|result| result)
            },
            move |result| Message::Listed(g, result),
        )
    }
    pub fn update(&mut self, message: Message, blocked: bool) -> Task<Message> {
        match message {
            Message::Listed(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok((plans, history)) => {
                        self.plans = plans;
                        self.history = history;
                    }
                    Err(error) => self.notice = format!("读取计划失败：{error}"),
                }
            }
            Message::Reviewed(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(review) => {
                        self.selected = Some(review);
                        self.notice = "核对文件内容、哈希和警告后，再批准本次计划。".into();
                    }
                    Err(error) => self.notice = format!("审阅失败：{error}"),
                }
            }
            Message::Done(g, result) if g == self.generation => {
                self.busy = false;
                self.selected = None;
                self.notice = match result {
                    Ok(notice) => notice,
                    Err(error) => format!("操作失败：{error}"),
                };
                return self.refresh();
            }
            Message::Listed(..) | Message::Reviewed(..) | Message::Done(..) => {}
            _ if self.busy || blocked => {}
            Message::Refresh => {
                self.selected = None;
                return self.refresh();
            }
            Message::Review(id) => {
                if !self.plans.iter().any(|plan| plan.id == id) {
                    return Task::none();
                }
                self.busy = true;
                self.selected = None;
                let (root, key, g) = (self.root.clone(), self.instance.clone(), self.generation);
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            instance_write::review_local_plan(&root, &key, &id)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                    },
                    move |result| Message::Reviewed(g, result),
                );
            }
            Message::Approve | Message::CancelPlan => {
                let approve = matches!(message, Message::Approve);
                let Some(review) = self.selected.clone() else {
                    return Task::none();
                };
                if approve && review.plan.state != "pending" {
                    return Task::none();
                }
                if !approve
                    && !["pending", "stale", "expired"].contains(&review.plan.state.as_str())
                {
                    return Task::none();
                }
                self.busy = true;
                let (root, key, g) = (self.root.clone(), self.instance.clone(), self.generation);
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            if approve {
                                instance_write::apply_local_plan(
                                    &root,
                                    &key,
                                    &review.plan.id,
                                    &review.plan.hash,
                                )
                                .map(|_| "本次计划已应用；原文件备份及事务记录已保留。".into())
                            } else {
                                instance_write::cancel_local_plan(
                                    &root,
                                    &key,
                                    &review.plan.id,
                                    &review.plan.hash,
                                )
                                .map(|_| "计划已取消，实例内容未修改。".into())
                            }
                            .map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                    },
                    move |result| Message::Done(g, result),
                );
            }
            Message::Recover => {
                self.busy = true;
                let (root, key, g) = (self.root.clone(), self.instance.clone(), self.generation);
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            file_transaction::recover(&root, &key)
                                .map(|report| {
                                    format!(
                                        "已恢复 {} 个中断事务；遇到外部修改时会停止并保留现场。",
                                        report.rolled_back.len()
                                    )
                                })
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                    },
                    move |result| Message::Done(g, result),
                );
            }
        }
        Task::none()
    }
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::ChangePlans(message)
}
pub fn open_app(app: &mut crate::app::Launcher) -> Task<AppMessage> {
    if app.instance_writing()
        || app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
    {
        return Task::none();
    }
    if let Some(entry) = app.page_instance() {
        let key = entry.instance.id.clone();
        let task = app
            .change_plans
            .open(app.environment.game_root.clone(), key)
            .map(AppMessage::ChangePlans);
        app.open_modal(crate::app::Modal::ChangePlans);
        return task;
    }
    Task::none()
}
pub fn update_app(app: &mut crate::app::Launcher, message: Message) -> Task<AppMessage> {
    let refresh = matches!(&message,Message::Done(g,Ok(_)) if *g==app.change_plans.generation);
    let blocked = app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
        || app.instance_settings.busy
        || app.resource_install.busy
        || app.loader_install.installing
        || app.instance_wizard.installing
        || app.pack_export.busy
        || !crate::local::same_root(&app.change_plans.root, &app.environment.game_root);
    let task = app
        .change_plans
        .update(message, blocked)
        .map(AppMessage::ChangePlans);
    if refresh {
        return Task::batch([task, app.update(AppMessage::Refresh)]);
    }
    task
}
pub fn view(state: &State, blocked: bool) -> Element<'_, AppMessage> {
    let enabled = !state.busy && !blocked;
    let mut body = column![
        label(
            format!("{} · {}", state.instance, state.root.display()),
            11.
        )
        .color(MUTED),
        row![
            button(label("刷新计划", 12.))
                .style(theme::quiet)
                .padding(9)
                .on_press_maybe(enabled.then_some(dispatch(Message::Refresh))),
            button(label("重试恢复中断事务", 12.))
                .style(theme::quiet)
                .padding(9)
                .on_press_maybe(enabled.then_some(dispatch(Message::Recover)))
        ]
        .spacing(8),
        label("批准仅应用所审阅的文件及哈希；不会扩大外部 MCP 权限。", 11.).color(MUTED)
    ]
    .spacing(12);
    let mut actions = row![];
    if let Some(review) = &state.selected {
        body = body
            .push(bold(
                format!(
                    "{} · {}",
                    kind(review.plan.kind),
                    status(&review.plan.state)
                ),
                15.,
            ))
            .push(label(format!("计划 {}", review.plan.id), 10.).color(MUTED))
            .push(label(format!("SHA-256 {}", review.plan.hash), 10.).color(MUTED));
        for warning in &review.preview.warnings {
            body = body.push(label(warning, 11.).color(MUTED));
        }
        for file in &review.preview.files {
            body = body
                .push(bold(
                    format!(
                        "{} · {} 字节",
                        file["path"].as_str().unwrap_or(""),
                        file["size"]
                    ),
                    12.,
                ))
                .push(
                    label(
                        format!("旧 {}", file["old_sha256"].as_str().unwrap_or("不存在")),
                        10.,
                    )
                    .color(MUTED),
                )
                .push(
                    label(format!("新 {}", file["sha256"].as_str().unwrap_or("")), 10.)
                        .color(MUTED),
                );
            if let Some(text) = file["text_preview"].as_str() {
                body = body.push(label(text.chars().take(1200).collect::<String>(), 11.));
                if text.chars().count() > 1200 || file["preview_truncated"] == true {
                    body = body.push(
                        label("内容预览已截断；批准以完整文件的上述 SHA-256 为准。", 10.)
                            .color(MUTED),
                    );
                }
            }
        }
        actions = row![
            button(label("批准并应用本次计划", 12.))
                .style(theme::action)
                .padding(10)
                .on_press_maybe(
                    (enabled && review.plan.state == "pending")
                        .then_some(dispatch(Message::Approve))
                ),
            button(label("取消此计划", 12.))
                .style(theme::quiet)
                .padding(10)
                .on_press_maybe(
                    (enabled
                        && ["pending", "stale", "expired"].contains(&review.plan.state.as_str()))
                    .then_some(dispatch(Message::CancelPlan))
                )
        ]
        .spacing(10);
    } else if state.plans.is_empty() {
        body = body.push(label("此实例没有已保存的变更计划。", 12.).color(MUTED));
    }
    for plan in &state.plans {
        body = body.push(
            button(label(
                format!(
                    "{} · {} 个文件 · {} · {}",
                    kind(plan.kind),
                    plan.files,
                    status(&plan.state),
                    plan.id
                ),
                11.,
            ))
            .padding(9)
            .style(theme::quiet)
            .on_press_maybe(enabled.then_some(dispatch(Message::Review(plan.id.clone())))),
        );
    }
    body = body.push(bold("事务记录", 14.));
    if state.history.is_empty() {
        body = body.push(label("暂无公共事务记录。", 11.).color(MUTED));
    }
    for entry in state.history.iter().take(30) {
        body = body.push(
            label(
                format!(
                    "{} · {} · {} 个文件 · {}",
                    entry.id,
                    status(&entry.state),
                    entry.files,
                    entry.origin
                ),
                10.,
            )
            .color(MUTED),
        );
    }
    column![
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
        actions,
        label(&state.notice, 11.).color(MUTED)
    ]
    .spacing(12)
    .height(Fill)
    .into()
}
fn kind(kind: instance_write::WriteKind) -> &'static str {
    match kind {
        instance_write::WriteKind::Config => "配置",
        instance_write::WriteKind::ModFile => "JAR 文件",
        instance_write::WriteKind::ModList => "来源 Mod 列表",
        instance_write::WriteKind::Quest => "FTB 任务",
    }
}
fn status(status: &str) -> &str {
    match status {
        "pending" => "待批准",
        "applied" | "committed" => "已提交",
        "rolled_back" => "已回滚",
        "interrupted" => "中断待检查",
        "expired" => "已过期",
        "stale" => "实例或文件已变化",
        "cancelled" => "已取消",
        other => other,
    }
}
