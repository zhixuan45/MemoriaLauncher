//! One public workflow for standalone loader instances and mrpack-driven new instances.
use super::{exchange::Pack, lockfile, *};
use crate::service::instance_write::{digest, files};
use crate::{
    runtime::loaders::{self, Catalog, Plan as LoaderPlan},
    DownloadClient, GameLayout, JavaRuntime, ProgressCallback, VanillaPlan,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone)]
pub struct Request {
    pub root: PathBuf,
    pub target: String,
    pub minecraft: String,
    pub loader: Option<Loader>,
    pub java: PathBuf,
    pub pack: Option<Pack>,
    pub optional: bool,
}
#[derive(Debug, Clone)]
pub struct Plan {
    target: String,
    minecraft: String,
    loader: Option<Loader>,
    warnings: Vec<String>,
    file_count: usize,
    known_bytes: u64,
    required_java: Option<u32>,
    request: Request,
    vanilla: VanillaPlan,
    loader_plan: Option<LoaderPlan>,
    operation: String,
    work: PathBuf,
}
impl Plan {
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn minecraft(&self) -> &str {
        &self.minecraft
    }
    pub fn loader(&self) -> Option<&Loader> {
        self.loader.as_ref()
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    pub fn file_count(&self) -> usize {
        self.file_count
    }
    pub fn known_bytes(&self) -> u64 {
        self.known_bytes
    }
    pub fn required_java(&self) -> Option<u32> {
        self.required_java
    }
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub async fn preview(request: Request, client: DownloadClient) -> Result<Plan, String> {
    preview_with(request, client.clone(), Catalog::new(client)).await
}
pub(crate) async fn preview_with(
    mut request: Request,
    client: DownloadClient,
    catalog: Catalog,
) -> Result<Plan, String> {
    component(&request.target)?;
    component(&request.minecraft)?;
    request.root = GameLayout::new(&request.root)
        .map_err(error)?
        .root()
        .to_owned();
    if request.target == request.minecraft {
        return Err("新实例名称不能与共享原版版本 ID 相同".into());
    }
    if files::safe_join(&request.root, &format!("versions/{}", request.target))
        .map_err(error)?
        .exists()
    {
        return Err("目标实例已存在，不会覆盖".into());
    }
    if let Some(pack) = &request.pack {
        if pack.minecraft != request.minecraft || pack.loader != request.loader {
            return Err("Minecraft 或加载器选择与整合包清单不一致".into());
        }
    }
    let operation = uuid::Uuid::new_v4().to_string();
    let work = request
        .root
        .join(format!(".memoria/instance-installs/{operation}/work"));
    let vanilla = crate::preview_vanilla(client, request.minecraft.clone())
        .await
        .map_err(error)?;
    let loader_plan = if let Some(loader) = &request.loader {
        Some(
            catalog
                .preview_new(
                    work.clone(),
                    request.minecraft.clone(),
                    loader.kind,
                    loader.catalog_version(&request.minecraft),
                    request.target.clone(),
                    vanilla.required_java(),
                )
                .await
                .map_err(error)?,
        )
    } else {
        None
    };
    let mut warnings = vec![
        "将新建独立实例；游戏、加载器与资源会在暂存目录校验后发布，不覆盖已有实例。".into(),
        "若部署失败，已校验的共享缓存或原版基础版本可能保留；安装记录供检查。".into(),
    ];
    let mut file_count = vanilla.files().len();
    let mut known_bytes: u64 = vanilla.files().iter().filter_map(|f| f.size).sum();
    if let Some(loader) = &loader_plan {
        file_count += loader.files().len();
        known_bytes = known_bytes.saturating_add(
            loader
                .files()
                .iter()
                .fold(0u64, |sum, file| sum.saturating_add(file.size.unwrap_or(0))),
        );
        if loader.installer().is_some() {
            warnings.push("确认后将以所选 Java 执行校验过的官方 Forge/NeoForge 安装器；暂存目录不构成操作系统沙箱。".into());
        }
    }
    if let Some(pack) = &request.pack {
        let (files, extra) = pack.files(request.optional)?;
        file_count += files.len();
        known_bytes = known_bytes.saturating_add(files.iter().map(|f| f.size).sum::<u64>());
        warnings.extend(extra);
    }
    if known_bytes > 8 * 1024 * 1024 * 1024 || file_count > 205000 {
        return Err("新实例超过总文件数量或已知体积预算".into());
    }
    let required_java = vanilla
        .required_java()
        .into_iter()
        .chain(loader_plan.as_ref().and_then(|l| l.required_java()))
        .max();
    Ok(Plan {
        target: request.target.clone(),
        minecraft: request.minecraft.clone(),
        loader: request.loader.clone(),
        warnings,
        file_count,
        known_bytes,
        required_java,
        request,
        vanilla,
        loader_plan,
        operation,
        work,
    })
}
pub async fn install(
    plan: Plan,
    client: DownloadClient,
    progress: ProgressCallback,
) -> Result<String, String> {
    let root = &plan.request.root;
    if !root.exists() {
        fs::create_dir_all(root).map_err(error)?;
    }
    let _collection = files::lock(root).map_err(error)?;
    let destination =
        files::safe_join(root, &format!("versions/{}", plan.target)).map_err(error)?;
    if destination.exists() {
        return Err("目标实例已存在，请重新预览".into());
    }
    let mut java = JavaRuntime::inspect(&plan.request.java)
        .await
        .map_err(error)?;
    if !java.executable.is_absolute() && java.executable.components().count() > 1 {
        java.executable = std::path::absolute(&java.executable).map_err(error)?;
    }
    if plan
        .required_java
        .is_some_and(|required| required != java.major_version)
    {
        return Err(format!(
            "此实例要求 Java {}，当前为 {}",
            plan.required_java.unwrap(),
            java.major_version
        ));
    }
    let operation = format!(".memoria/instance-installs/{}", plan.operation);
    files::create_dirs(root, &format!("{operation}/work")).map_err(error)?;
    files::write_new(&files::safe_join(root,&format!("{operation}/plan.json")).map_err(error)?,&serde_json::to_vec_pretty(&serde_json::json!({"version":1,"target":plan.target,"minecraft":plan.minecraft,"loader":plan.loader,"state":"staging"})).map_err(error)?).map_err(error)?;
    let result = install_staged(&plan, client, java.executable, progress).await;
    let report = serde_json::json!({"instance":result.as_ref().ok(),"error":result.as_ref().err(),"state":if result.is_ok(){"complete"}else{"failed"}});
    let _ = fs::write(
        root.join(format!("{operation}/result.json")),
        serde_json::to_vec_pretty(&report).map_err(error)?,
    );
    result.map_err(|cause| format!("{cause}；安装记录：{}", root.join(operation).display()))
}
async fn install_staged(
    plan: &Plan,
    client: DownloadClient,
    java: PathBuf,
    progress: ProgressCallback,
) -> Result<String, String> {
    let layout = GameLayout::new(&plan.work).map_err(error)?;
    hydrate(plan, progress.clone())?;
    crate::install_vanilla_plan(
        client.clone(),
        layout,
        plan.vanilla.clone(),
        progress.clone(),
    )
    .await
    .map_err(error)?;
    let mut pins = vec![];
    for extension in ["json", "jar"] {
        let path = format!("versions/{0}/{0}.{extension}", plan.minecraft);
        pins.push((
            path.clone(),
            hash(&files::safe_join(&plan.work, &path).map_err(error)?)?.0,
        ));
    }
    let key = if let Some(loader) = plan.loader_plan.clone() {
        loaders::install(
            client.clone(),
            loader.bind_base_snapshot().map_err(error)?,
            java.clone(),
            progress.clone(),
        )
        .await
        .map_err(error)?
    } else {
        let base =
            files::create_dirs(&plan.work, &format!("versions/{}", plan.target)).map_err(error)?;
        files::write_new(&base.join(format!("{}.json",plan.target)),&serde_json::to_vec_pretty(&serde_json::json!({"id":plan.target,"inheritsFrom":plan.minecraft,"clientVersion":plan.minecraft,"type":"release"})).map_err(error)?).map_err(error)?;
        format!("versions/{}", plan.target)
    };
    let profile_path = format!("{key}/{}.json", plan.target);
    pins.push((
        profile_path.clone(),
        hash(&files::safe_join(&plan.work, &profile_path).map_err(error)?)?.0,
    ));
    if let Some(pack) = &plan.request.pack {
        let preview = crate::service::resource_install::prepare_pack(
            plan.work.clone(),
            key.clone(),
            pack.bytes.clone(),
            plan.request.optional,
            client.clone(),
        )
        .await?;
        let forward = progress.clone();
        crate::service::resource_install::install(
            preview,
            client,
            std::sync::Arc::new(move |p| {
                forward(crate::DownloadProgress {
                    completed: p.done,
                    total: p.total,
                    reused: 0,
                })
            }),
        )
        .await?;
    }
    let base = files::instance_dir(&plan.work, &key).map_err(error)?;
    files::create_dirs(&base, ".memoria").map_err(error)?;
    let settings = crate::service::desktop_instances::InstanceSettings {
        name: plan
            .request
            .pack
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| plan.target.clone())
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect(),
        java: Some(java),
        ..Default::default()
    };
    settings.validate()?;
    fs::write(
        base.join(".memoria/instance-settings.json"),
        serde_json::to_vec_pretty(&settings).map_err(error)?,
    )
    .map_err(error)?;
    let lock = lockfile::capture(&plan.work, &key, true).await?;
    lockfile::save(&plan.work, &key, &lock).await?;
    publish(plan, &base, &lock, &pins)?;
    Ok(key)
}
fn tree(base: &Path, relative: &str, result: &mut Vec<String>, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("共享依赖目录嵌套过深".into());
    }
    let path = files::safe_join(base, relative).map_err(error)?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(error(e)),
    };
    for entry in entries {
        let name = entry
            .map_err(error)?
            .file_name()
            .into_string()
            .map_err(|_| "共享文件名无效")?;
        let path = format!("{relative}/{name}");
        let absolute = files::safe_join(base, &path).map_err(error)?;
        let meta = fs::symlink_metadata(absolute).map_err(error)?;
        if meta.is_dir() {
            tree(base, &path, result, depth + 1)?;
        } else if meta.is_file() {
            result.push(path);
        } else {
            return Err("共享依赖包含非普通文件".into());
        }
        if result.len() > 200000 {
            return Err("共享依赖超过 200000 个文件".into());
        }
    }
    Ok(())
}
fn hash(path: &Path) -> Result<(String, usize), String> {
    let bytes = files::read_bounded(path, 128 * 1024 * 1024).map_err(error)?;
    Ok((digest(&bytes), bytes.len()))
}
fn copy_new(source: &Path, destination: &Path) -> Result<(), String> {
    copy_checked(source, destination, None)
}
fn copy_checked(source: &Path, destination: &Path, expected: Option<&str>) -> Result<(), String> {
    let parent = destination.parent().ok_or("发布路径无效")?;
    fs::create_dir_all(parent).map_err(error)?;
    let temporary = parent.join(format!(".memoria-publish-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(error)?;
        let bytes = files::read_bounded(source, 128 * 1024 * 1024).map_err(error)?;
        if expected.is_some_and(|expected| digest(&bytes) != expected) {
            return Err("发布前暂存文件哈希发生变化".into());
        }
        output.write_all(&bytes).map_err(error)?;
        output.sync_all().map_err(error)?;
        drop(output);
        fs::hard_link(&temporary, destination).map_err(error)?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}
fn hydrate(plan: &Plan, progress: ProgressCallback) -> Result<(), String> {
    use sha1::{Digest, Sha1};
    let mut reused = 0;
    for file in plan.vanilla.files() {
        let Some(expected) = &file.sha1 else {
            continue;
        };
        let source = files::safe_join(&plan.request.root, &file.path).map_err(error)?;
        if !source.exists() {
            continue;
        }
        let bytes = files::read_bounded(&source, 128 * 1024 * 1024).map_err(error)?;
        if !format!("{:x}", Sha1::digest(&bytes)).eq_ignore_ascii_case(expected)
            || file.size.is_some_and(|size| size != bytes.len() as u64)
        {
            return Err(format!("已有共享文件内容不同，不会覆盖：{}", file.path));
        }
        let target = files::safe_join(&plan.work, &file.path).map_err(error)?;
        copy_new(&source, &target)?;
        reused += 1;
        progress(crate::DownloadProgress {
            completed: reused,
            total: plan.vanilla.files().len(),
            reused,
        });
    }
    Ok(())
}
fn publish(
    plan: &Plan,
    base: &Path,
    lock: &lockfile::Lock,
    pins: &[(String, String)],
) -> Result<(), String> {
    let root = &plan.request.root;
    let mut shared = vec![];
    for folder in ["libraries", "assets"] {
        tree(&plan.work, folder, &mut shared, 0)?;
    }
    shared.sort();
    shared.push(format!("versions/{0}/{0}.jar", plan.minecraft));
    shared.push(format!("versions/{0}/{0}.json", plan.minecraft));
    let mut verified = vec![];
    let mut total = 0u64;
    for path in &shared {
        let source = files::safe_join(&plan.work, path).map_err(error)?;
        let (expected, size) = hash(&source)?;
        if pins
            .iter()
            .find(|(name, _)| name == path)
            .is_some_and(|(_, pinned)| pinned != &expected)
        {
            return Err("原版基础文件在暂存期间被修改".into());
        }
        total = total.saturating_add(size as u64);
        if total > 8 * 1024 * 1024 * 1024u64 {
            return Err("共享依赖超过 8 GiB 发布预算".into());
        }
        let target = files::safe_join(root, path).map_err(error)?;
        if target.exists() && hash(&target)?.0 != expected {
            return Err(format!("已有共享文件内容不同，不会覆盖：{path}"));
        }
        verified.push((path.clone(), expected));
    }
    for (path, expected) in &verified {
        let source = files::safe_join(&plan.work, path).map_err(error)?;
        if hash(&source)?.0 != *expected {
            return Err("暂存共享文件已变化".into());
        }
        let target = files::safe_join(root, path).map_err(error)?;
        if target.exists() {
            if hash(&target)?.0 != *expected {
                return Err("共享文件在发布期间被修改".into());
            }
        } else {
            copy_checked(&source, &target, Some(expected))?;
        }
    }
    let destination =
        files::safe_join(root, &format!("versions/{}", plan.target)).map_err(error)?;
    files::create_dirs(root, "versions").map_err(error)?;
    fs::create_dir(&destination).map_err(error)?;
    let _target_guard = files::lock(&destination).map_err(error)?;
    let result = (|| -> Result<(), String> {
        let mut names = lock
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>();
        names.extend([
            ".memoria/instance-settings.json".into(),
            lockfile::LOCK_PATH.into(),
        ]);
        if base.join(".memoria/resource-install.json").exists() {
            names.push(".memoria/resource-install.json".into());
        }
        for path in names {
            let source = files::safe_join(base, &path).map_err(error)?;
            let target = files::safe_join(&destination, &path).map_err(error)?;
            let expected = if path == lockfile::LOCK_PATH {
                digest(&serde_json::to_vec_pretty(lock).map_err(error)?)
            } else {
                lock.files
                    .iter()
                    .find(|f| f.path == path)
                    .map(|f| f.sha256.clone())
                    .unwrap_or(hash(&source)?.0)
            };
            copy_checked(&source, &target, Some(&expected))?;
        }
        for file in &lock.files {
            if files::file_hash(&files::safe_join(&destination, &file.path).map_err(error)?)
                .map_err(error)?
                .as_deref()
                != Some(&file.sha256)
            {
                return Err("发布后的实例内容与锁定记录不一致".into());
            }
        }
        // The profile is the visibility/launch commit point, after all content and lock data.
        for (path, expected) in pins
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("versions/{}/", plan.minecraft)))
        {
            if hash(&files::safe_join(root, path).map_err(error)?)?.0 != *expected {
                return Err("共享原版基础文件在发布期间已变化".into());
            }
        }
        let expected = pins
            .iter()
            .find(|(path, _)| path == &format!("versions/{0}/{0}.json", plan.target))
            .ok_or("缺少新实例配置快照")?;
        copy_checked(
            &base.join(format!("{}.json", plan.target)),
            &destination.join(format!("{}.json", plan.target)),
            Some(&expected.1),
        )
    })();
    result.map_err(|cause| {
        format!(
            "{cause}；未发布完整新实例，预留目录保留供检查：{}",
            destination.display()
        )
    })
}
