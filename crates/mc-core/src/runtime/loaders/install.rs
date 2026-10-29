use super::*;
use crate::{JavaRuntime, ProgressCallback};
use std::{fs, io::Write};

/// Installs into a NEW isolated instance. Existing version JSONs and libraries are never replaced.
pub async fn install(
    client: DownloadClient,
    plan: Plan,
    java: PathBuf,
    progress: ProgressCallback,
) -> LauncherResult<String> {
    let layout = GameLayout::new(&plan.root)?;
    let base = files::instance_dir(&plan.root, &plan.base).map_err(invalid)?;
    let _collection_lock = files::lock(&plan.root).map_err(invalid)?;
    let _base_lock = files::lock(&base).map_err(invalid)?;
    if Some(base_snapshot(&layout, &plan.base)?) != plan.snapshot {
        return Err(LauncherError::PlanStale);
    }
    let mut java = JavaRuntime::inspect(java).await?;
    if !java.executable.is_absolute() && java.executable.components().count() > 1 {
        java.executable = std::path::absolute(&java.executable)?;
    }
    if plan
        .required_java
        .is_some_and(|required| java.major_version != required)
    {
        return Err(LauncherError::JavaUnavailable(format!(
            "此实例要求 Java {}，当前为 {}",
            plan.required_java.unwrap(),
            java.major_version
        )));
    }
    let operation = format!(".memoria/loader-installs/{}", uuid::Uuid::new_v4());
    let stage = files::safe_join(&plan.root, &operation).map_err(invalid)?;
    files::create_dirs(&plan.root, &format!("{operation}/work")).map_err(invalid)?;
    let work = stage.join("work");
    let result = install_staged(&client, &plan, &java, &layout, &base, &work, progress).await;
    let status = match &result {
        Ok(key) => serde_json::json!({"status":"complete","instance":key}),
        Err(error) => serde_json::json!({"status":"failed","error":error.to_string()}),
    };
    // Best effort receipt; the staged files remain available for diagnosis/recovery.
    let _ = fs::write(
        stage.join("result.json"),
        serde_json::to_vec_pretty(&status)?,
    );
    result.map_err(|error| invalid(format!("{error}；安装记录保留在 {}", stage.display())))
}

async fn install_staged(
    client: &DownloadClient,
    plan: &Plan,
    java: &JavaRuntime,
    layout: &GameLayout,
    base: &Path,
    work: &Path,
    progress: ProgressCallback,
) -> LauncherResult<String> {
    let base_id = plan.base.split_once('/').unwrap().1;
    let base_stage =
        files::safe_join(work, &format!("versions/{}", plan.minecraft)).map_err(invalid)?;
    fs::create_dir_all(&base_stage)?;
    for extension in ["json", "jar"] {
        let name = format!("{base_id}.{extension}");
        let source = files::safe_join(base, &name).map_err(invalid)?;
        let destination = base_stage.join(format!("{}.{extension}", plan.minecraft));
        if extension == "json" {
            let mut metadata: Value = serde_json::from_slice(&fs::read(source)?)?;
            metadata["id"] = plan.minecraft.clone().into();
            fs::write(destination, serde_json::to_vec(&metadata)?)?;
        } else {
            fs::copy(source, destination)?;
        }
    }
    fs::write(work.join("launcher_profiles.json"), br#"{"profiles":{}}"#)?;
    let staging_layout = GameLayout::new(work)?;
    let profile = plan
        .profile
        .as_ref()
        .ok_or_else(|| invalid("缺少加载器预览"))?;
    let profile = if let Some(installer) = &plan.installer {
        client
            .download_files(
                staging_layout.clone(),
                vec![installer.clone()],
                progress.clone(),
            )
            .await?;
        java_installer::run(&java.executable, &work.join("installer.jar"), work).await?;
        let id = profile["id"].as_str().unwrap();
        let output =
            files::safe_join(work, &format!("versions/{id}/{id}.json")).map_err(invalid)?;
        let bytes = files::read_bounded(&output, 4 * 1024 * 1024).map_err(invalid)?;
        let actual: Value = serde_json::from_slice(&bytes)?;
        if &actual != profile {
            return Err(invalid("安装器输出与预览中的版本配置不同，未发布实例。"));
        }
        actual
    } else {
        let mut total_bytes = 0usize;
        for (index, file) in plan.files.iter().enumerate() {
            let limit = (1024 * 1024 * 1024usize - total_bytes).min(128 * 1024 * 1024);
            let bytes = client.bytes(&file.url, limit).await?;
            crate::service::downloads::verify_sha1(
                &format!("{:x}", sha1::Sha1::digest(&bytes)),
                file.sha1.as_deref(),
            )?;
            if file.size.is_some_and(|size| size != bytes.len() as u64) {
                return Err(invalid("加载器库文件大小不匹配"));
            }
            total_bytes += bytes.len();
            let path = files::safe_join(work, &file.path).map_err(invalid)?;
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, bytes)?;
            progress(crate::DownloadProgress {
                completed: index + 1,
                total: plan.files.len(),
                reused: 0,
            });
        }
        profile.clone()
    };
    profiles::validate(&profile, plan)?;
    let libraries = if plan.installer.is_some() {
        profiles::libraries(client, &profile, false).await?
    } else {
        plan.files.clone()
    };
    // Verify every artifact before copying any file to the live collection.
    let mut manifest = vec![];
    let mut artifacts = std::collections::BTreeMap::new();
    for file in libraries {
        let source = files::safe_join(work, &file.path).map_err(invalid)?;
        let bytes = files::read_bounded(&source, 128 * 1024 * 1024).map_err(invalid)?;
        let sha1 = format!("{:x}", sha1::Sha1::digest(&bytes));
        crate::service::downloads::verify_sha1(&sha1, file.sha1.as_deref())?;
        if file.size.is_some_and(|size| size != bytes.len() as u64) {
            return Err(invalid("安装器库文件大小不匹配"));
        }
        artifacts.insert(
            file.path.clone(),
            (sha1, bytes.len() as u64, file.url.clone()),
        );
        manifest.push((file.path, format!("{:x}", Sha256::digest(bytes))));
    }
    fs::write(
        work.parent().unwrap().join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    if Some(base_snapshot(layout, &plan.base)?) != plan.snapshot {
        return Err(LauncherError::PlanStale);
    }
    for (path, hash) in &manifest {
        let destination = files::safe_join(&plan.root, path).map_err(invalid)?;
        if destination.exists() {
            let current = files::read_bounded(&destination, 128 * 1024 * 1024).map_err(invalid)?;
            if format!("{:x}", Sha256::digest(current)) != *hash {
                return Err(invalid(format!("已有共享库与安装文件不同，未覆盖：{path}")));
            }
        }
    }
    // Only add absent, verified artifacts; publishing the profile is the final commit.
    for (path, _) in &manifest {
        let destination = files::safe_join(&plan.root, path).map_err(invalid)?;
        let source = files::safe_join(work, path).map_err(invalid)?;
        if !destination.exists() {
            publish_file(&source, &destination)?;
        }
    }
    if Some(base_snapshot(layout, &plan.base)?) != plan.snapshot {
        return Err(LauncherError::PlanStale);
    }
    let destination =
        files::safe_join(&plan.root, &format!("versions/{}", plan.target)).map_err(invalid)?;
    fs::create_dir_all(destination.parent().unwrap())?;
    fs::create_dir(&destination)?; // atomic reservation, never reuse another instance directory
    let result = (|| {
        let mut profile = profiles::normalized(&profile, plan);
        // Replace installer placeholders (empty hashes / size 0) with the actual verified artifacts.
        // Direct metadata profiles also retain their pinned sidecar hashes for future launch checks.
        for library in profile["libraries"].as_array_mut().unwrap() {
            let maven = super::super::metadata::maven_path(library["name"].as_str().unwrap())?;
            let path = library["downloads"]["artifact"]["path"]
                .as_str()
                .unwrap_or(&maven)
                .to_owned();
            if let Some((sha1, size, url)) = artifacts.get(&format!("libraries/{path}")) {
                library["downloads"]["artifact"]["path"] = path.into();
                library["downloads"]["artifact"]["sha1"] = sha1.clone().into();
                library["downloads"]["artifact"]["size"] = (*size).into();
                library["downloads"]["artifact"]["url"] = url.clone().into();
            }
        }
        let settings = crate::service::desktop_instances::InstanceSettings {
            java: Some(java.executable.clone()),
            ..Default::default()
        };
        fs::create_dir(destination.join(".memoria"))?;
        let settings_path = work.join("instance-settings.json");
        fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;
        publish_file(
            &settings_path,
            &destination.join(".memoria/instance-settings.json"),
        )?;
        let temporary = work.join("published-profile.json");
        fs::write(&temporary, serde_json::to_vec_pretty(&profile)?)?;
        publish_file(
            &temporary,
            &destination.join(format!("{}.json", plan.target)),
        )
    })();
    if result.is_err() {
        let settings = destination.join(".memoria/instance-settings.json");
        if let (Ok(actual), Ok(expected)) = (
            fs::read(&settings),
            fs::read(work.join("instance-settings.json")),
        ) {
            if actual == expected {
                let _ = fs::remove_file(settings);
            }
        }
        let _ = fs::remove_dir(destination.join(".memoria"));
        let _ = fs::remove_dir(&destination);
    }
    result?;
    Ok(format!("versions/{}", plan.target))
}

fn publish_file(source: &Path, destination: &Path) -> LauncherResult<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("目标路径无效"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".memoria-loader-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        output.write_all(&std::fs::read(source)?)?;
        output.sync_all()?;
        drop(output);
        // A hard link publishes a complete file and atomically refuses an existing destination.
        fs::hard_link(&temporary, destination)
    })();
    let _ = fs::remove_file(temporary);
    result.map_err(Into::into)
}
