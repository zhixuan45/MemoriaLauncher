//! Portable content locks, mrpack exchange, and a staged new-instance installer.
pub mod exchange;
pub mod install;
pub use install::{
    install as install_new_instance, preview as preview_new_instance, Plan as NewInstancePlan,
    Request as NewInstanceRequest,
};
pub mod lockfile;
use crate::runtime::loaders::Kind;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loader {
    pub kind: Kind,
    pub version: String,
}
impl Loader {
    pub fn dependency(&self) -> &'static str {
        match self.kind {
            Kind::Fabric => "fabric-loader",
            Kind::Quilt => "quilt-loader",
            Kind::Forge => "forge",
            Kind::NeoForge => "neoforge",
        }
    }
    pub fn catalog_version(&self, game: &str) -> String {
        if self.kind == Kind::Forge && !self.version.starts_with(&format!("{game}-")) {
            format!("{game}-{}", self.version)
        } else {
            self.version.clone()
        }
    }
}
pub(crate) fn component(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 160
        || value.contains("..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
    {
        Err("版本或实例标识无效".into())
    } else {
        Ok(())
    }
}
pub(crate) fn normalized_loader(
    kind: crate::LoaderType,
    version: &str,
    game: &str,
) -> Option<Loader> {
    let kind = match kind {
        crate::LoaderType::Fabric => Kind::Fabric,
        crate::LoaderType::Quilt => Kind::Quilt,
        crate::LoaderType::Forge => Kind::Forge,
        crate::LoaderType::NeoForge => Kind::NeoForge,
        _ => return None,
    };
    Some(Loader {
        kind,
        version: version
            .strip_prefix(&format!("{game}-"))
            .unwrap_or(version)
            .into(),
    })
}

#[cfg(test)]
mod tests;
