use super::*;
use serde_json::json;
use std::io::Write;
fn candidate(id: &str, version: &str, deps: Vec<Requirement>) -> Candidate {
    Candidate {
        id: id.into(),
        version: version.into(),
        source: format!("{id}-{version}.jar"),
        provides: vec![],
        provided_versions: Default::default(),
        origin: Default::default(),
        dependencies: deps,
        environment: Environment::Client,
        installed: false,
        metadata_complete: true,
    }
}
fn req(id: &str, constraint: Constraint) -> Requirement {
    Requirement {
        id: id.into(),
        constraint,
        relation: Relation::Required,
        reason: "fixture declared dependency".into(),
    }
}
fn root(id: &str) -> Root {
    Root {
        id: id.into(),
        constraint: Constraint::Any,
        reason: "explicit fixture root".into(),
    }
}
pub(super) fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    for (name, bytes) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[test]
fn fabric_predicates_and_maven_ranges_are_conservative_and_keep_opaque_versions_unknown() {
    use version::Match::*;
    for (actual, range, expected) in [
        ("1.5.0", ">=1.2 <2", Yes),
        ("2.0.0", ">=1.2 <2", No),
        ("1.21.1", "1.21.x", Yes),
        ("0.2.9", "^0.2.1", Yes),
        ("0.3.0", "^0.2.1", No),
        ("1.3.0", "~1.2.0", No),
        ("3.0.0", "<2 || >=3", Yes),
        ("opaque-build", ">=1", Unknown),
        ("opaque-build", "opaque-build", Yes),
        ("1.0.0-beta.1", ">=1.0.0", No),
        ("1.0", "=>bad", No),
    ] {
        assert_eq!(
            version::matches(actual, &Constraint::fabric(range)),
            expected,
            "{actual} {range}"
        );
    }
    for (actual, range, expected) in [
        ("1.20.1", "[1.20,1.21)", Yes),
        ("1.21", "[1.20,1.21)", No),
        ("2.0", "(,1.0],[2.0,)", Yes),
        ("1.0.0.1", "[1.0.0.1,)", Yes),
        ("1.0.Final", "[1.0]", Yes),
        ("1.0-rc1", "[1.0,)", No),
        ("1.0-SNAPSHOT", "[1.0-beta2,1.0)", Yes),
        ("1.0-custom", "[1.0,)", Unknown),
        ("1.0", "[1.0,", Unknown),
    ] {
        assert_eq!(
            version::matches(actual, &Constraint::maven(range)),
            expected,
            "{actual} {range}"
        );
    }
}

#[test]
fn backtracking_finds_a_shared_compatible_prerequisite_instead_of_the_first_version() {
    let candidates = vec![
        candidate("a", "1", vec![req("d", Constraint::Any)]),
        candidate("c", "1", vec![req("b", Constraint::fabric("<2"))]),
        candidate("d", "2", vec![req("b", Constraint::fabric(">=2"))]),
        candidate("d", "1", vec![req("b", Constraint::fabric("<2"))]),
        candidate("b", "2", vec![]),
        candidate("b", "1", vec![]),
    ];
    let report = solve(&candidates, &[root("a"), root("c")], Environment::Client);
    assert_eq!(report.status, Status::Resolved);
    assert!(report
        .selected
        .iter()
        .any(|c| c.id == "d" && c.version == "1"));
    assert!(report
        .selected
        .iter()
        .any(|c| c.id == "b" && c.version == "1"));
    assert_eq!(report.selected.iter().filter(|c| c.id == "b").count(), 1);
    assert!(report.steps >= 5);
}

#[test]
fn conflicts_cycles_aliases_optional_dependencies_and_environments_have_distinct_results() {
    let mut a = candidate(
        "a",
        "1",
        vec![
            req("alias", Constraint::fabric(">=1")),
            req("optional", Constraint::Any),
        ],
    );
    a.dependencies[1].relation = Relation::Optional;
    let mut b = candidate("provider", "1", vec![req("a", Constraint::Any)]);
    b.provides = vec!["alias".into()];
    let report = solve(&[a.clone(), b.clone()], &[root("a")], Environment::Client);
    assert_eq!(report.status, Status::Resolved);
    assert_eq!(
        report.cycles,
        vec![vec!["a".to_owned(), "provider".to_owned()]]
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| w.code == "optional_dependency"));
    a.dependencies.push(Requirement {
        id: "provider".into(),
        constraint: Constraint::Any,
        relation: Relation::Conflict,
        reason: "known incompatible".into(),
    });
    assert_eq!(
        solve(&[a.clone(), b.clone()], &[root("a")], Environment::Client).status,
        Status::Conflict
    );
    a.dependencies.pop();
    b.environment = Environment::Server;
    assert_eq!(
        solve(&[a, b], &[root("a")], Environment::Client).status,
        Status::Conflict
    );
    let unknown = candidate(
        "unknown",
        "opaque",
        vec![req("b", Constraint::fabric(">=1"))],
    );
    assert_eq!(
        solve(
            &[unknown, candidate("b", "unparseable", vec![])],
            &[root("unknown")],
            Environment::Client
        )
        .status,
        Status::Unknown
    );
    let large = vec![candidate("a", "1", vec![]); 2049];
    assert_eq!(
        solve(&large, &[root("a")], Environment::Client).status,
        Status::LimitReached
    );
}

#[test]
fn reads_fabric_nested_mods_quilt_and_multimod_forge_without_executing_archives() {
    let inner = jar(&[(
        "fabric.mod.json",
        br#"{"id":"dep","version":"1.3","provides":["alias"]}"#,
    )]);
    let outer=jar(&[("fabric.mod.json",br#"{"id":"a","version":"1.0","depends":{"alias":">=1.2"},"jars":[{"file":"META-INF/dep.jar"}]}"#),("META-INF/dep.jar",&inner)]);
    let parsed = metadata::parse_jar(&outer, "outer.jar", "fabric").unwrap();
    assert_eq!(parsed.candidates.len(), 2);
    assert_eq!(
        solve(&parsed.candidates, &parsed.roots, Environment::Client).status,
        Status::Resolved
    );
    let quilt=jar(&[("quilt.mod.json",br#"{"quilt_loader":{"id":"q","version":"1","depends":[{"id":"minecraft","versions":{"any":[">=1.20","1.19.x"]}}]}}"#)]);
    assert!(
        !metadata::parse_jar(&quilt, "q.jar", "quilt")
            .unwrap()
            .wrong_loader
    );
    let toml = br#"modLoader="javafml"
loaderVersion="[47,)"
[[mods]]
modId="first"
version="${file.jarVersion}"
[[mods]]
modId="second"
version="1.0"
[[dependencies.first]]
modId="second"
mandatory=true
versionRange="[1,)"
side="BOTH"
"#;
    let forge = jar(&[
        ("META-INF/mods.toml", toml),
        ("META-INF/MANIFEST.MF", b"Implementation-Version: 2.0\n"),
    ]);
    let parsed = metadata::parse_jar(&forge, "bundle.jar", "forge").unwrap();
    assert_eq!(parsed.roots.len(), 2);
    assert_eq!(parsed.candidates[0].version, "2.0");
    let mut candidates = parsed.candidates.clone();
    candidates.push(candidate("forge", "47.4.0", vec![]));
    let report = solve(&candidates, &parsed.roots, Environment::Client);
    assert_eq!(report.status, Status::Resolved);
    let universal = jar(&[
        ("fabric.mod.json", br#"{"id":"fabric_part","version":"1"}"#),
        ("META-INF/mods.toml", toml),
    ]);
    assert!(
        !metadata::parse_jar(&universal, "universal.jar", "forge")
            .unwrap()
            .wrong_loader
    );
    let unsafe_nested = jar(&[
        (
            "fabric.mod.json",
            br#"{"id":"bad","version":"1","jars":[{"file":"../dep.jar"}]}"#,
        ),
        ("../dep.jar", &inner),
    ]);
    assert!(metadata::parse_jar(&unsafe_nested, "bad.jar", "fabric").is_err());
}

#[tokio::test]
async fn inventory_reports_duplicates_ignores_disabled_files_and_plans_candidate_replacements_read_only(
) {
    let root = std::env::temp_dir().join(format!("memoria-dependency-{}", uuid::Uuid::new_v4()));
    let base = root.join("versions/test");
    std::fs::create_dir_all(base.join("mods")).unwrap();
    std::fs::write(base.join("test.json"),json!({"id":"test","mainClass":"net.minecraft.client.main.Main","clientVersion":"1.21.1","javaVersion":{"majorVersion":21},"libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}).to_string()).unwrap();
    let a = jar(&[(
        "fabric.mod.json",
        br#"{"id":"a","version":"1","depends":{"b":">=2","minecraft":"1.21.x"}}"#,
    )]);
    let b = jar(&[("fabric.mod.json", br#"{"id":"b","version":"1"}"#)]);
    std::fs::write(base.join("mods/a.jar"), &a).unwrap();
    std::fs::write(base.join("mods/b.jar.disabled"), &b).unwrap();
    let mut inventory = inventory::inspect_instance(&root, "versions/test")
        .await
        .unwrap();
    assert_eq!(inventory.audit().status, Status::Conflict);
    assert_eq!(inventory.files.len(), 1);
    let new = jar(&[("fabric.mod.json", br#"{"id":"b","version":"2"}"#)]);
    inventory.add_jar(&new, "candidate-b.jar", false);
    assert_eq!(inventory.audit().status, Status::Resolved);
    let mut invalid = inventory.clone();
    invalid.add_jar(
        &jar(&[("fabric.mod.json", br#"{"id":"invalid id","version":"1"}"#)]),
        "invalid.jar",
        true,
    );
    assert_eq!(invalid.audit().status, Status::Unknown);
    let mut empty = inventory.clone();
    empty.add_jar(
        &jar(&[("META-INF/mods.toml", b"modLoader=\"javafml\"\n")]),
        "empty.jar",
        true,
    );
    assert!(empty
        .warnings
        .iter()
        .any(|issue| issue.code == "empty_mod_metadata"));
    assert_eq!(std::fs::read_dir(base.join("mods")).unwrap().count(), 2);
    std::fs::write(base.join("mods/a-copy.jar"), &a).unwrap();
    let inventory = inventory::inspect_instance(&root, "versions/test")
        .await
        .unwrap();
    assert!(inventory
        .audit()
        .issues
        .iter()
        .any(|i| i.code == "duplicate_mod"));
    std::fs::remove_dir_all(root).unwrap();
}

#[path = "remote_tests.rs"]
mod remote_tests;

#[tokio::test]
async fn candidate_file_replacement_keeps_multimod_files_whole_and_rejects_duplicate_choices() {
    let root = std::env::temp_dir().join(format!("memoria-bundle-{}", uuid::Uuid::new_v4()));
    let base = root.join("versions/test");
    std::fs::create_dir_all(base.join("mods")).unwrap();
    std::fs::write(base.join("test.json"), json!({"id":"test","mainClass":"net.minecraft.client.main.Main","clientVersion":"1.20.1","libraries":[{"name":"net.minecraftforge:forge:1.20.1-47.4.0"}]}).to_string()).unwrap();
    let old = jar(&[("META-INF/mods.toml", b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"a\"\nversion=\"1\"\n[[mods]]\nmodId=\"b\"\nversion=\"1\"\n")]);
    let new = jar(&[(
        "META-INF/mods.toml",
        b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"a\"\nversion=\"2\"\n",
    )]);
    std::fs::write(base.join("mods/old.jar"), &old).unwrap();
    let path = root.join("new.jar");
    std::fs::write(&path, &new).unwrap();
    let report = inventory::solve_candidate_files(&root, "versions/test", vec![path.clone()])
        .await
        .unwrap();
    assert_eq!(
        report.status,
        Status::Conflict,
        "removing the old file also removes b"
    );
    assert!(report.issues.iter().any(|issue| issue.subject == "b"));
    let report = inventory::solve_candidate_files(&root, "versions/test", vec![path.clone(), path])
        .await
        .unwrap();
    assert_eq!(report.status, Status::Conflict);
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.code == "duplicate_candidate"));
    assert_eq!(std::fs::read(base.join("mods/old.jar")).unwrap(), old);
    assert_eq!(std::fs::read_dir(base.join("mods")).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn provided_capabilities_keep_their_own_versions_and_multimod_bundles_are_indivisible() {
    let mut provider = candidate("provider", "1", vec![]);
    provider.provides = vec!["alias".into()];
    provider
        .provided_versions
        .insert("alias".into(), "2.0".into());
    let report = solve(
        &[
            candidate("main", "1", vec![req("alias", Constraint::fabric(">=2"))]),
            provider,
        ],
        &[root("main")],
        Environment::Client,
    );
    assert_eq!(report.status, Status::Resolved);
    let mut old_a = candidate("a", "1", vec![]);
    old_a.source = "old.jar".into();
    let mut old_b = candidate("b", "1", vec![]);
    old_b.source = "old.jar".into();
    let mut new_a = candidate("a", "2", vec![]);
    new_a.source = "new.jar".into();
    let mut new_b = candidate("b", "2", vec![]);
    new_b.source = "new.jar".into();
    let candidates = [old_a, old_b, new_a, new_b];
    let fixed_a = Root {
        id: "a".into(),
        constraint: Constraint::Exact("2".into()),
        reason: "explicit new version".into(),
    };
    assert_eq!(
        solve(
            &candidates,
            &[
                fixed_a.clone(),
                Root {
                    id: "b".into(),
                    constraint: Constraint::Exact("1".into()),
                    reason: "preserve old b".into()
                }
            ],
            Environment::Client
        )
        .status,
        Status::Conflict
    );
    let report = solve(&candidates, &[fixed_a], Environment::Client);
    assert_eq!(report.status, Status::Resolved);
    assert!(report.selected.iter().all(|c| c.source == "new.jar"));
    assert_eq!(report.selected.len(), 2);
}
