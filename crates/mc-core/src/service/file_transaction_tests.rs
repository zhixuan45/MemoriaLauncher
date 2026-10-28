use super::*;
struct Fixture {
    root: PathBuf,
    base: PathBuf,
    id: String,
    ops: Vec<Operation>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("memoria-txn-{}", uuid::Uuid::new_v4()));
        let base = root.join("versions/test");
        fs::create_dir_all(base.join("config")).unwrap();
        fs::create_dir_all(base.join("stage")).unwrap();
        fs::write(base.join("test.json"), b"{}").unwrap();
        fs::write(base.join("config/old.json"), b"old").unwrap();
        fs::write(base.join("config/delete.json"), b"delete").unwrap();
        fs::write(base.join("stage/0"), b"new").unwrap();
        fs::write(base.join("stage/1"), b"added").unwrap();
        let ops = vec![
            Operation {
                path: "config/old.json".into(),
                old_sha256: Some(digest(b"old")),
                staged: Some("stage/0".into()),
            },
            Operation {
                path: "config/added.json".into(),
                old_sha256: None,
                staged: Some("stage/1".into()),
            },
            Operation {
                path: "config/delete.json".into(),
                old_sha256: Some(digest(b"delete")),
                staged: None,
            },
        ];
        Self {
            root,
            base,
            id: uuid::Uuid::new_v4().to_string(),
            ops,
        }
    }
    fn interrupt(&self, step: usize) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "service::file_transaction::tests::crash_worker",
                "--ignored",
            ])
            .env("MEMORIA_TXN_CRASH_BASE", &self.base)
            .env("MEMORIA_TXN_CRASH_ID", &self.id)
            .env("MEMORIA_TXN_CRASH_STEP", step.to_string())
            .output()
            .unwrap();
        assert_eq!(
            status.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&status.stdout)
        );
    }
    fn assert_old(&self) {
        assert_eq!(fs::read(self.base.join("config/old.json")).unwrap(), b"old");
        assert_eq!(
            fs::read(self.base.join("config/delete.json")).unwrap(),
            b"delete"
        );
        assert!(!self.base.join("config/added.json").exists());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
pub(super) fn crash_point(base: &Path, step: usize) {
    if std::env::var_os("MEMORIA_TXN_CRASH_BASE").is_some_and(|p| PathBuf::from(p) == base)
        && std::env::var("MEMORIA_TXN_CRASH_STEP").ok().as_deref()
            == Some(step.to_string().as_str())
    {
        std::process::exit(91);
    }
}
#[test]
#[ignore = "subprocess worker; exits without destructors during commit"]
fn crash_worker() {
    let base = PathBuf::from(std::env::var_os("MEMORIA_TXN_CRASH_BASE").unwrap());
    let id = std::env::var("MEMORIA_TXN_CRASH_ID").unwrap();
    let ops = vec![
        Operation {
            path: "config/old.json".into(),
            old_sha256: Some(digest(b"old")),
            staged: Some("stage/0".into()),
        },
        Operation {
            path: "config/added.json".into(),
            old_sha256: None,
            staged: Some("stage/1".into()),
        },
        Operation {
            path: "config/delete.json".into(),
            old_sha256: Some(digest(b"delete")),
            staged: None,
        },
    ];
    let _lock = files::lock(&base).unwrap();
    execute_locked(&base, &id, "subprocess_fixture", &ops).unwrap();
    panic!("crash point was not reached");
}
#[test]
fn restart_rolls_back_each_real_process_interruption_and_is_idempotent() {
    for step in 0..3 {
        let f = Fixture::new();
        f.interrupt(step);
        let report = recover(&f.root, "versions/test").unwrap();
        assert_eq!(report.rolled_back, vec![f.id.clone()]);
        f.assert_old();
        assert!(recover(&f.root, "versions/test")
            .unwrap()
            .rolled_back
            .is_empty());
        assert_eq!(
            history(&f.root, "versions/test").unwrap()[0].state,
            "rolled_back"
        );
    }
}
#[test]
fn restart_after_commit_marker_preserves_success_even_when_the_caller_has_no_receipt() {
    let f = Fixture::new();
    f.interrupt(3);
    assert!(recover(&f.root, "versions/test")
        .unwrap()
        .rolled_back
        .is_empty());
    assert_eq!(fs::read(f.base.join("config/old.json")).unwrap(), b"new");
    assert_eq!(
        history(&f.root, "versions/test").unwrap()[0].state,
        "committed"
    );
}
#[test]
fn successful_commit_keeps_backups_and_ordinary_errors_roll_back() {
    let f = Fixture::new();
    let _lock = files::lock(&f.base).unwrap();
    execute_locked(&f.base, &f.id, "fixture", &f.ops).unwrap();
    assert_eq!(fs::read(f.base.join("config/old.json")).unwrap(), b"new");
    assert!(!f.base.join("config/delete.json").exists());
    assert!(recover_locked(&f.base).unwrap().rolled_back.is_empty());
    assert_eq!(
        fs::read(f.base.join(format!("{}/{}/before/0", DIRECTORY, f.id))).unwrap(),
        b"old"
    );
    let g = Fixture::new();
    assert_eq!(
        execute_with(&g.base, &g.id, "fixture", &g.ops, Some(1))
            .unwrap_err()
            .code,
        "APPLY_FAILED"
    );
    g.assert_old();
}
#[test]
fn external_edits_corrupt_backups_and_untrusted_paths_block_recovery_without_partial_rollback() {
    let f = Fixture::new();
    f.interrupt(1);
    fs::write(f.base.join("config/old.json"), b"external").unwrap();
    assert!(recover(&f.root, "versions/test").is_err());
    assert!(files::lock(&f.base).is_err());
    assert_eq!(
        fs::read(f.base.join("config/old.json")).unwrap(),
        b"external"
    );
    assert!(
        f.base.join("config/added.json").exists(),
        "preflight failure must leave the entire transaction alone"
    );
    fs::write(f.base.join("config/old.json"), b"new").unwrap();
    let backup = f.base.join(format!("{}/{}/before/0", DIRECTORY, f.id));
    fs::write(&backup, b"corrupt").unwrap();
    assert!(recover(&f.root, "versions/test").is_err());
    assert_eq!(fs::read(f.base.join("config/old.json")).unwrap(), b"new");
    fs::write(backup, b"old").unwrap();
    let path = f.base.join(format!("{}/{}/journal.json", DIRECTORY, f.id));
    let original = fs::read(&path).unwrap();
    let mut journal: serde_json::Value = serde_json::from_slice(&original).unwrap();
    journal["entries"][0]["path"] = serde_json::json!("../escape");
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert!(recover(&f.root, "versions/test").is_err());
    fs::write(path, original).unwrap();
    recover(&f.root, "versions/test").unwrap();
    f.assert_old();
}
#[test]
fn dangerous_targets_and_stale_files_never_enter_commit() {
    let f = Fixture::new();
    let mut ops = f.ops.clone();
    ops[0].path = "test.json".into();
    assert!(execute_locked(&f.base, &f.id, "fixture", &ops).is_err());
    let g = Fixture::new();
    fs::write(g.base.join("config/old.json"), b"external").unwrap();
    assert_eq!(
        execute_locked(&g.base, &g.id, "fixture", &g.ops)
            .unwrap_err()
            .code,
        "PLAN_STALE"
    );
    assert!(!g.base.join("config/added.json").exists());
}
