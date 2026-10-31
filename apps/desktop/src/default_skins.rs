//! Vanilla 1.20.1/1.21.1 DefaultPlayerSkin selection (Java UUID.hashCode + floorMod).
use iced::widget::image::Handle;
use std::sync::LazyLock;

const NAMES: [&str; 9] = [
    "Alex", "Ari", "Efe", "Kai", "Makena", "Noor", "Steve", "Sunny", "Zuri",
];
const SKINS: [&[u8]; 18] = [
    include_bytes!("../ui/assets/default-skins/slim-alex.png"),
    include_bytes!("../ui/assets/default-skins/slim-ari.png"),
    include_bytes!("../ui/assets/default-skins/slim-efe.png"),
    include_bytes!("../ui/assets/default-skins/slim-kai.png"),
    include_bytes!("../ui/assets/default-skins/slim-makena.png"),
    include_bytes!("../ui/assets/default-skins/slim-noor.png"),
    include_bytes!("../ui/assets/default-skins/slim-steve.png"),
    include_bytes!("../ui/assets/default-skins/slim-sunny.png"),
    include_bytes!("../ui/assets/default-skins/slim-zuri.png"),
    include_bytes!("../ui/assets/default-skins/wide-alex.png"),
    include_bytes!("../ui/assets/default-skins/wide-ari.png"),
    include_bytes!("../ui/assets/default-skins/wide-efe.png"),
    include_bytes!("../ui/assets/default-skins/wide-kai.png"),
    include_bytes!("../ui/assets/default-skins/wide-makena.png"),
    include_bytes!("../ui/assets/default-skins/wide-noor.png"),
    include_bytes!("../ui/assets/default-skins/wide-steve.png"),
    include_bytes!("../ui/assets/default-skins/wide-sunny.png"),
    include_bytes!("../ui/assets/default-skins/wide-zuri.png"),
];

static AVATARS: LazyLock<[Handle; 18]> = LazyLock::new(|| {
    std::array::from_fn(|index| {
        crate::account_skin::avatar(SKINS[index]).expect("bundled default skin")
    })
});

pub fn index(uuid: &str) -> Result<usize, String> {
    let uuid = uuid::Uuid::parse_str(uuid).map_err(|_| "账户 UUID 无效")?;
    let hash = uuid.as_bytes().chunks_exact(4).fold(0u32, |hash, bytes| {
        hash ^ u32::from_be_bytes(bytes.try_into().unwrap())
    }) as i32;
    Ok(hash.rem_euclid(18) as usize)
}

pub fn avatar(uuid: &str) -> Handle {
    AVATARS[index(uuid).unwrap_or(15)].clone()
}

pub fn name(uuid: &str) -> &'static str {
    NAMES[index(uuid).unwrap_or(15) % 9]
}

pub fn texture_path(uuid: &str) -> Result<String, String> {
    let index = index(uuid)?;
    Ok(format!(
        "assets/minecraft/textures/entity/player/{}/{}.png",
        if index < 9 { "slim" } else { "wide" },
        NAMES[index % 9].to_ascii_lowercase()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_java_signed_hash_and_all_bundled_textures_decode() {
        assert_eq!(index("00000000-0000-0000-0000-000000000000").unwrap(), 0);
        assert_eq!(index("80000000-0000-0000-0000-000000000000").unwrap(), 16);
        for skin in SKINS {
            crate::account_skin::avatar(skin).unwrap();
        }
    }
}
