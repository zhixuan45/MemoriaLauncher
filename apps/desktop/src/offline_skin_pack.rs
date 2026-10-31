//! A launcher-owned resource pack for the offline account's vanilla default skin.
use mc_core::{service::instance_write::files, AuthAccount};
use std::{
    io::{Read, Write},
    path::Path,
};

const FILE: &str = "Memoria-Offline-Skin.zip";
const PACK_ID: &str = "file/Memoria-Offline-Skin.zip";
const MARKER: &str = "memoria-offline-skin.txt";
const MARKER_CONTENT: &[u8] = b"Memoria Launcher offline skin pack v1";

fn target(uuid: &str) -> Result<String, String> {
    crate::default_skins::texture_path(uuid)
}

fn format_for(version: &str) -> Result<u32, String> {
    match version {
        "1.20.1" => Ok(15),
        "1.21.1" => Ok(34),
        _ => Err(format!(
            "离线皮肤资源包暂不支持 Minecraft {version}；已验证版本为 1.20.1 和 1.21.1。"
        )),
    }
}

pub fn supported(version: &str) -> bool {
    format_for(version).is_ok()
}

fn pack(account: &AuthAccount, skin: &[u8], version: &str) -> Result<Vec<u8>, String> {
    crate::account_skin::avatar(skin)?;
    let image = image::load_from_memory_with_format(skin, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    if image.height() != 64 {
        return Err("游戏内离线皮肤需要 64×64 PNG；64×32 旧版皮肤仍可用于启动器头像。".into());
    }
    let format = format_for(version)?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("pack.mcmeta", options)
        .map_err(|e| e.to_string())?;
    writer
        .write_all(
            serde_json::json!({"pack":{"pack_format":format,"description":"Memoria 离线账户皮肤"}})
                .to_string()
                .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    writer
        .start_file(MARKER, options)
        .map_err(|e| e.to_string())?;
    writer
        .write_all(MARKER_CONTENT)
        .map_err(|e| e.to_string())?;
    writer
        .start_file(target(&account.uuid)?, options)
        .map_err(|e| e.to_string())?;
    writer.write_all(skin).map_err(|e| e.to_string())?;
    Ok(writer.finish().map_err(|e| e.to_string())?.into_inner())
}

fn check_owned(bytes: &[u8]) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| "同名资源包不属于启动器，未覆盖。")?;
    let mut marker = zip
        .by_name(MARKER)
        .map_err(|_| "同名资源包不属于启动器，未覆盖。")?;
    let mut content = Vec::new();
    marker
        .by_ref()
        .take(100)
        .read_to_end(&mut content)
        .map_err(|e| e.to_string())?;
    if content != MARKER_CONTENT {
        return Err("同名资源包不属于启动器，未覆盖。".into());
    }
    Ok(())
}

fn options(original: &str, enabled: bool) -> Result<String, String> {
    let mut out = String::new();
    let mut found = false;
    for line in original.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if let Some(value) = content.strip_prefix("resourcePacks:") {
            if found {
                return Err("options.txt 包含重复的 resourcePacks 设置。".into());
            }
            found = true;
            let mut packs: Vec<String> = serde_json::from_str(value)
                .map_err(|_| "options.txt 的 resourcePacks 无法解析。")?;
            packs.retain(|name| name != PACK_ID);
            if enabled {
                packs.push(PACK_ID.into());
            }
            out.push_str("resourcePacks:");
            out.push_str(&serde_json::to_string(&packs).map_err(|e| e.to_string())?);
            out.push_str(&line[content.len()..]);
        } else {
            out.push_str(line);
        }
    }
    if !found && enabled {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("resourcePacks:[\"{PACK_ID}\"]\n"));
    }
    Ok(out)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_file_name(format!(".memoria-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

pub fn prepare(
    base: &Path,
    account: &AuthAccount,
    data_root: &Path,
    version: &str,
) -> Result<Option<String>, String> {
    let skin_path = crate::account_skin::skin_path(data_root, account)?;
    let skin = if account.is_offline() {
        match std::fs::read(skin_path) {
            Ok(data) => Some(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("读取离线皮肤失败：{e}")),
        }
    } else {
        None
    };
    let mut warning = None;
    let legacy = if supported(version) {
        if let Some(skin) = skin.as_deref() {
            crate::account_skin::avatar(skin)?;
            image::load_from_memory_with_format(skin, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?
                .height()
                == 32
        } else {
            false
        }
    } else {
        false
    };
    let new_pack = if skin.is_some() && !supported(version) {
        warning = Some("此版本暂不支持离线皮肤资源包".to_string());
        None
    } else if legacy {
        warning = Some("游戏内离线皮肤需要 64×64 PNG".to_string());
        None
    } else if supported(version) {
        skin.as_deref()
            .map(|skin| pack(account, skin, version))
            .transpose()?
    } else {
        None
    };
    let options_path = files::safe_join(base, "options.txt").map_err(|e| e.to_string())?;
    let original = match std::fs::read(&options_path) {
        Ok(bytes) if bytes.len() <= 4 * 1024 * 1024 => bytes,
        Ok(_) => return Err("options.txt 过大，未修改。".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("读取 options.txt 失败：{e}")),
    };
    let text = std::str::from_utf8(&original).map_err(|_| "options.txt 不是 UTF-8，未修改。")?;
    if new_pack.is_none() && !text.contains(PACK_ID) {
        return Ok(warning);
    }
    if new_pack.is_none() {
        let existing =
            files::safe_join(base, &format!("resourcepacks/{FILE}")).map_err(|e| e.to_string())?;
        match std::fs::read(existing) {
            Ok(bytes) => check_owned(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("读取皮肤资源包失败：{error}")),
        }
    }
    let updated = options(text, new_pack.is_some())?;
    if let Some(bytes) = new_pack {
        files::create_dirs(base, "resourcepacks").map_err(|e| e.to_string())?;
        let dest =
            files::safe_join(base, &format!("resourcepacks/{FILE}")).map_err(|e| e.to_string())?;
        let old = match std::fs::read(&dest) {
            Ok(data) => {
                check_owned(&data)?;
                Some(data)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("读取皮肤资源包失败：{e}")),
        };
        atomic_write(&dest, &bytes).map_err(|e| format!("写入皮肤资源包失败：{e}"))?;
        if updated.as_bytes() != original {
            if let Err(error) = atomic_write(&options_path, updated.as_bytes()) {
                let rollback = match old {
                    Some(old) => atomic_write(&dest, &old),
                    None => std::fs::remove_file(&dest).map_err(|e| e.to_string()),
                };
                return Err(match rollback {
                    Ok(()) => format!("启用皮肤资源包失败：{error}"),
                    Err(e) => format!("启用皮肤资源包失败：{error}；恢复资源包失败：{e}"),
                });
            }
        }
    } else if updated.as_bytes() != original {
        atomic_write(&options_path, updated.as_bytes())
            .map_err(|e| format!("停用离线皮肤资源包失败：{e}"))?;
    }
    Ok(warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_one_texture_preserves_options_and_disables_on_account_change() {
        let root = std::env::temp_dir().join(format!("memoria-pack-{}", uuid::Uuid::new_v4()));
        let game = root.join("game");
        let data = root.join("data");
        std::fs::create_dir_all(&game).unwrap();
        let account = AuthAccount::offline("Alex");
        let source = root.join("skin.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba([32, 45, 55, 255]))
            .save(&source)
            .unwrap();
        crate::account_skin::save_offline(&data, &account, &source).unwrap();
        let initial = "fullscreen:false\r\nresourcePacks:[\"vanilla\",\"file/custom.zip\"]\r\nlastServer:local\r\n";
        std::fs::write(game.join("options.txt"), initial).unwrap();
        prepare(&game, &account, &data, "1.20.1").unwrap();
        let updated = std::fs::read_to_string(game.join("options.txt")).unwrap();
        assert!(
            updated.contains("[\"vanilla\",\"file/custom.zip\",\"file/Memoria-Offline-Skin.zip\"]")
        );
        assert!(updated.contains("lastServer:local\r\n"));
        let bytes = std::fs::read(game.join("resourcepacks").join(FILE)).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert!(zip.by_name(&target(&account.uuid).unwrap()).is_ok());
        let mut metadata = String::new();
        zip.by_name("pack.mcmeta")
            .unwrap()
            .read_to_string(&mut metadata)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["pack"]["pack_format"],
            15
        );
        prepare(&game, &account, &data, "1.21.1").unwrap();
        assert!(prepare(&game, &account, &data, "1.22").unwrap().is_some());
        assert_eq!(
            std::fs::read_to_string(game.join("options.txt")).unwrap(),
            initial
        );
        prepare(&game, &account, &data, "1.21.1").unwrap();
        let other = AuthAccount::offline("Other");
        prepare(&game, &other, &data, "1.21.1").unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("options.txt")).unwrap(),
            initial
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_unowned_pack_and_malformed_options() {
        assert!(options("resourcePacks:not-json\n", true).is_err());
        assert!(options("resourcePacks:[]\nresourcePacks:[]\n", true).is_err());
        assert!(check_owned(b"unrelated").is_err());
        assert_eq!(
            target("00000000-0000-0000-0000-000000000000").unwrap(),
            "assets/minecraft/textures/entity/player/slim/alex.png"
        );
        assert!(!supported("1.22"));
    }
}
