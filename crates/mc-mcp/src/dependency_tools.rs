use super::*;
use mc_core::service::{
    instance_write::files,
    mod_resolver::{self, remote::Pin},
};
pub fn definition() -> Value {
    json!({"name":"minecraft_resolve_dependencies","description":"Read-only local Mod JAR prerequisite/conflict audit. Optional pinned Modrinth roots produce a backtracking install plan while preserving other installed projects; no files are written. Requires instance permission; remote roots also require catalog permission.","annotations":{"readOnlyHint":true},"inputSchema":{"type":"object","properties":{
    "instance_id":{"type":"string","minLength":1,"maxLength":240},
    "roots":{"type":"array","maxItems":16,"items":{"type":"object","properties":{"project":{"type":"string","minLength":1,"maxLength":100},"version":{"type":"string","minLength":1,"maxLength":100}},"required":["project","version"],"additionalProperties":false}}
},"required":["instance_id"],"additionalProperties":false}})
}
pub async fn call(args: &Value, root: &Path) -> Result<Value, (String, String)> {
    verify_args(args, &["instance_id", "roots"])
        .map_err(|e| ("INVALID_ARGUMENT".into(), e.into()))?;
    let instance = string_arg(args, "instance_id")
        .filter(|v| v.len() <= 240)
        .ok_or_else(|| ("INVALID_ARGUMENT".into(), "instance_id is required".into()))?;
    let roots: Vec<Pin> = args
        .get("roots")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|_| {
            (
                "INVALID_ARGUMENT".into(),
                "roots must contain pinned project/version objects".into(),
            )
        })?
        .unwrap_or_default();
    if roots.len() > 16 {
        return Err(("INVALID_ARGUMENT".into(), "at most 16 roots".into()));
    }
    let inventory = mod_resolver::inventory::inspect_instance(root, instance)
        .await
        .map_err(|e| ("DEPENDENCY_SCAN_FAILED".into(), e))?;
    let report = inventory.audit();
    let mut remote = Value::Null;
    if !roots.is_empty() {
        let base = files::instance_dir(root, instance)
            .map_err(|e| ("DEPENDENCY_SCAN_FAILED".into(), e.to_string()))?;
        let path = files::safe_join(&base, ".memoria/resource-install.json")
            .map_err(|e| ("DEPENDENCY_SCAN_FAILED".into(), e.to_string()))?;
        let mut installed = vec![];
        if path.exists() {
            let bytes = files::read_bounded(&path, 1024 * 1024)
                .map_err(|e| ("SOURCE_RECORD_INVALID".into(), e.to_string()))?;
            let manifest: Value = serde_json::from_slice(&bytes)
                .map_err(|e| ("SOURCE_RECORD_INVALID".into(), e.to_string()))?;
            let records = manifest["resources"].as_array().ok_or_else(|| {
                (
                    "SOURCE_RECORD_INVALID".into(),
                    "resources list missing".into(),
                )
            })?;
            for record in records {
                let Some(path) = record["path"].as_str().filter(|p| p.starts_with("mods/")) else {
                    continue;
                };
                let filename = path.trim_start_matches("mods/");
                let Some((_, hash)) = inventory.files.iter().find(|(name, _)| name == filename)
                else {
                    continue;
                };
                if record["sha256"].as_str() != Some(hash.as_str()) {
                    return Err((
                        "SOURCE_RECORD_INVALID".into(),
                        "record does not match installed bytes".into(),
                    ));
                }
                if let (Some(project), Some(version)) =
                    (record["project"].as_str(), record["version"].as_str())
                {
                    installed.push(Pin {
                        project: project.into(),
                        version: version.into(),
                    });
                }
            }
        }
        let catalog = mc_core::ResourceCatalog::new()
            .map_err(|e| ("CATALOG_UNAVAILABLE".into(), e.to_string()))?;
        remote = serde_json::to_value(
            mod_resolver::remote::resolve(
                &catalog,
                &roots,
                &installed,
                &inventory.minecraft,
                &inventory.loader,
            )
            .await
            .map_err(|e| ("DEPENDENCY_RESOLVE_FAILED".into(), e))?,
        )
        .unwrap();
    }
    let digest = mod_resolver::inventory::environment_digest(root, instance)
        .map_err(|e| ("DEPENDENCY_SCAN_FAILED".into(), e))?;
    Ok(
        json!({"instance_id":instance,"read_only":true,"local_report":report,"remote_plan":remote,"snapshot":{"environment_digest":digest,"mods":inventory.files},"notice":"Plans are finite static checks, not approval to install. Unknown metadata is not proof of compatibility."}),
    )
}
