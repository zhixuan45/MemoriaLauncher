use crate::{
    app::Message as AppMessage,
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, column, row, scrollable},
    Fill, Task,
};
use mc_core::{
    runtime::diagnostics::{self as service, Analysis},
    GameLayout, InstanceLog,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Select(String),
    Import,
    Picked(u64, Option<PathBuf>),
    Listed(u64, Result<Vec<InstanceLog>, String>),
    Analyzed(u64, Result<Analysis, String>),
    ImportRules,
    RestoreRules,
    RulesPicked(u64, Option<PathBuf>),
    RulesStored(u64, Result<String, String>),
}
#[derive(Default)]
pub struct State {
    pub root: PathBuf,
    pub instance: String,
    pub generation: u64,
    pub busy: bool,
    pub logs: Vec<InstanceLog>,
    pub selected: String,
    pub analysis: Option<Analysis>,
    pub notice: String,
    pub exit_code: Option<i32>,
    pub exit_path: String,
    pub rule_notice: String,
    imported_file: Option<PathBuf>,
}
impl State {
    pub fn open(
        &mut self,
        root: PathBuf,
        instance: String,
        selected: Option<String>,
        exit_code: Option<i32>,
    ) -> Task<Message> {
        *self = Self {
            root,
            instance,
            generation: self.generation + 1,
            exit_path: selected.clone().unwrap_or_default(),
            selected: selected.unwrap_or_default(),
            exit_code,
            ..Default::default()
        };
        if self.instance.is_empty() {
            self.notice = "选择导入文本日志即可在本地解析。".into();
            return Task::none();
        }
        self.update(Message::Refresh)
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RulesStored(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(notice) => {
                        self.rule_notice = notice;
                        if let Some(path) = self.imported_file.clone() {
                            return self.update(Message::Picked(g, Some(path)));
                        }
                        if !self.selected.is_empty() {
                            return self.update(Message::Select(self.selected.clone()));
                        }
                    }
                    Err(error) => self.rule_notice = format!("规则保存失败，原配置保留：{error}"),
                }
            }
            Message::RulesPicked(g, Some(source)) if g == self.generation => {
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            mc_core::runtime::diagnostic_rules::import_custom(&source, &rule_path())
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |result| Message::RulesStored(g, result),
                );
            }
            Message::RulesPicked(g, None) if g == self.generation => {
                self.busy = false;
                self.rule_notice = "已取消规则导入。".into();
            }
            Message::RulesStored(..) | Message::RulesPicked(..) => {}
            Message::Listed(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(logs) => {
                        self.logs = logs;
                        let path = if self.selected.is_empty() {
                            self.logs.first().map(|log| log.path.clone())
                        } else {
                            Some(self.selected.clone())
                        };
                        if let Some(path) = path {
                            return self.update(Message::Select(path));
                        }
                        self.notice = "此实例没有可读取的日志，可导入本地文本日志。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Analyzed(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(analysis) => {
                        self.analysis = Some(analysis);
                        self.notice = "本地解析完成，未发送网络请求，也未修改游戏文件。".into();
                    }
                    Err(e) => self.notice = format!("解析失败：{e}"),
                }
            }
            Message::Picked(g, Some(path)) if g == self.generation => {
                self.imported_file = Some(path.clone());
                self.selected = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.analysis = None;
                self.busy = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let rules =
                                mc_core::runtime::diagnostic_rules::RuleSet::load(&rule_path())?;
                            service::analyze_file_with_rules(&path, &rules)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())
                    },
                    move |r| Message::Analyzed(g, r),
                );
            }
            Message::Picked(g, None) if g == self.generation => {
                self.busy = false;
                self.notice = "已取消导入。".into();
            }
            Message::Listed(..) | Message::Analyzed(..) | Message::Picked(..) => {}
            Message::Refresh => {
                self.generation += 1;
                self.busy = true;
                self.analysis = None;
                self.notice = "正在读取实例日志目录…".into();
                let g = self.generation;
                let layout = match GameLayout::new(&self.root) {
                    Ok(v) => v,
                    Err(e) => {
                        self.busy = false;
                        self.notice = e.to_string();
                        return Task::none();
                    }
                };
                let instance = self.instance.clone();
                return Task::perform(
                    async move {
                        mc_core::list_instance_logs(layout, instance)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |r| Message::Listed(g, r),
                );
            }
            Message::Select(path) => {
                if !self.logs.iter().any(|log| log.path == path) {
                    self.notice = "所选日志不在当前实例日志列表中。".into();
                    return Task::none();
                }
                self.generation += 1;
                let g = self.generation;
                self.busy = true;
                self.analysis = None;
                self.selected = path.clone();
                self.imported_file = None;
                self.notice = "正在本地解析日志…".into();
                let root = self.root.clone();
                let instance = self.instance.clone();
                // A game exit code belongs only to its exact launcher log, never another historical log.
                let code = if path == self.exit_path {
                    self.exit_code
                } else {
                    None
                };
                return Task::perform(
                    async move {
                        let rules =
                            mc_core::runtime::diagnostic_rules::RuleSet::load(&rule_path())?;
                        service::analyze_instance_with_rules(
                            GameLayout::new(root).map_err(|e| e.to_string())?,
                            instance,
                            path,
                            code,
                            &rules,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    move |r| Message::Analyzed(g, r),
                );
            }
            Message::Import => {
                self.generation += 1;
                let g = self.generation;
                self.busy = true;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("文本日志", &["log", "txt"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    move |r| Message::Picked(g, r),
                );
            }
            Message::ImportRules => {
                self.generation += 1;
                let g = self.generation;
                self.busy = true;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("导入追加诊断规则")
                            .add_filter("诊断规则 JSON", &["json"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    move |path| Message::RulesPicked(g, path),
                );
            }
            Message::RestoreRules => {
                self.generation += 1;
                let g = self.generation;
                self.busy = true;
                return Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            mc_core::runtime::diagnostic_rules::restore_builtin(&rule_path())
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |result| Message::RulesStored(g, result),
                );
            }
        }
        Task::none()
    }
}
fn rule_path() -> PathBuf {
    crate::accounts::data_dir().join("diagnostic-rules.json")
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::Diagnostics(message)
}
pub fn summary(analysis: &Analysis) -> Element<'_, AppMessage> {
    let mut findings = column![label(
        format!(
            "检查 {} 行 · 遮蔽 {} 行凭据{}",
            analysis.inspected_lines,
            analysis.redacted_lines,
            if analysis.truncated {
                " · 仅检查日志末尾"
            } else {
                ""
            }
        ),
        11.
    )
    .color(MUTED)]
    .spacing(12);
    findings = findings.push(
        label(
            format!(
                "使用 {} 条规则{}",
                analysis.rules_applied,
                if analysis.rule_limit_reached {
                    " · 命中数量达到预算，可能有未列出的匹配"
                } else {
                    ""
                }
            ),
            10.,
        )
        .color(MUTED),
    );
    for finding in &analysis.findings {
        let mut card = column![
            bold(&finding.title, 14.),
            label(
                format!("{} · 置信度 {}", finding.category, finding.confidence),
                10.
            )
            .color(MUTED)
        ]
        .spacing(6);
        card = card.push(
            label(
                format!(
                    "规则 {} · 片段行 {:?}",
                    finding.rule_id, finding.source_lines
                ),
                10.,
            )
            .color(MUTED),
        );
        if let Some(id) = &finding.required_mod_id {
            card = card.push(label(
                format!(
                    "Mod {} → 前置 {} {}",
                    finding.mod_id.as_deref().unwrap_or("未提取"),
                    id,
                    finding.required_version.as_deref().unwrap_or("")
                ),
                11.,
            ));
        }
        for evidence in &finding.evidence {
            card = card.push(label(evidence, 11.).color(MUTED));
        }
        card = card.push(label(&finding.suggested_next_action, 12.));
        findings = findings.push(card);
    }
    findings.into()
}
pub fn view(state: &State) -> Element<'_, AppMessage> {
    let mut content = column![
        row![
            button(label("刷新实例日志", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press(dispatch(Message::Refresh)),
            button(label("导入文本日志", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press(dispatch(Message::Import)),
            button(label("导入诊断规则", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press(dispatch(Message::ImportRules)),
            button(label("恢复内置规则", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press(dispatch(Message::RestoreRules)),
        ]
        .spacing(10),
        label(format!("实例：{}", state.instance), 11.).color(MUTED)
    ]
    .spacing(10);
    for log in &state.logs {
        content = content.push(
            button(label(&log.path, 11.))
                .style(if log.path == state.selected {
                    theme::selected
                } else {
                    theme::quiet
                })
                .on_press(dispatch(Message::Select(log.path.clone()))),
        );
    }
    if let Some(analysis) = &state.analysis {
        content = content
            .push(bold(&state.selected, 14.))
            .push(summary(analysis));
    }
    column![
        scrollable(content)
            .direction(theme::scroll_direction())
            .height(Fill),
        label(&state.notice, 12.).color(MUTED),
        label(&state.rule_notice, 11.).color(MUTED)
    ]
    .spacing(10)
    .height(420)
    .into()
}
