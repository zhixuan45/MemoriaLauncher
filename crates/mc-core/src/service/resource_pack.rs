use super::*;
pub async fn fetch_pack(
    client: DownloadClient,
    project: String,
    version: String,
) -> Result<super::super::packs::exchange::Pack> {
    identifier(&project)?;
    identifier(&version)?;
    let catalog = ResourceCatalog::new().map_err(error)?;
    let owner = catalog
        .metadata(&format!("/project/{project}"))
        .await
        .map_err(error)?;
    if owner["project_type"] != "modpack" {
        return Err("来源项目不是整合包".into());
    }
    let metadata = catalog
        .metadata(&format!("/version/{version}"))
        .await
        .map_err(error)?;
    if metadata["id"] != version || metadata["project_id"] != project {
        return Err("整合包版本身份不一致".into());
    }
    let list = metadata["files"].as_array().ok_or("整合包版本没有文件")?;
    let primary = list
        .iter()
        .find(|f| f["primary"] == true)
        .or(list.first())
        .ok_or("整合包版本没有文件")?;
    let filename = string(primary, "filename")?;
    files::basename(filename, ".mrpack").map_err(error)?;
    let descriptor = file(primary, filename.into(), "源站整合包".into())?;
    super::super::packs::exchange::parse(&download(&client, &descriptor).await?)
}
/// Preview a local mrpack against an existing compatible instance, without modifying game content.
pub async fn prepare_pack(
    root: PathBuf,
    instance: String,
    bytes: Vec<u8>,
    optional: bool,
    client: DownloadClient,
) -> Result<Preview> {
    let pack = super::super::packs::exchange::parse(&bytes)?;
    let request = Request {
        root,
        instance,
        kind: ResourceKind::Modpack,
        project: "local-pack".into(),
        version: digest(&bytes),
        optional,
    };
    let base = directory(&request.root, &request.instance)?;
    let report = LocalClientService::new(GameLayout::new(&request.root).map_err(error)?)
        .scan()
        .await
        .map_err(error)?;
    let target = report
        .clients
        .iter()
        .find(|entry| entry.instance.id == request.instance)
        .ok_or("目标实例元数据无效")?;
    let (loader, loader_version) = loader(target.instance.loader.as_ref());
    let (mut output, mut warnings) = unpack(
        &bytes,
        &target.instance.mc_version,
        loader,
        loader_version,
        optional,
    )?;
    let environment_digest = super::super::mod_resolver::inventory::environment_digest(
        &request.root,
        &request.instance,
    )?;
    let mut watches = vec![
        files::Watch::file(&base, MANIFEST).map_err(error)?,
        files::Watch::file(&base, super::super::packs::lockfile::LOCK_PATH).map_err(error)?,
    ];
    for folder in ["mods", "resourcepacks", "shaderpacks"] {
        watches.push(files::Watch::directory(&base, folder).map_err(error)?);
    }
    let previous = files::safe_join(&base, MANIFEST).map_err(error)?;
    let mut manifest: Value = if previous.exists() {
        serde_json::from_slice(&files::read_bounded(&previous, 4 * 1024 * 1024).map_err(error)?)
            .map_err(error)?
    } else {
        json!({"version":1,"resources":[]})
    };
    let records = manifest["resources"].as_array_mut().ok_or("来源清单无效")?;
    for file in &output {
        watches.push(files::Watch::file(&base, &file.path).map_err(error)?);
        records.retain(|record| record["path"] != file.path);
        records.push(
            json!({"path":file.path,"downloads":file.urls,"sha1":file.sha1,"sha512":file.sha512}),
        );
    }
    let bytes = output.iter().map(|f| f.size).sum::<u64>();
    if bytes > 1024 * 1024 * 1024 {
        return Err("单次整合包计划超过 1 GiB".into());
    }
    dependencies::audit_preview(&request, &client, &mut output, &mut watches, &mut warnings)
        .await?;
    let inline = serde_json::to_vec_pretty(&manifest).map_err(error)?;
    output.push(InstallFile {
        path: MANIFEST.into(),
        size: inline.len() as u64,
        source: "整合包来源记录".into(),
        urls: vec![],
        sha1: String::new(),
        sha512: None,
        inline: Some(inline),
        remove: false,
    });
    warnings.push("整合包不会移除目标中的额外文件；请核对前置和冲突，或选择新建独立实例。".into());
    Ok(Preview {
        request,
        files: output,
        warnings,
        title: pack.name,
        target: target.instance.name.clone(),
        bytes,
        watches,
        environment_digest,
        #[cfg(test)]
        fail_commit_at: None,
    })
}
