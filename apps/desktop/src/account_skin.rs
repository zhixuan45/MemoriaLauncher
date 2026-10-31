use base64::{engine::general_purpose::STANDARD, Engine};
use iced::widget::image::Handle;
use image::{GenericImageView, Pixel};
use mc_core::AuthAccount;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skin {
    pub avatar: Handle,
    pub description: String,
    pub custom: bool,
    pub warning: Option<String>,
}

impl Skin {
    pub fn default_for(account: &AuthAccount) -> Self {
        Self {
            avatar: fallback(account),
            description: format!("默认皮肤 · {}", crate::default_skins::name(&account.uuid)),
            custom: false,
            warning: None,
        }
    }
}

pub fn skin_path(root: &Path, account: &AuthAccount) -> Result<PathBuf, String> {
    let id = uuid::Uuid::parse_str(&account.uuid).map_err(|_| "账户 UUID 无效")?;
    Ok(root.join("account-skins").join(format!("{id}.png")))
}

pub fn avatar(bytes: &[u8]) -> Result<Handle, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("皮肤文件不能大于 1 MiB".into());
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(64);
    limits.max_image_height = Some(64);
    limits.max_alloc = Some(1024 * 1024);
    reader.limits(limits);
    let skin = reader.decode().map_err(|e| format!("皮肤 PNG 无效：{e}"))?;
    if !matches!(skin.dimensions(), (64, 64) | (64, 32)) {
        return Err("皮肤尺寸必须为 64×64 或 64×32。".into());
    }
    let mut face = image::RgbaImage::new(8, 8);
    // Legacy RGB skins often fill the unused hat area with opaque black. Vanilla
    // clears that area when it contains no transparent pixels.
    let opaque_legacy_hat =
        skin.height() == 32 && (0..32).all(|y| (32..64).all(|x| skin.get_pixel(x, y)[3] >= 128));
    for y in 0..8 {
        for x in 0..8 {
            let mut pixel = skin.get_pixel(x + 8, y + 8);
            pixel[3] = 255;
            if !opaque_legacy_hat {
                pixel.blend(&skin.get_pixel(x + 40, y + 8));
            }
            pixel[3] = 255;
            face.put_pixel(x, y, pixel);
        }
    }
    Ok(Handle::from_rgba(8, 8, face.into_raw()))
}

pub fn fallback(account: &AuthAccount) -> Handle {
    crate::default_skins::avatar(&account.uuid)
}

fn save_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().ok_or("皮肤目录无效")?).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("保存皮肤失败：{error}"));
    }
    Ok(())
}

pub fn save_offline(root: &Path, account: &AuthAccount, source: &Path) -> Result<Skin, String> {
    if !account.is_offline() {
        return Err("只能为离线账户选择本地皮肤。".into());
    }
    let metadata = std::fs::metadata(source).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("皮肤文件必须小于 1 MiB。".into());
    }
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    let handle = avatar(&bytes)?;
    let path = skin_path(root, account)?;
    save_bytes(&path, &bytes)?;
    Ok(Skin {
        avatar: handle,
        description: "本地自定义皮肤".into(),
        custom: true,
        warning: None,
    })
}

pub async fn reset_offline(root: PathBuf, account: AuthAccount) -> Result<Skin, String> {
    if !account.is_offline() {
        return Err("只能重置离线账户的本地皮肤。".into());
    }
    match tokio::fs::remove_file(skin_path(&root, &account)?).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("重置皮肤失败：{e}")),
    }
    Ok(Skin::default_for(&account))
}

async fn read_skin(path: &Path) -> Result<Option<Handle>, String> {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.len() > 1024 * 1024 || !meta.is_file() => {
            return Err("皮肤文件不能大于 1 MiB".into())
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    avatar(&tokio::fs::read(path).await.map_err(|e| e.to_string())?).map(Some)
}

fn cache_path(root: &Path, account: &AuthAccount) -> Result<PathBuf, String> {
    Ok(root.join("cache/account-skins").join(
        skin_path(root, account)?
            .file_name()
            .ok_or("皮肤路径无效")?,
    ))
}

pub async fn cached(root: &Path, account: &AuthAccount) -> Result<Option<Skin>, String> {
    Ok(read_skin(&cache_path(root, account)?)
        .await?
        .map(|avatar| Skin {
            avatar,
            description: "已缓存的账户皮肤".into(),
            custom: false,
            warning: None,
        }))
}

pub async fn load(root: PathBuf, account: AuthAccount) -> Result<Skin, String> {
    if account.is_offline() {
        let path = skin_path(&root, &account)?;
        return Ok(match read_skin(&path).await? {
            Some(avatar) => Skin {
                avatar,
                description: "本地自定义皮肤".into(),
                custom: true,
                warning: None,
            },
            None => Skin::default_for(&account),
        });
    }
    let result = download(&account).await;
    apply_download(&root, &account, result).await
}

async fn apply_download(
    root: &Path,
    account: &AuthAccount,
    result: Result<Option<Vec<u8>>, String>,
) -> Result<Skin, String> {
    let path = cache_path(root, account)?;
    match result {
        Ok(Some(bytes)) => {
            let avatar = avatar(&bytes)?;
            let warning = tokio::task::spawn_blocking(move || save_bytes(&path, &bytes))
                .await
                .map_err(|e| e.to_string())?
                .err();
            Ok(Skin {
                avatar,
                description: "Microsoft 账户皮肤".into(),
                custom: false,
                warning,
            })
        }
        Ok(None) => {
            // An account may have removed its custom skin since the last refresh.
            match tokio::fs::remove_file(path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("清理旧皮肤缓存失败：{e}")),
            }
            Ok(Skin::default_for(account))
        }
        Err(error) => match cached(root, account).await {
            Ok(Some(mut skin)) => {
                skin.warning = Some(format!("皮肤刷新失败，使用缓存：{error}"));
                Ok(skin)
            }
            _ => Err(format!("皮肤加载失败：{error}")),
        },
    }
}

fn request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "连接皮肤服务超时，请稍后刷新。".into()
    } else if let Some(status) = error.status() {
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            "皮肤服务请求过于频繁，请稍后刷新。".into()
        } else {
            format!("皮肤服务返回 HTTP {}，请稍后刷新。", status.as_u16())
        }
    } else {
        "无法读取皮肤服务，请检查网络后刷新。".into()
    }
}

async fn download(account: &AuthAccount) -> Result<Option<Vec<u8>>, String> {
    let id = uuid::Uuid::parse_str(&account.uuid)
        .map_err(|_| "账户 UUID 无效")?
        .simple()
        .to_string();
    mc_core::service::online_catalog::init_tls_crypto();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    async fn get(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(request_error)?
            .error_for_status()
            .map_err(request_error)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if bytes.len() + chunk.len() > limit {
                return Err("皮肤响应过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    let profile = get(
        &client,
        &format!("https://sessionserver.mojang.com/session/minecraft/profile/{id}"),
        64 * 1024,
    )
    .await?;
    let profile: serde_json::Value = serde_json::from_slice(&profile).map_err(|e| e.to_string())?;
    let encoded = profile["properties"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["name"] == "textures"))
        .and_then(|v| v["value"].as_str());
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    let decoded = STANDARD.decode(encoded).map_err(|e| e.to_string())?;
    let textures: serde_json::Value =
        serde_json::from_slice(&decoded).map_err(|e| e.to_string())?;
    let Some(raw) = textures["textures"]["SKIN"]["url"].as_str() else {
        return Ok(None);
    };
    let parsed = url::Url::parse(raw).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str() != Some("textures.minecraft.net")
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("皮肤地址无效".into());
    }
    let mut safe = parsed;
    safe.set_scheme("https").map_err(|_| "皮肤地址无效")?;
    let bytes = get(&client, safe.as_str(), 1024 * 1024).await?;
    avatar(&bytes)?;
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(image: image::RgbaImage) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn offline_skin_round_trip_extracts_face_and_outer_layer() {
        let root = std::env::temp_dir().join(format!("memoria-skin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("input.png");
        let mut skin = image::RgbaImage::new(64, 64);
        skin.put_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        skin.put_pixel(40, 8, image::Rgba([200, 40, 50, 255]));
        std::fs::write(&source, png(skin)).unwrap();
        let account = AuthAccount::offline("Alex");
        save_offline(&root, &account, &source).unwrap();
        let Handle::Rgba {
            pixels,
            width,
            height,
            ..
        } = load(root.clone(), account).await.unwrap().avatar
        else {
            panic!("expected pixels")
        };
        assert_eq!((width, height), (8, 8));
        assert_eq!(&pixels[..4], &[200, 40, 50, 255]);
        assert!(avatar(&png(image::RgbaImage::new(32, 32))).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn offline_default_custom_replacement_restart_and_reset() {
        let root = std::env::temp_dir().join(format!("memoria-skin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let account = AuthAccount::offline("Player");
        let initial = load(root.clone(), account.clone()).await.unwrap();
        assert!(!initial.custom);
        assert_eq!(initial.avatar, fallback(&account));
        let source = root.join("input.png");
        for color in [[1, 2, 3, 255], [4, 5, 6, 255]] {
            std::fs::write(
                &source,
                png(image::RgbaImage::from_pixel(64, 64, image::Rgba(color))),
            )
            .unwrap();
            save_offline(&root, &account, &source).unwrap();
            let reloaded = load(root.clone(), account.clone()).await.unwrap();
            assert!(reloaded.custom);
            let Handle::Rgba { pixels, .. } = reloaded.avatar else {
                panic!("expected pixels")
            };
            assert_eq!(&pixels[..4], &color);
        }
        std::fs::write(&source, b"invalid PNG").unwrap();
        assert!(save_offline(&root, &account, &source).is_err());
        assert!(load(root.clone(), account.clone()).await.unwrap().custom);
        let reset = reset_offline(root.clone(), account.clone()).await.unwrap();
        assert_eq!(reset.avatar, initial.avatar);
        assert!(!load(root.clone(), account).await.unwrap().custom);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn online_cache_survives_failed_refresh_and_removed_skin_clears_cache() {
        let root = std::env::temp_dir().join(format!("memoria-skin-{}", uuid::Uuid::new_v4()));
        let account = AuthAccount {
            id: "online".into(),
            username: "Player".into(),
            uuid: uuid::Uuid::new_v4().to_string(),
        };
        assert!(apply_download(&root, &account, Err("timeout".into()))
            .await
            .is_err());
        let bytes = png(image::RgbaImage::from_pixel(
            64,
            64,
            image::Rgba([1, 2, 3, 255]),
        ));
        apply_download(&root, &account, Ok(Some(bytes)))
            .await
            .unwrap();
        assert!(cached(&root, &account).await.unwrap().is_some());
        let cached_skin = apply_download(&root, &account, Err("timeout".into()))
            .await
            .unwrap();
        assert!(cached_skin.warning.unwrap().contains("timeout"));
        let Handle::Rgba { pixels, .. } = cached_skin.avatar else {
            panic!("expected pixels")
        };
        assert_eq!(&pixels[..4], &[1, 2, 3, 255]);
        apply_download(&root, &account, Ok(None)).await.unwrap();
        assert!(cached(&root, &account).await.unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_rgb_skin_ignores_unused_opaque_hat_and_blends_real_transparency() {
        let mut skin = image::RgbaImage::from_pixel(64, 32, image::Rgba([0, 0, 0, 255]));
        skin.put_pixel(8, 8, image::Rgba([50, 100, 150, 255]));
        let Handle::Rgba { pixels, .. } = avatar(&png(skin.clone())).unwrap() else {
            panic!("expected pixels")
        };
        assert_eq!(&pixels[..4], &[50, 100, 150, 255]);
        skin.put_pixel(32, 0, image::Rgba([0, 0, 0, 0]));
        skin.put_pixel(40, 8, image::Rgba([150, 100, 50, 128]));
        let Handle::Rgba { pixels, .. } = avatar(&png(skin)).unwrap() else {
            panic!("expected pixels")
        };
        assert_eq!(pixels[3], 255);
        assert!((99..=101).contains(&pixels[0]));
        assert!((99..=101).contains(&pixels[2]));
    }

    #[tokio::test]
    #[ignore = "requires Mojang session and texture services"]
    async fn live_account_skin_download() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/account-skin/live");
        let account = AuthAccount {
            id: "skin-probe".into(),
            username: "SkinProbe".into(),
            uuid: std::env::var("MEMORIA_TEST_SKIN_UUID").expect("Set MEMORIA_TEST_SKIN_UUID"),
        };
        let skin = load(root, account).await.unwrap();
        assert!(skin.warning.is_none(), "{:?}", skin.warning);
        eprintln!("{}", skin.description);
    }
}
