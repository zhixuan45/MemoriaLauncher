//! Semantic FTB Quests SNBT authoring (1.20.1 format); never writes player progress.
use super::{
    digest, error,
    files::{self, Watch},
    invalid, io_error, items,
    snbt::{self, Node},
    PreparedChange, PreparedFile, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
const CHAPTERS: &str = "config/ftbquests/quests/chapters";
pub fn image_size() -> f64 {
    1.0
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}
impl Position {
    fn snapped(self) -> Result<Self> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || self.x.abs() > 10_000.0
            || self.y.abs() > 10_000.0
        {
            return Err(invalid("任务坐标必须在 -10000..10000 内"));
        }
        Ok(Self {
            x: (self.x * 10.0).round() / 10.0,
            y: (self.y * 10.0).round() / 10.0,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestEntry {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub title: String,
    #[serde(default, deserialize_with = "description")]
    pub description: Vec<String>,
    pub completion: Vec<Condition>,
    #[serde(default)]
    pub rewards: Vec<Reward>,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub icon: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Checkmark,
    Item {
        item: String,
        #[serde(default = "one")]
        count: u32,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reward {
    Item {
        item: String,
        #[serde(default = "one")]
        count: u32,
    },
}
fn one() -> u32 {
    1
}
fn description<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Text(String),
        Lines(Vec<String>),
    }
    Ok(match Input::deserialize(d)? {
        Input::Text(text) => text.lines().map(str::to_owned).collect(),
        Input::Lines(lines) => lines,
    })
}
fn slug(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(invalid(
            "章节和条目 ID 使用 1–80 个字母、数字、下划线或连字符",
        ));
    }
    Ok(())
}
pub fn native_id(value: &str) -> Result<String> {
    slug(value)?;
    if value.len() == 16 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        if value.bytes().all(|b| b == b'0') {
            return Err(invalid("任务 ID 不能为零"));
        }
        return Ok(value.to_ascii_uppercase());
    }
    let hash = digest(format!("memoria-ftb:{value}").as_bytes());
    let number = u64::from_str_radix(&hash[..16], 16).map_err(invalid)? & 0x7fff_ffff_ffff_ffff;
    Ok(format!("{:016X}", number.max(1)))
}
fn node_id(node: &Node) -> Result<String> {
    let id = node
        .get("id")
        .and_then(Node::text)
        .ok_or_else(|| invalid("FTB 对象缺少 id"))?;
    if id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("FTB 对象 id 不是 16 位十六进制"));
    }
    native_id(id)
}
fn text(value: &str, max: usize) -> Result<()> {
    if value.len() > max || value.contains('\0') {
        Err(invalid("标题或说明过长或包含空字符"))
    } else {
        Ok(())
    }
}
struct Chapters {
    docs: BTreeMap<String, Node>,
    watches: Vec<Watch>,
    reserved: BTreeSet<String>,
}
fn load(base: &Path) -> Result<Chapters> {
    let directory = files::safe_join(base, CHAPTERS)?;
    let mut docs = BTreeMap::new();
    let mut watches = vec![Watch::directory(base, CHAPTERS)?];
    let mut total = 0;
    if directory.exists() {
        for entry in std::fs::read_dir(directory).map_err(io_error)? {
            let name = entry
                .map_err(io_error)?
                .file_name()
                .to_str()
                .ok_or_else(|| invalid("章节文件名无效"))?
                .to_owned();
            if name.ends_with(".json5") {
                return Err(error(
                    "UNSUPPORTED_QUEST_FORMAT",
                    "此实例使用 JSON5 任务格式，当前接口仅支持 FTB SNBT",
                ));
            }
            if !name.ends_with(".snbt") {
                continue;
            }
            files::basename(&name, ".snbt")?;
            let path = format!("{CHAPTERS}/{name}");
            let watch = Watch::file(base, &path)?;
            let bytes =
                files::read_bounded(&files::safe_join(base, &path)?, files::MAX_TEXT_BYTES)?;
            if watch.hash.as_deref() != Some(&digest(&bytes)) {
                return Err(error("PLAN_STALE", "读取章节时文件发生变化"));
            }
            total += bytes.len();
            if docs.len() >= 128 || total > 8 * 1024 * 1024 {
                return Err(invalid("章节数量或总大小超出限制"));
            }
            docs.insert(
                path,
                snbt::parse(std::str::from_utf8(&bytes).map_err(invalid)?)?,
            );
            watches.push(watch);
        }
    }
    let (reserved, side_watches) = side_ids(base)?;
    watches.extend(side_watches);
    Ok(Chapters {
        docs,
        watches,
        reserved,
    })
}
fn side_ids(base: &Path) -> Result<(BTreeSet<String>, Vec<Watch>)> {
    let groups = "config/ftbquests/quests/chapter_groups.snbt";
    let directory = "config/ftbquests/quests/reward_tables";
    let mut watches = vec![Watch::directory(base, directory)?];
    let mut paths = vec![groups.to_owned()];
    let tables = files::safe_join(base, directory)?;
    if tables.exists() {
        for entry in std::fs::read_dir(tables).map_err(io_error)? {
            let name = entry
                .map_err(io_error)?
                .file_name()
                .to_str()
                .ok_or_else(|| invalid("奖励表名称无效"))?
                .to_owned();
            if name.ends_with(".snbt") {
                files::basename(&name, ".snbt")?;
                paths.push(format!("{directory}/{name}"));
            }
            if paths.len() > 129 {
                return Err(invalid("奖励表超过 128 个"));
            }
        }
    }
    let mut ids = BTreeSet::from(["0000000000000001".into(), "0000000000000002".into()]);
    let mut total = 0;
    fn collect(node: &Node, ids: &mut BTreeSet<String>) -> Result<()> {
        match node {
            Node::Compound(values) => {
                if let Some(id) = values
                    .get("id")
                    .and_then(Node::text)
                    .filter(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    if !ids.insert(native_id(id)?) {
                        return Err(invalid("FTB 分组或奖励表 ID 重复"));
                    }
                }
                for node in values.values() {
                    collect(node, ids)?;
                }
            }
            Node::List(values) => {
                for node in values {
                    collect(node, ids)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for path in paths {
        let watch = Watch::file(base, &path)?;
        if watch.hash.is_some() {
            let bytes =
                files::read_bounded(&files::safe_join(base, &path)?, files::MAX_TEXT_BYTES)?;
            total += bytes.len();
            if total > 8 * 1024 * 1024 {
                return Err(invalid("奖励表与分组超过 8 MiB"));
            }
            if watch.hash.as_deref() != Some(&digest(&bytes)) {
                return Err(error("PLAN_STALE", "读取 FTB 对象时文件发生变化"));
            }
            collect(
                &snbt::parse(std::str::from_utf8(&bytes).map_err(invalid)?)?,
                &mut ids,
            )?;
        }
        watches.push(watch);
    }
    Ok((ids, watches))
}
fn validate_graph(docs: &BTreeMap<String, Node>, reserved: &BTreeSet<String>) -> Result<()> {
    let mut ids = reserved.clone();
    let mut graph = BTreeMap::new();
    for doc in docs.values() {
        if !ids.insert(node_id(doc)?) {
            return Err(invalid("FTB 章节或条目 ID 重复"));
        }
        if let Some(quests) = doc.get("quests") {
            for quest in quests.list()? {
                let id = node_id(quest)?;
                if !ids.insert(id.clone()) {
                    return Err(invalid("FTB 条目 ID 重复"));
                }
                let dependencies = quest
                    .get("dependencies")
                    .map(Node::list)
                    .transpose()?
                    .into_iter()
                    .flatten()
                    .map(|n| native_id(n.text().ok_or_else(|| invalid("上级条目不是字符串"))?))
                    .collect::<Result<Vec<_>>>()?;
                graph.insert(id, dependencies);
                for key in ["tasks", "rewards"] {
                    if let Some(list) = quest.get(key) {
                        for item in list.list()? {
                            if !ids.insert(node_id(item)?) {
                                return Err(invalid("FTB 条件或奖励 ID 重复"));
                            }
                        }
                    }
                }
            }
        }
    }
    fn visit(
        id: &str,
        graph: &BTreeMap<String, Vec<String>>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if active.len() >= 128 {
            return Err(invalid("任务上级链超过 128 层"));
        }
        if !active.insert(id.into()) {
            return Err(invalid("任务上级关系构成循环"));
        }
        for parent in &graph[id] {
            if !graph.contains_key(parent) {
                return Err(invalid(format!("上级条目 {parent} 不存在")));
            }
            visit(parent, graph, active, done)?;
        }
        active.remove(id);
        done.insert(id.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in graph.keys() {
        visit(id, &graph, &mut BTreeSet::new(), &mut done)?
    }
    Ok(())
}
fn set(node: &mut Node, key: &str, value: Node) -> Result<()> {
    node.object_mut()?.insert(key.into(), value);
    Ok(())
}
fn chapter_path(chapter: &str) -> Result<String> {
    slug(chapter)?;
    Ok(format!("{CHAPTERS}/{chapter}.snbt"))
}
pub fn entry(
    base: &Path,
    chapter: &str,
    title: Option<&str>,
    input: &QuestEntry,
) -> Result<PreparedChange> {
    let path = chapter_path(chapter)?;
    let id = native_id(&input.id)?;
    if input.title.trim().is_empty()
        || input.completion.is_empty()
        || input.completion.len() > 32
        || input.rewards.len() > 32
        || input.description.len() > 64
    {
        return Err(invalid(
            "任务须有标题和 1–32 个条件，最多 32 个奖励与 64 行说明",
        ));
    }
    text(&input.title, 512)?;
    for line in &input.description {
        text(line, 4096)?
    }
    let position = input.position.snapped()?;
    let mut item_ids = vec![];
    if let Some(icon) = &input.icon {
        item_ids.push(icon.clone());
    }
    let mut tasks = vec![];
    let mut rewards = vec![];
    for (index, condition) in input.completion.iter().enumerate() {
        let mut node = Node::empty();
        set(
            &mut node,
            "id",
            Node::string(native_id(&format!("{}-task-{index}", id))?),
        )?;
        match condition {
            Condition::Checkmark => set(&mut node, "type", Node::string("checkmark"))?,
            Condition::Item { item, count } => {
                if *count == 0 || *count > 1_000_000 {
                    return Err(invalid("条件物品数量为 1–1000000"));
                }
                item_ids.push(item.clone());
                set(&mut node, "type", Node::string("item"))?;
                set(&mut node, "item", Node::string(item))?;
                set(&mut node, "count", Node::number(format!("{count}L")))?;
            }
        }
        tasks.push(node);
    }
    for (index, reward) in input.rewards.iter().enumerate() {
        let mut node = Node::empty();
        set(
            &mut node,
            "id",
            Node::string(native_id(&format!("{}-reward-{index}", id))?),
        )?;
        match reward {
            Reward::Item { item, count } => {
                if *count == 0 || *count > 8192 {
                    return Err(invalid("奖励物品数量为 1–8192"));
                }
                item_ids.push(item.clone());
                set(&mut node, "type", Node::string("item"))?;
                set(&mut node, "item", Node::string(item))?;
                set(&mut node, "count", Node::number(count))?;
            }
        }
        rewards.push(node);
    }
    let item_watches = items::watches(base)?;
    items::require(base, &item_ids)?;
    let mut chapters = load(base)?;
    if !chapters.docs.contains_key(&path) {
        let chapter_title = title.ok_or_else(|| invalid("创建章节须提供 chapter_title"))?;
        text(chapter_title, 512)?;
        let mut doc = Node::empty();
        set(
            &mut doc,
            "id",
            Node::string(native_id(&format!("chapter-{chapter}"))?),
        )?;
        set(&mut doc, "title", Node::string(chapter_title))?;
        set(&mut doc, "quests", Node::List(vec![]))?;
        chapters.docs.insert(path.clone(), doc);
    }
    let doc = chapters.docs.get_mut(&path).unwrap();
    doc.object_mut()?
        .entry("quests".into())
        .or_insert(Node::List(vec![]));
    let quests = doc.object_mut()?.get_mut("quests").unwrap().list_mut()?;
    let existing = quests
        .iter()
        .position(|quest| node_id(quest).ok().as_deref() == Some(id.as_str()));
    let mut quest = existing
        .map(|i| quests[i].clone())
        .unwrap_or_else(Node::empty);
    let parent = input.parent.as_deref().map(native_id).transpose()?;
    for (key, value) in [
        ("id", Node::string(&id)),
        ("title", Node::string(&input.title)),
        (
            "description",
            Node::List(input.description.iter().map(Node::string).collect()),
        ),
        (
            "dependencies",
            Node::List(parent.iter().map(Node::string).collect()),
        ),
        ("x", Node::number(position.x)),
        ("y", Node::number(position.y)),
        ("tasks", Node::List(tasks)),
        ("rewards", Node::List(rewards)),
    ] {
        set(&mut quest, key, value)?;
    }
    if let Some(icon) = &input.icon {
        set(&mut quest, "icon", Node::string(format!("item:{icon}")))?;
    } else {
        quest.object_mut()?.remove("icon");
    }
    if let Some(i) = existing {
        quests[i] = quest;
    } else {
        quests.push(quest);
    }
    validate_graph(&chapters.docs, &chapters.reserved)?;
    let bytes = chapters.docs[&path].render().into_bytes();
    if bytes.len() > files::MAX_TEXT_BYTES {
        return Err(invalid("章节超过 1 MiB"));
    }
    chapters.watches.extend(item_watches);
    Ok(PreparedChange{files:vec![PreparedFile{path,bytes}],watches:chapters.watches,warnings:vec!["FTB Quests 1.20.1 SNBT 格式；保留未知字段，重新排版会移除注释。修改后请在游戏中重载任务。".into()],details:json!({"entry_id":id,"parent":parent,"position":position,"grid_step":0.1,"origin":[0,0]})})
}
pub fn image(
    base: &Path,
    chapter: &str,
    filename: &str,
    bytes: Vec<u8>,
    position: Position,
    width: f64,
    height: f64,
) -> Result<PreparedChange> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !["png", "jpg", "jpeg"].contains(&extension.as_str()) {
        return Err(invalid("图片须为 PNG 或 JPEG"));
    }
    files::basename(filename, &format!(".{extension}"))?;
    if bytes.is_empty() || bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid("图片大小为 1 字节至 4 MiB"));
    }
    let format = image::guess_format(&bytes).map_err(invalid)?;
    if !matches!(
        (format, extension.as_str()),
        (image::ImageFormat::Png, "png") | (image::ImageFormat::Jpeg, "jpg" | "jpeg")
    ) {
        return Err(invalid("图片内容与扩展名不符"));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(invalid)?;
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > 100.0
        || height > 100.0
    {
        return Err(invalid("图片宽高为 0–100 个任务网格单位"));
    }
    let position = position.snapped()?;
    let path = chapter_path(chapter)?;
    let mut chapters = load(base)?;
    let doc = chapters
        .docs
        .get_mut(&path)
        .ok_or_else(|| invalid("请先创建章节再插入图片"))?;
    let image_path = format!("config/ftbquests/quests/images/{filename}");
    let target = files::safe_join(base, &image_path)?;
    let uri = url::Url::from_file_path(std::path::absolute(target).map_err(io_error)?)
        .map_err(|_| invalid("图片文件 URI 无效"))?
        .to_string();
    let mut node = Node::empty();
    for (key, value) in [
        ("image", Node::string(&uri)),
        ("x", Node::number(position.x)),
        ("y", Node::number(position.y)),
        ("width", Node::number(width)),
        ("height", Node::number(height)),
    ] {
        set(&mut node, key, value)?;
    }
    let images = doc
        .object_mut()?
        .entry("images".into())
        .or_insert(Node::List(vec![]))
        .list_mut()?;
    if let Some(i) = images
        .iter()
        .position(|image| image.get("image").and_then(Node::text) == Some(uri.as_str()))
    {
        images[i] = node;
    } else {
        images.push(node);
    }
    let bytes_chapter = doc.render().into_bytes();
    if bytes_chapter.len() > files::MAX_TEXT_BYTES {
        return Err(invalid("章节超过 1 MiB"));
    }
    Ok(PreparedChange {
        files: vec![
            PreparedFile {
                path,
                bytes: bytes_chapter,
            },
            PreparedFile {
                path: image_path,
                bytes,
            },
        ],
        watches: chapters.watches,
        warnings: vec!["图片使用此实例的本地 file: URI；移动实例目录后需重新插入图片。".into()],
        details: json!({"position":position,"width":width,"height":height,"grid_step":0.1}),
    })
}
pub fn read(base: &Path) -> Result<Value> {
    let chapters = load(base)?;
    let mut output = vec![];
    for (path, doc) in &chapters.docs {
        let mut entries = vec![];
        if let Some(quests) = doc.get("quests") {
            for quest in quests.list()? {
                let mut entry = serde_json::Map::new();
                for key in [
                    "id",
                    "title",
                    "description",
                    "dependencies",
                    "icon",
                    "x",
                    "y",
                    "tasks",
                    "rewards",
                ] {
                    if let Some(node) = quest.get(key) {
                        entry.insert(key.into(), plain(node));
                    }
                }
                entries.push(Value::Object(entry));
            }
        }
        output.push(json!({"path":path,"id":doc.get("id").map(plain),"title":doc.get("title").map(plain),"entries":entries,"images":doc.get("images").map(plain)}));
    }
    Ok(json!({"chapters":output,"grid_step":0.1,"origin":[0,0],"untrusted_data":true}))
}
fn plain(node: &Node) -> Value {
    match node {
        Node::String(v) => json!(v),
        Node::Number(v) => json!(v),
        Node::List(v) | Node::Array(_, v) => Value::Array(v.iter().map(plain).collect()),
        Node::Compound(v) => Value::Object(v.iter().map(|(k, v)| (k.clone(), plain(v))).collect()),
    }
}
