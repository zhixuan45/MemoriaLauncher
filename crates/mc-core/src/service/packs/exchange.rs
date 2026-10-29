use super::{
    lockfile::{self, Lock},
    *,
};
use crate::service::{
    instance_write::{digest, files},
    resource_install,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
pub const MAX_ARCHIVE: usize = 64 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct Pack {
    pub name: String,
    pub version: String,
    pub minecraft: String,
    pub loader: Option<Loader>,
    pub(crate) bytes: Vec<u8>,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn read_pack(path: &Path) -> Result<Pack, String> {
    parse(&files::read_bounded(path, MAX_ARCHIVE).map_err(error)?)
}
pub fn parse(bytes: &[u8]) -> Result<Pack, String> {
    if bytes.len() > MAX_ARCHIVE {
        return Err("整合包压缩文件超过 64 MiB".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(error)?;
    if archive.len() > 10000 {
        return Err("整合包条目过多".into());
    }
    let mut names = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(error)?;
        let name = entry.name().trim_end_matches('/');
        files::safe_join(Path::new("."), name).map_err(error)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
            || !names.insert(entry.name().to_ascii_lowercase())
        {
            return Err("整合包含重复路径或链接".into());
        }
    }
    let mut index = archive.by_name("modrinth.index.json").map_err(error)?;
    if index.size() > 4 * 1024 * 1024 {
        return Err("整合包清单超过 4 MiB".into());
    }
    let mut data = vec![];
    index
        .by_ref()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(error)?;
    let value: Value = serde_json::from_slice(&data).map_err(error)?;
    if value["formatVersion"] != 1 || value["game"] != "minecraft" {
        return Err("不支持的整合包格式".into());
    }
    let dependencies = value["dependencies"].as_object().ok_or("整合包依赖缺失")?;
    let minecraft = dependencies
        .get("minecraft")
        .and_then(Value::as_str)
        .ok_or("缺少 Minecraft 版本")?
        .to_owned();
    component(&minecraft)?;
    let mut loader = None;
    for (key, value) in dependencies {
        let kind = match key.as_str() {
            "minecraft" => continue,
            "fabric-loader" => Kind::Fabric,
            "quilt-loader" => Kind::Quilt,
            "forge" => Kind::Forge,
            "neoforge" => Kind::NeoForge,
            _ => return Err(format!("未知整合包运行依赖：{key}")),
        };
        let version = value.as_str().ok_or("加载器版本无效")?.to_owned();
        component(&version)?;
        if loader.is_some() {
            return Err("整合包不能同时要求多个加载器".into());
        }
        loader = Some(Loader { kind, version });
    }
    let pack = Pack {
        name: value["name"]
            .as_str()
            .unwrap_or("导入整合包")
            .chars()
            .take(160)
            .collect(),
        version: value["versionId"]
            .as_str()
            .unwrap_or("1")
            .chars()
            .take(100)
            .collect(),
        minecraft,
        loader,
        bytes: bytes.to_vec(),
    };
    // Reuse the production content/hash/path validator even before a target exists.
    pack.files(true)?;
    Ok(pack)
}
impl Pack {
    pub(crate) fn files(
        &self,
        optional: bool,
    ) -> Result<(Vec<resource_install::InstallFile>, Vec<String>), String> {
        resource_install::unpack(
            &self.bytes,
            &self.minecraft,
            self.loader
                .as_ref()
                .map(|l| l.kind.slug())
                .unwrap_or("minecraft"),
            self.loader
                .as_ref()
                .map(|l| l.version.as_str())
                .unwrap_or(""),
            optional,
        )
    }
}
#[derive(Debug, Clone)]
pub struct ExportPlan {
    pub name: String,
    pub download_files: usize,
    pub embedded_files: usize,
    pub bytes: usize,
    pub warnings: Vec<String>,
    pub lock: Lock,
    root: PathBuf,
    key: String,
    archive: Vec<u8>,
}
pub async fn preview_export(
    root: PathBuf,
    key: String,
    name: String,
    include_configs: bool,
) -> Result<ExportPlan, String> {
    if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("请输入 80 个字符以内、不含控制字符的整合包名称".into());
    }
    let lock = lockfile::capture(&root, &key, include_configs).await?;
    if lock.files.len() > 4094 {
        return Err("整合包导出最多包含 4094 个内容文件".into());
    }
    let base = files::instance_dir(&root, &key).map_err(error)?;
    let embedded = lock
        .files
        .iter()
        .filter(|file| file.downloads.is_empty() || file.size == 0)
        .collect::<Vec<_>>();
    if embedded.iter().map(|file| file.size).sum::<u64>() > 128 * 1024 * 1024 {
        return Err("本地覆盖文件超过 128 MiB，请先为大文件安装可复用来源版本".into());
    }
    let mut dependencies = json!({"minecraft":lock.minecraft});
    if let Some(loader) = &lock.loader {
        dependencies[loader.dependency()] = loader.version.clone().into();
    }
    let downloadable = lock
        .files
        .iter()
        .filter(|file| !file.downloads.is_empty() && file.size > 0)
        .collect::<Vec<_>>();
    let index = json!({"formatVersion":1,"game":"minecraft","name":name,"versionId":"1","dependencies":dependencies,"files":downloadable.iter().map(|file|json!({"path":file.path,"fileSize":file.size,"hashes":{"sha1":file.sha1,"sha512":file.sha512},"downloads":file.downloads,"env":{"client":"required","server":"optional"}})).collect::<Vec<_>>()});
    let index = serde_json::to_vec_pretty(&index).map_err(error)?;
    if index.len() > 4 * 1024 * 1024 {
        return Err("导出清单超过 4 MiB".into());
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(vec![]));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [
        ("modrinth.index.json", index),
        (
            "memoria.lock.json",
            serde_json::to_vec_pretty(&lock).map_err(error)?,
        ),
    ] {
        archive.start_file(name, options).map_err(error)?;
        archive.write_all(&bytes).map_err(error)?;
    }
    for file in &embedded {
        let bytes = files::read_bounded(
            &files::safe_join(&base, &file.path).map_err(error)?,
            64 * 1024 * 1024,
        )
        .map_err(error)?;
        if digest(&bytes) != file.sha256 {
            return Err("实例内容在导出预览期间变化".into());
        }
        archive
            .start_file(format!("overrides/{}", file.path), options)
            .map_err(error)?;
        archive.write_all(&bytes).map_err(error)?;
    }
    let archive = archive.finish().map_err(error)?.into_inner();
    if archive.len() > MAX_ARCHIVE {
        return Err("导出压缩文件超过 64 MiB".into());
    }
    Ok(ExportPlan {
        name,
        download_files: downloadable.len(),
        embedded_files: embedded.len(),
        bytes: archive.len(),
        warnings: lock.warnings.clone(),
        lock,
        root,
        key,
        archive,
    })
}
pub async fn export(plan: ExportPlan, destination: &Path) -> Result<(), String> {
    if !lockfile::verify(&plan.root, &plan.key, &plan.lock)
        .await?
        .matches
    {
        return Err("实例已变化，请重新生成导出预览".into());
    }
    if destination.exists() {
        return Err("导出文件已存在，请选择新文件名".into());
    }
    let parent = destination.parent().ok_or("导出路径无效")?;
    let temporary = parent.join(format!(".memoria-pack-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        files::write_new(&temporary, &plan.archive).map_err(error)?;
        fs::hard_link(&temporary, destination).map_err(error)?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}
