//! Modrinth search and version inspection. Catalogue requests never install files.
use crate::motion_renderer::Element;
use crate::theme::{self, bold, label, GREEN, MUTED};
use iced::widget::{button, column, container, pick_list, row, scrollable, text_input, Space};
use iced::{Alignment, Fill, Task};
use mc_core::{
    ResourceCatalog, ResourceKind, ResourcePage, ResourceProject, ResourceQuery, ResourceVersion,
};

const LOADERS: [&str; 6] = [
    "全部加载器",
    "Fabric",
    "Forge",
    "NeoForge",
    "Quilt",
    "LiteLoader",
];
#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    Version(String),
    Loader(&'static str),
    Search,
    Page(bool),
    Loaded(u64, Result<ResourcePage, String>),
    Select(String),
    Versions(u64, String, Result<Vec<ResourceVersion>, String>),
    CopyUrl,
}
pub struct State {
    pub query: String,
    version: String,
    loader: &'static str,
    kind: ResourceKind,
    applied: Option<ResourceQuery>,
    pub page: Option<ResourcePage>,
    pub loading: bool,
    pub notice: String,
    pub selected: Option<String>,
    pub versions: Vec<ResourceVersion>,
    detail_loading: bool,
    detail_notice: String,
    generation: u64,
    detail_generation: u64,
    client: Result<ResourceCatalog, String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            query: String::new(),
            version: String::new(),
            loader: LOADERS[0],
            kind: ResourceKind::Mod,
            applied: None,
            page: None,
            loading: false,
            notice: String::new(),
            selected: None,
            versions: vec![],
            detail_loading: false,
            detail_notice: String::new(),
            generation: 0,
            detail_generation: 0,
            client: ResourceCatalog::new().map_err(|e| e.to_string()),
        }
    }
}
impl State {
    #[cfg(test)]
    pub(crate) fn test_generation(&self) -> u64 {
        self.generation
    }
    fn draft(&self, offset: usize) -> ResourceQuery {
        ResourceQuery {
            kind: self.kind,
            query: self.query.trim().into(),
            game_version: self.version.trim().into(),
            loader: if self.loader == LOADERS[0] {
                String::new()
            } else {
                self.loader.to_lowercase()
            },
            offset,
        }
    }
    pub fn category(&mut self, category: usize) -> Task<Message> {
        let kind = match category {
            2 => ResourceKind::Modpack,
            3 => ResourceKind::Resourcepack,
            4 => ResourceKind::Shader,
            _ => ResourceKind::Mod,
        };
        if kind != self.kind {
            self.kind = kind;
            self.loader = LOADERS[0];
            return self.search(self.draft(0));
        }
        self.ensure_loaded()
    }
    pub fn ensure_loaded(&mut self) -> Task<Message> {
        if self.loading || self.page.is_some() {
            Task::none()
        } else {
            self.search(self.draft(0))
        }
    }
    fn search(&mut self, query: ResourceQuery) -> Task<Message> {
        self.generation += 1;
        self.detail_generation += 1;
        self.selected = None;
        self.versions.clear();
        self.detail_loading = false;
        self.page = None;
        self.loading = true;
        self.notice = "正在查询 Modrinth…".into();
        self.applied = Some(query.clone());
        let generation = self.generation;
        let client = self.client.clone();
        Task::perform(
            async move { client?.search(&query).await.map_err(|e| e.to_string()) },
            move |result| Message::Loaded(generation, result),
        )
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Query(value) => self.query = value,
            Message::Version(value) => self.version = value,
            Message::Loader(value) => self.loader = value,
            Message::Search => return self.search(self.draft(0)),
            Message::Page(next) if !self.loading => {
                if let (Some(page), Some(mut query)) = (&self.page, self.applied.clone()) {
                    if next && page.offset + page.hits.len() < page.total_hits {
                        query.offset = page.offset + ResourceQuery::PAGE_SIZE;
                    } else if !next && page.offset > 0 {
                        query.offset = page.offset.saturating_sub(ResourceQuery::PAGE_SIZE);
                    } else {
                        return Task::none();
                    }
                    return self.search(query);
                }
            }
            Message::Page(_) => {}
            Message::Loaded(generation, result) if generation == self.generation => {
                self.loading = false;
                match result {
                    Ok(page) => {
                        self.notice = format!("Modrinth · 共 {} 个结果", page.total_hits);
                        self.page = Some(page);
                    }
                    Err(error) => self.notice = format!("查询失败：{error}。点击搜索重试。"),
                }
            }
            Message::Loaded(..) => {}
            Message::Select(id) => {
                if !self
                    .page
                    .as_ref()
                    .is_some_and(|p| p.hits.iter().any(|h| h.project_id == id))
                {
                    return Task::none();
                }
                self.selected = Some(id.clone());
                self.versions.clear();
                self.detail_loading = true;
                self.detail_notice = "正在读取版本文件…".into();
                self.detail_generation += 1;
                let generation = self.detail_generation;
                let query = self.applied.clone().expect("results have a query");
                let client = self.client.clone();
                let selected = id.clone();
                return Task::perform(
                    async move {
                        client?
                            .versions(&id, &query.game_version, &query.loader)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |result| Message::Versions(generation, selected.clone(), result),
                );
            }
            Message::Versions(generation, id, result) if generation == self.detail_generation => {
                if self.selected.as_deref() != Some(&id) {
                    return Task::none();
                }
                self.detail_loading = false;
                match result {
                    Ok(versions) => {
                        self.versions = versions;
                        self.detail_notice = format!("{} 个匹配版本", self.versions.len());
                    }
                    Err(error) => {
                        self.detail_notice = format!("版本读取失败：{error}。重新选择项目可重试。")
                    }
                }
            }
            Message::Versions(..) => {}
            Message::CopyUrl => {
                if let Some(project) = self.project() {
                    let mut url = url::Url::parse("https://modrinth.com/").expect("constant URL");
                    url.path_segments_mut()
                        .expect("base URL")
                        .pop_if_empty()
                        .push(self.kind.as_str())
                        .push(&project.slug);
                    return iced::clipboard::write(url.to_string());
                }
            }
        }
        Task::none()
    }
    fn project(&self) -> Option<&ResourceProject> {
        self.page
            .as_ref()?
            .hits
            .iter()
            .find(|p| Some(&p.project_id) == self.selected.as_ref())
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    let enabled = !state.loading;
    let filters = row![
        text_input("Minecraft 版本（可留空）", &state.version)
            .on_input(Message::Version)
            .on_submit(Message::Search)
            .padding(10)
            .size(12),
        pick_list(LOADERS, Some(state.loader), Message::Loader).text_size(12),
        button(label("搜索", 12.))
            .padding(10)
            .style(theme::action)
            .on_press_maybe(enabled.then_some(Message::Search)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let mut list = column![].spacing(8);
    if let Some(page) = &state.page {
        for project in &page.hits {
            list = list.push(
                button(
                    column![
                        bold(&project.title, 16.),
                        label(&project.description, 12.).color(MUTED),
                        label(
                            format!("{} · {} 次下载", project.author, project.downloads),
                            10.
                        )
                        .color(GREEN),
                    ]
                    .spacing(6),
                )
                .padding(12)
                .width(Fill)
                .style(if state.selected.as_deref() == Some(&project.project_id) {
                    theme::selected
                } else {
                    theme::quiet
                })
                .on_press(Message::Select(project.project_id.clone())),
            );
        }
        if page.hits.is_empty() {
            list =
                list.push(label("没有匹配的资源，请调整关键词、版本或加载器。", 13.).color(MUTED));
        }
    }
    let (previous, next, page_number) = state
        .page
        .as_ref()
        .map(|p| {
            (
                p.offset > 0,
                p.offset + p.hits.len() < p.total_hits,
                format!("第 {} 页", p.offset / ResourceQuery::PAGE_SIZE + 1),
            )
        })
        .unwrap_or((false, false, String::new()));
    let left = container(
        column![
            scrollable(list)
                .direction(theme::scroll_direction())
                .height(Fill),
            row![
                button(label("上一页", 12.))
                    .padding(8)
                    .style(theme::quiet)
                    .on_press_maybe((enabled && previous).then_some(Message::Page(false))),
                Space::new().width(Fill),
                label(page_number, 11.).color(MUTED),
                Space::new().width(Fill),
                button(label("下一页", 12.))
                    .padding(8)
                    .style(theme::quiet)
                    .on_press_maybe((enabled && next).then_some(Message::Page(true))),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(12)
    .width(iced::Length::FillPortion(5))
    .height(Fill)
    .style(theme::panel);
    column![
        filters,
        label(&state.notice, 11.).color(MUTED),
        row![left, details(state)].spacing(12).height(Fill)
    ]
    .spacing(10)
    .height(Fill)
    .into()
}
fn details(state: &State) -> Element<'_, Message> {
    let mut content = column![].spacing(12);
    if let Some(project) = state.project() {
        content = content
            .push(bold(&project.title, 24.))
            .push(label(&project.description, 13.))
            .push(label(format!("分类：{}", project.categories.join(" / ")), 11.).color(MUTED))
            .push(
                label(
                    format!(
                        "支持 {} 个 Minecraft 版本，可按版本筛选文件。",
                        project.versions.len()
                    ),
                    11.,
                )
                .color(MUTED),
            )
            .push(
                button(label("复制项目链接", 12.))
                    .padding(8)
                    .style(theme::quiet)
                    .on_press(Message::CopyUrl),
            )
            .push(label(&state.detail_notice, 12.).color(GREEN));
        // Bound widget construction; the full filtered response is still counted above.
        for version in state.versions.iter().take(30) {
            let mut metadata = column![
                bold(&version.name, 15.),
                label(
                    format!(
                        "{} · {} · {}",
                        version.version_number,
                        version.version_type,
                        version.loaders.join(" / ")
                    ),
                    11.
                )
                .color(GREEN),
                label(
                    format!("Minecraft {}", version.game_versions.join(" / ")),
                    11.
                )
                .color(MUTED),
                label(format!("版本 ID：{}", version.id), 10.).color(MUTED),
            ]
            .spacing(6);
            for file in &version.files {
                metadata = metadata.push(label(
                    format!("{} · {} KiB", file.filename, file.size.div_ceil(1024)),
                    11.,
                ));
            }
            for dependency in &version.dependencies {
                metadata = metadata.push(
                    label(
                        format!(
                            "依赖 {} · 项目 {} · 版本 {}",
                            dependency.dependency_type,
                            dependency.project_id.as_deref().unwrap_or("未指定"),
                            dependency.version_id.as_deref().unwrap_or("未指定")
                        ),
                        10.,
                    )
                    .color(MUTED),
                );
            }
            content = content.push(
                container(metadata)
                    .padding(12)
                    .width(Fill)
                    .style(theme::surface),
            );
        }
        if state.versions.len() > 30 {
            content = content.push(
                label("仅显示前 30 个版本，可填写 Minecraft 版本缩小范围。", 11.).color(MUTED),
            );
        }
        content = content.push(
            label(
                "当前提供资源查询与版本文件预览；安装与依赖部署尚未接入。",
                11.,
            )
            .color(MUTED),
        );
    } else {
        content = content.push(label("选择资源查看版本、文件及依赖。", 15.).color(MUTED));
    }
    container(
        scrollable(content)
            .direction(theme::scroll_direction())
            .height(Fill),
    )
    .padding(16)
    .width(iced::Length::FillPortion(6))
    .height(Fill)
    .style(theme::panel)
    .into()
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
