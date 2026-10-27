use super::*;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

pub(super) async fn sha1(client: &DownloadClient, url: &str) -> LauncherResult<String> {
    let bytes = client.bytes(&format!("{url}.sha1"), 4096).await?;
    let text = String::from_utf8_lossy(&bytes);
    let hash = text.split_whitespace().next().unwrap_or_default();
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("源站未提供有效 SHA-1，停止安装。"));
    }
    Ok(hash.to_ascii_lowercase())
}
pub(super) fn installer_profile(bytes: &[u8]) -> LauncherResult<Value> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    let file = archive
        .by_name("version.json")
        .map_err(|_| invalid("此旧版安装器缺少 version.json，当前仅支持现代 Forge/NeoForge。"))?;
    if file.size() > 4 * 1024 * 1024 {
        return Err(invalid("安装器版本元数据过大"));
    }
    let mut bytes = vec![];
    file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub(super) fn validate(profile: &Value, plan: &Plan) -> LauncherResult<()> {
    let id = profile["id"]
        .as_str()
        .ok_or_else(|| invalid("加载器配置缺少 id"))?;
    component(id)?;
    if profile["inheritsFrom"].as_str() != Some(plan.minecraft.as_str()) {
        return Err(invalid("加载器声明的父游戏版本不匹配"));
    }
    if profile.get("jar").is_some() {
        return Err(invalid("不支持加载器替换客户端 JAR 指向"));
    }
    if profile["mainClass"].as_str().is_none_or(|s| s.is_empty()) {
        return Err(invalid("加载器缺少启动主类"));
    }
    let expected = format!("{}{}", plan.kind.coordinate(), plan.version);
    if !profile["libraries"].as_array().is_some_and(|libs| {
        libs.iter().any(|lib| {
            lib["name"]
                .as_str()
                .is_some_and(|name| name == expected || name.starts_with(&format!("{expected}:")))
        })
    }) {
        return Err(invalid("加载器配置未声明所选加载器的确切版本"));
    }
    if profile
        .get("downloads")
        .is_some_and(|value| value.as_object().is_none_or(|value| !value.is_empty()))
    {
        return Err(invalid("加载器配置不能覆盖原版客户端下载信息"));
    }
    Ok(())
}

pub(super) async fn libraries(
    client: &DownloadClient,
    profile: &Value,
    require_hash: bool,
) -> LauncherResult<Vec<DownloadFile>> {
    let libraries = profile["libraries"]
        .as_array()
        .ok_or_else(|| invalid("加载器库列表无效"))?;
    if libraries.len() > 1000 {
        return Err(invalid("加载器库数量过多"));
    }
    let mut files = BTreeMap::new();
    for lib in libraries {
        // Loader-generated libraries do not use platform rules in direct profiles.
        if lib.get("natives").is_some() || (require_hash && lib.get("rules").is_some()) {
            return Err(invalid(
                "不支持此加载器库的平台规则，请使用官方安装器配置。",
            ));
        }
        if let Some(rules) = lib.get("rules") {
            let rules: Vec<super::super::rules::Rule> = serde_json::from_value(rules.clone())?;
            if !super::super::rules::allowed(
                &rules,
                &super::super::rules::Platform::current(),
                &std::collections::HashMap::new(),
            )? {
                continue;
            }
        }
        let name = lib["name"]
            .as_str()
            .ok_or_else(|| invalid("库缺少 Maven 坐标"))?;
        let maven = super::super::metadata::maven_path(name)?;
        let artifact = &lib["downloads"]["artifact"];
        let path = artifact["path"].as_str().unwrap_or(&maven);
        let relative = format!("libraries/{path}");
        files::safe_join(Path::new("."), &relative).map_err(invalid)?;
        // A Maven coordinate and artifact path must refer to the same file.
        if require_hash && path != maven {
            return Err(invalid("库路径与 Maven 坐标不一致"));
        }
        let url = artifact["url"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{}/{}",
                    lib["url"]
                        .as_str()
                        .unwrap_or("https://libraries.minecraft.net")
                        .trim_end_matches('/'),
                    path
                )
            });
        let generated = !require_hash && artifact["url"].as_str() == Some("");
        let sha1 = match artifact["sha1"].as_str() {
            Some(hash) if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {
                Some(hash.to_owned())
            }
            Some("") if generated => None,
            Some(_) => return Err(invalid("库 SHA-1 无效")),
            None if require_hash => Some(self::sha1(client, &url).await?),
            None => None,
        };
        let file = DownloadFile {
            url,
            path: relative.clone(),
            sha1,
            size: artifact["size"]
                .as_u64()
                .filter(|size| !generated || *size != 0),
        };
        if file.size.is_some_and(|n| n > 128 * 1024 * 1024) {
            return Err(invalid("单个加载器库过大"));
        }
        if let Some(previous) = files.insert(relative, file.clone()) {
            if previous.url != file.url || previous.sha1 != file.sha1 {
                return Err(invalid("重复的库路径内容冲突"));
            }
        }
    }
    Ok(files.into_values().collect())
}

pub(super) fn normalized(profile: &Value, plan: &Plan) -> Value {
    let mut profile = profile.clone();
    profile["id"] = plan.target.clone().into();
    profile["inheritsFrom"] = plan.base.split_once('/').unwrap().1.into();
    profile
}
