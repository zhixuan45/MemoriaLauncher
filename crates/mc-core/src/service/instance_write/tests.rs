use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
};
struct Fixture {
    root: PathBuf,
    base: PathBuf,
    policy: McpPolicy,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("memoria-write-{}", uuid::Uuid::new_v4()));
        let base = root.join("versions/pack");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("pack.json"),
            json!({"id":"pack","mainClass":"net.minecraft.client.main.Main","type":"release"})
                .to_string(),
        )
        .unwrap();
        let mut policy = McpPolicy {
            enabled: true,
            game_root: root.clone(),
            allowed_instances: vec!["versions/pack".into()],
            ..Default::default()
        };
        policy.permissions.write_mod_list = true;
        policy.permissions.write_mod_files = true;
        policy.permissions.write_configs = true;
        policy.permissions.write_quests = true;
        Self { root, base, policy }
    }
    fn registry(&self) {
        let source = self.root.join("registry.json");
        std::fs::write(&source,json!({"minecraft:item":{"entries":{"minecraft:oak_log":{},"minecraft:apple":{},"demo:gear":{}}}}).to_string()).unwrap();
        items::import_registry(&self.root, "versions/pack", &source).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
pub(super) fn jar(id: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(vec![]));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    write!(
        archive,
        "{}",
        json!({"schemaVersion":1,"id":id,"version":"1.0"})
    )
    .unwrap();
    archive.finish().unwrap().into_inner()
}
fn config(value: bool) -> Change {
    Change::Config {
        path: "config/demo.json".into(),
        content: json!({"enabled":value}).to_string(),
    }
}
#[tokio::test]
async fn desktop_review_applies_only_the_exact_local_plan_without_granting_mcp_permissions() {
    let f=Fixture::new();
    let preview=prepare(&f.policy,"versions/pack",&config(true)).await.unwrap();
    let listed=list_local_plans(&f.root,"versions/pack").unwrap();
    assert_eq!(listed.len(),1);assert_eq!(listed[0].state,"pending");
    let review=review_local_plan(&f.root,"versions/pack",&preview.plan_id).unwrap();
    assert_eq!(review.preview.plan_hash,preview.plan_hash);
    assert!(review.preview.files[0]["text_preview"].as_str().unwrap().contains("true"));
    assert!(!f.base.join("config/demo.json").exists());
    let mut denied=f.policy.clone();denied.permissions.write_configs=false;
    assert_eq!(apply(&denied,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap_err().code,"PERMISSION_DENIED");
    assert_eq!(apply_local_plan(&f.root,"versions/pack",&preview.plan_id,"bad hash").unwrap_err().code,"PLAN_HASH_MISMATCH");
    apply_local_plan(&f.root,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap();
    assert!(!denied.permissions.write_configs);
    assert_eq!(apply(&denied,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap_err().code,"PERMISSION_DENIED");
    assert_eq!(list_local_plans(&f.root,"versions/pack").unwrap()[0].state,"applied");
    assert_eq!(crate::service::file_transaction::history(&f.root,"versions/pack").unwrap()[0].state,"committed");
}
#[tokio::test]
async fn cancelled_stale_and_changed_plans_cannot_be_approved() {
    let f=Fixture::new();
    let preview=prepare(&f.policy,"versions/pack",&config(true)).await.unwrap();
    cancel_local_plan(&f.root,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap();
    assert_eq!(apply(&f.policy,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap_err().code,"PLAN_CANCELLED");
    assert!(!f.base.join("config/demo.json").exists());
    let preview=prepare(&f.policy,"versions/pack",&config(true)).await.unwrap();
    std::fs::create_dir_all(f.base.join("config")).unwrap();
    std::fs::write(f.base.join("config/demo.json"),b"external").unwrap();
    assert_eq!(review_local_plan(&f.root,"versions/pack",&preview.plan_id).unwrap().plan.state,"stale");
    assert_eq!(apply_local_plan(&f.root,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap_err().code,"PLAN_STALE");
    assert_eq!(std::fs::read(f.base.join("config/demo.json")).unwrap(),b"external");
}
#[tokio::test]
async fn a_committed_transaction_without_a_mcp_receipt_is_never_applied_again() {
    let f=Fixture::new();
    let preview=prepare(&f.policy,"versions/pack",&config(true)).await.unwrap();
    let base=f.base.clone();
    let operations=[crate::service::file_transaction::Operation {path:"config/demo.json".into(),old_sha256:None,staged:Some(format!(".launcher/mcp/plans/{}/0.bin",preview.plan_id))}];
    {
        let _lock=files::lock(&base).unwrap();
        crate::service::file_transaction::execute_locked(&base,&preview.plan_id,"mcp_instance_write",&operations).unwrap();
    }
    assert_eq!(apply(&f.policy,"versions/pack",&preview.plan_id,&preview.plan_hash).unwrap()["status"],"already_applied");
    assert_eq!(crate::service::file_transaction::history(&f.root,"versions/pack").unwrap().len(),1);
}
fn entry(id: &str) -> ftb::QuestEntry {
    serde_json::from_value(json!({"id":id,"title":"开始冒险","description":["收集原木"],"icon":"minecraft:oak_log","completion":[{"type":"item","item":"minecraft:oak_log","count":16}],"rewards":[{"type":"item","item":"minecraft:apple","count":4}],"position":{"x":0.14,"y":0}})).unwrap()
}
fn quest(input: ftb::QuestEntry) -> Change {
    Change::QuestEntry {
        chapter: "start".into(),
        chapter_title: Some("起步".into()),
        entry: input,
    }
}
#[tokio::test]
async fn config_preview_apply_backup_idempotence_and_staleness() {
    let f = Fixture::new();
    let plan = prepare(&f.policy, "versions/pack", &config(true))
        .await
        .unwrap();
    assert!(!f.base.join("config/demo.json").exists());
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, "bad")
            .unwrap_err()
            .code,
        "PLAN_HASH_MISMATCH"
    );
    let applied = apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash).unwrap();
    assert_eq!(applied["status"], "applied");
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash).unwrap()["status"],
        "already_applied"
    );
    let second = prepare(&f.policy, "versions/pack", &config(false))
        .await
        .unwrap();
    apply(
        &f.policy,
        "versions/pack",
        &second.plan_id,
        &second.plan_hash,
    )
    .unwrap();
    let backup = f.base.join(format!(
        ".launcher/mcp/backups/{}/config/demo.json",
        second.plan_id
    ));
    assert!(std::fs::read_to_string(backup).unwrap().contains("true"));
    let stale = prepare(&f.policy, "versions/pack", &config(true))
        .await
        .unwrap();
    std::fs::write(f.base.join("config/demo.json"), "{}").unwrap();
    assert_eq!(
        apply(&f.policy, "versions/pack", &stale.plan_id, &stale.plan_hash)
            .unwrap_err()
            .code,
        "PLAN_STALE"
    );
}
#[tokio::test]
async fn permissions_checked_at_prepare_and_again_on_apply() {
    let mut f = Fixture::new();
    let plan = prepare(&f.policy, "versions/pack", &config(true))
        .await
        .unwrap();
    f.policy.permissions.write_configs = false;
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash)
            .unwrap_err()
            .code,
        "PERMISSION_DENIED"
    );
    assert_eq!(
        prepare(&f.policy, "versions/pack", &config(true))
            .await
            .unwrap_err()
            .code,
        "PERMISSION_DENIED"
    );
    assert_eq!(
        prepare(&f.policy, "versions/other", &config(true))
            .await
            .unwrap_err()
            .code,
        "PERMISSION_DENIED"
    );
    f.policy.enabled = false;
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash)
            .unwrap_err()
            .code,
        "PERMISSION_DENIED"
    );
    assert!(!f.base.join("config/demo.json").exists());
}
#[tokio::test]
async fn config_paths_and_formats_cannot_bypass_task_or_external_scope() {
    let f = Fixture::new();
    for path in [
        "../outside.json",
        "config/../outside.json",
        "config/ftbquests/quests/a.snbt",
        "defaultconfigs/FTBQuests/a.json",
        "config/a:stream.json",
        "config/CON.json",
        "config/a./b.json",
        "config/x\\y.json",
        "mods/a.jar",
        "config/a.exe",
    ] {
        assert!(
            prepare(
                &f.policy,
                "versions/pack",
                &Change::Config {
                    path: path.into(),
                    content: "{}".into()
                }
            )
            .await
            .is_err(),
            "{path}"
        );
    }
    for (path, content) in [
        ("config/bad.json", "{no"),
        ("config/bad.toml", "a=["),
        ("config/bad.snbt", "{broken"),
    ] {
        assert_eq!(
            prepare(
                &f.policy,
                "versions/pack",
                &Change::Config {
                    path: path.into(),
                    content: content.into()
                }
            )
            .await
            .unwrap_err()
            .code,
            "INVALID_CONFIG"
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_rejected_during_prepare_and_after_preview() {
    let f = Fixture::new();
    let outside = f.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, f.base.join("config")).unwrap();
    assert_eq!(
        prepare(&f.policy, "versions/pack", &config(true))
            .await
            .unwrap_err()
            .code,
        "UNSAFE_PATH"
    );
    std::fs::remove_file(f.base.join("config")).unwrap();
    let plan = prepare(&f.policy, "versions/pack", &config(true))
        .await
        .unwrap();
    std::os::unix::fs::symlink(&outside, f.base.join("config")).unwrap();
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash)
            .unwrap_err()
            .code,
        "UNSAFE_PATH"
    );
    assert!(!outside.join("demo.json").exists());
}
#[tokio::test]
async fn direct_jar_requires_integrity_and_parsed_mod_metadata() {
    let f = Fixture::new();
    let bytes = jar("demo");
    let mut change = Change::ModFile {
        filename: "demo.jar".into(),
        data_base64: STANDARD.encode(&bytes),
        sha256: digest(&bytes),
    };
    let plan = prepare(&f.policy, "versions/pack", &change).await.unwrap();
    apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash).unwrap();
    assert_eq!(std::fs::read(f.base.join("mods/demo.jar")).unwrap(), bytes);
    if let Change::ModFile { sha256, .. } = &mut change {
        *sha256 = "0".repeat(64)
    }
    assert_eq!(
        prepare(&f.policy, "versions/pack", &change)
            .await
            .unwrap_err()
            .code,
        "FILE_HASH_MISMATCH"
    );
    assert!(validate_jar(b"not jar").is_err());
    assert!(transaction::decode("%bad%", 10).is_err());
}
#[tokio::test]
async fn task_icons_conditions_rewards_parent_grid_and_unknown_fields() {
    let f = Fixture::new();
    f.registry();
    let first = prepare(&f.policy, "versions/pack", &quest(entry("start")))
        .await
        .unwrap();
    assert_eq!(first.details["position"], json!({"x":0.1,"y":0.0}));
    apply(&f.policy, "versions/pack", &first.plan_id, &first.plan_hash).unwrap();
    let path = f.base.join("config/ftbquests/quests/chapters/start.snbt");
    let mut doc = snbt::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    doc.object_mut()
        .unwrap()
        .insert("custom_field".into(), snbt::Node::string("keep me"));
    std::fs::write(&path, doc.render()).unwrap();
    let mut child = entry("second");
    child.parent = Some("start".into());
    let plan = prepare(&f.policy, "versions/pack", &quest(child))
        .await
        .unwrap();
    apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("keep me"));
    assert!(text.contains("item:minecraft:oak_log"));
    assert!(text.contains("16L"));
    let read = ftb::read(&f.base).unwrap();
    assert_eq!(read["chapters"][0]["entries"].as_array().unwrap().len(), 2);
    let mut parent = entry("start");
    parent.parent = Some("second".into());
    assert!(prepare(&f.policy, "versions/pack", &quest(parent))
        .await
        .unwrap_err()
        .message
        .contains("循环"));
    let mut absent = entry("third");
    absent.parent = Some("absent".into());
    assert!(prepare(&f.policy, "versions/pack", &quest(absent))
        .await
        .unwrap_err()
        .message
        .contains("不存在"));
}
#[tokio::test]
async fn item_checks_require_registry_real_membership_and_installed_namespace() {
    let f = Fixture::new();
    assert_eq!(
        prepare(&f.policy, "versions/pack", &quest(entry("first")))
            .await
            .unwrap_err()
            .code,
        "ITEM_REGISTRY_REQUIRED"
    );
    f.registry();
    let ids = vec![
        "Minecraft:stone".into(),
        "absent:item".into(),
        "minecraft:missing".into(),
        "minecraft:oak_log".into(),
        "demo:gear".into(),
    ];
    let checked = items::validate(&f.base, &ids).unwrap();
    assert_eq!(
        checked.iter().map(|v| v.code.as_str()).collect::<Vec<_>>(),
        vec![
            "INVALID_ITEM_ID",
            "MOD_NOT_INSTALLED",
            "ITEM_NOT_FOUND",
            "OK",
            "MOD_NOT_INSTALLED"
        ]
    );
    std::fs::create_dir(f.base.join("mods")).unwrap();
    std::fs::write(f.base.join("mods/demo.jar"), jar("demo")).unwrap();
    assert_eq!(
        items::validate(&f.base, &["minecraft:oak_log".into()]).unwrap()[0].code,
        "ITEM_REGISTRY_STALE"
    );
    f.registry();
    assert!(items::validate(&f.base, &["demo:gear".into()]).unwrap()[0].valid);
    let mut bad = entry("invalid");
    bad.icon = Some("minecraft:missing".into());
    assert_eq!(
        prepare(&f.policy, "versions/pack", &quest(bad))
            .await
            .unwrap_err()
            .code,
        "ITEM_NOT_FOUND"
    );
    let mut bad = entry("invalid");
    bad.rewards = vec![ftb::Reward::Item {
        item: "minecraft:missing".into(),
        count: 1,
    }];
    assert_eq!(
        prepare(&f.policy, "versions/pack", &quest(bad))
            .await
            .unwrap_err()
            .code,
        "ITEM_NOT_FOUND"
    );
}
#[tokio::test]
async fn changed_registry_or_new_chapter_invalidates_quest_preview() {
    let f = Fixture::new();
    f.registry();
    let plan = prepare(&f.policy, "versions/pack", &quest(entry("first")))
        .await
        .unwrap();
    std::fs::write(f.base.join("pack.json"), "{}").unwrap();
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash)
            .unwrap_err()
            .code,
        "PLAN_STALE"
    );
}
#[tokio::test]
async fn actual_image_bytes_and_coordinates_are_written_together() {
    let f = Fixture::new();
    f.registry();
    let plan = prepare(&f.policy, "versions/pack", &quest(entry("first")))
        .await
        .unwrap();
    apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash).unwrap();
    let image = image::DynamicImage::new_rgba8(2, 2);
    let mut png = Cursor::new(vec![]);
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let change = Change::QuestImage {
        chapter: "start".into(),
        filename: "guide.png".into(),
        data_base64: STANDARD.encode(png.get_ref()),
        position: ftb::Position { x: 0.0, y: -1.56 },
        width: 2.0,
        height: 1.0,
    };
    let plan = prepare(&f.policy, "versions/pack", &change).await.unwrap();
    assert_eq!(plan.details["position"]["y"], -1.6);
    apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash).unwrap();
    assert_eq!(
        std::fs::read(f.base.join("config/ftbquests/quests/images/guide.png")).unwrap(),
        *png.get_ref()
    );
    assert!(
        ftb::read(&f.base).unwrap()["chapters"][0]["images"][0]["image"]
            .as_str()
            .unwrap()
            .starts_with("file:")
    );
    assert!(ftb::image(
        &f.base,
        "start",
        "bad.png",
        b"not a png".to_vec(),
        Default::default(),
        1.0,
        1.0
    )
    .is_err());
}
#[test]
fn snbt_preserves_types_unknown_fields_and_rejects_bad_inputs() {
    let node = snbt::parse(
        "{id: \"1234567890ABCDEF\" count: 16L list: [1b,2b] nested: {a: true} /* comment */}",
    )
    .unwrap();
    assert_eq!(snbt::parse(&node.render()).unwrap(), node);
    for bad in ["{a:1,a:2}", "{broken", "{} trailing", "{a:[B;\"bad\"]}"] {
        assert!(snbt::parse(bad).is_err(), "{bad}")
    }
    assert!(serde_json::from_value::<ftb::QuestEntry>(
        json!({"id":"bad","title":"x","completion":[{"type":"command","command":"op"}]})
    )
    .is_err());
}

#[tokio::test]
async fn staging_tampering_and_ftb_group_id_collision_are_rejected() {
    let f = Fixture::new();
    let plan = prepare(&f.policy, "versions/pack", &config(true))
        .await
        .unwrap();
    std::fs::write(
        f.base
            .join(format!(".launcher/mcp/plans/{}/0.bin", plan.plan_id)),
        "tamper",
    )
    .unwrap();
    assert_eq!(
        apply(&f.policy, "versions/pack", &plan.plan_id, &plan.plan_hash)
            .unwrap_err()
            .code,
        "PLAN_HASH_MISMATCH"
    );
    assert!(!f.base.join("config/demo.json").exists());
    f.registry();
    std::fs::create_dir_all(f.base.join("config/ftbquests/quests")).unwrap();
    std::fs::write(
        f.base.join("config/ftbquests/quests/chapter_groups.snbt"),
        "{chapter_groups:[{id:\"1234567890ABCDEF\",title:\"group\"}]}",
    )
    .unwrap();
    assert!(prepare(
        &f.policy,
        "versions/pack",
        &quest(entry("1234567890ABCDEF"))
    )
    .await
    .unwrap_err()
    .message
    .contains("重复"));
}
#[test]
fn plain_description_and_loader_metadata_are_parsed() {
    let entry:ftb::QuestEntry=serde_json::from_value(json!({"id":"plain","title":"Title","description":"First line\nSecond line","completion":[{"type":"checkmark"}]})).unwrap();
    assert_eq!(entry.description, vec!["First line", "Second line"]);
    for (path, content, id) in [
        (
            "META-INF/mods.toml",
            "[[mods]]\nmodId=\"demo_forge\"\nversion=\"1.0\"",
            "demo_forge",
        ),
        (
            "quilt.mod.json",
            r#"{"quilt_loader":{"id":"demo_quilt"}}"#,
            "demo_quilt",
        ),
        ("mcmod.info", r#"[{"modid":"legacy"}]"#, "legacy"),
    ] {
        let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(items::mod_namespaces(&bytes).unwrap().contains(id));
    }
    let nested = jar("embedded");
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"id":"outer","jars":[{"file":"META-INF/jars/embedded.jar"}]}"#)
        .unwrap();
    zip.start_file(
        "META-INF/jars/embedded.jar",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(&nested).unwrap();
    assert!(items::mod_namespaces(&zip.finish().unwrap().into_inner())
        .unwrap()
        .contains("embedded"));
}
