use super::*;

fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("memoria-general-{}", uuid::Uuid::new_v4()))
}

#[test]
fn settings_round_trip_export_validation_and_failed_replacement_preserve_saved_data() {
    let root = directory();
    assert_eq!(load(&root).unwrap(), Config::default());
    let config = Config {
        minimize_on_launch: true,
        check_updates: true,
        date_format: 2,
        ..Default::default()
    };
    let path = root.join("general.json");
    write(&path, &config).unwrap();
    write(&path, &config).unwrap();
    assert_eq!(load(&root).unwrap(), config);
    assert_eq!(load(&root).unwrap().date_pattern(), "%m/%d/%Y %H:%M");
    let export = root.join("export.json");
    write(&export, &config).unwrap();
    assert_eq!(read(&export).unwrap(), config);
    let previous = fs::read(&path).unwrap();
    let invalid = Config {
        date_format: 3,
        ..config.clone()
    };
    assert!(write(&path, &invalid).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    let bad_target = root.join("directory.json");
    fs::create_dir(&bad_target).unwrap();
    assert!(write(&bad_target, &config).is_err());
    assert!(fs::read_dir(&root).unwrap().all(|e| e
        .unwrap()
        .path()
        .extension()
        .is_none_or(|ext| ext != "tmp")));
    assert_eq!(fs::read(&path).unwrap(), previous);
    for bad in [
        b"corrupted".as_slice(),
        br#"{"format":"memoria-general-settings","version":2,"general":{}}"#,
        br#"{"format":"another-app","version":1,"general":{}}"#,
        br#"{"format":"memoria-general-settings","version":1,"general":{"date_format":999}}"#,
        br#"{"format":"memoria-general-settings","version":1,"general":{"api_key":"secret"}}"#,
    ] {
        fs::write(&export, bad).unwrap();
        assert!(read(&export).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
    }
    fs::File::create(&export)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    assert!(read(&export).is_err());
    fs::write(&path, b"corrupt").unwrap();
    assert!(load(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cache_cleanup_is_recoverable_and_preserves_configuration_games_and_logs() {
    let root = directory();
    assert!(recover_cache(&root).unwrap().contains("没有"));
    fs::create_dir_all(root.join("cache/nested")).unwrap();
    fs::write(root.join("cache/nested/file"), b"cached content").unwrap();
    for name in [
        "accounts.json",
        "environment.json",
        "appearance.json",
        "game.jar",
        "game.log",
    ] {
        fs::write(root.join(name), b"keep").unwrap();
    }
    assert!(recover_cache(&root).unwrap().contains("手动恢复"));
    assert!(!root.join("cache").exists());
    let backup = fs::read_dir(root.join("recovered-cache"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(backup.join("nested/file")).unwrap(),
        b"cached content"
    );
    for name in [
        "accounts.json",
        "environment.json",
        "appearance.json",
        "game.jar",
        "game.log",
    ] {
        assert_eq!(fs::read(root.join(name)).unwrap(), b"keep");
    }
    fs::write(root.join("cache"), b"not a directory").unwrap();
    assert!(recover_cache(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn cache_cleanup_rejects_links_and_does_not_follow_nested_links() {
    use std::os::unix::fs::symlink;
    let root = directory();
    let outside = directory();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep"), b"outside").unwrap();
    symlink(&outside, root.join("cache")).unwrap();
    assert!(recover_cache(&root).is_err());
    fs::remove_file(root.join("cache")).unwrap();
    fs::create_dir(root.join("cache")).unwrap();
    symlink(&outside, root.join("recovered-cache")).unwrap();
    assert!(recover_cache(&root).is_err());
    fs::remove_file(root.join("recovered-cache")).unwrap();
    symlink(&outside, root.join("cache/link")).unwrap();
    recover_cache(&root).unwrap();
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"outside");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
