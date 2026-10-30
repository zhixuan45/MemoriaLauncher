//! Opt-in verification against an existing installation. No account credentials are read.
use mc_core::{GameLauncher, GameLayout, LaunchOptions, LocalClientService, OfflineAuthProvider};
use std::{path::PathBuf, sync::Arc};

#[test]
#[ignore="Requires MEMORIA_TEST_GAME_ROOT and MEMORIA_TEST_INSTANCE"]
fn real_instance_settings_load_with_all_local_mods() {
    let root=PathBuf::from(std::env::var_os("MEMORIA_TEST_GAME_ROOT").unwrap());
    let key=std::env::var("MEMORIA_TEST_INSTANCE").unwrap();
    let snapshot=mc_core::service::desktop_instances::snapshot(&root,&key).unwrap();
    eprintln!("Loaded settings and {} mods; largest {} bytes",snapshot.mods.len(),snapshot.mods.iter().map(|m|m.size).max().unwrap_or(0));
    assert!(!snapshot.mods.is_empty());
}

#[tokio::test]
#[ignore = "Requires MEMORIA_TEST_GAME_ROOT, MEMORIA_TEST_INSTANCE and MEMORIA_TEST_JAVA"]
async fn real_profile_passes_full_startup_checks_and_builds_resolved_command() {
    let root = PathBuf::from(std::env::var_os("MEMORIA_TEST_GAME_ROOT").unwrap());
    let key = std::env::var("MEMORIA_TEST_INSTANCE").unwrap();
    let java = PathBuf::from(std::env::var_os("MEMORIA_TEST_JAVA").unwrap());
    let layout = GameLayout::new(root.canonicalize().unwrap()).unwrap();
    let service = LocalClientService::new(layout.clone());
    let report = service.scan().await.unwrap();
    let client = report
        .clients
        .iter()
        .find(|client| client.instance.id == key)
        .unwrap();
    eprintln!(
        "Scanned {}: MC {}, loader {:?}, required Java {:?}",
        key, client.instance.mc_version, client.instance.loader, client.java_major
    );
    let launcher = GameLauncher::new(layout, Arc::new(OfflineAuthProvider));
    if std::env::var_os("MEMORIA_TEST_REPAIR_LOGGING").is_some() {
        launcher
            .repair_logging(
                &key,
                &mc_core::DownloadClient::new(Default::default()).unwrap(),
            )
            .await
            .unwrap();
    }
    let options = LaunchOptions {
        java,
        ..Default::default()
    };
    let check = launcher.check(&key, options.clone()).await.unwrap();
    eprintln!("Startup check: {check:?}");
    let plan = launcher
        .prepare(&key, "LaunchProbe", options)
        .await
        .unwrap();
    let arguments = plan.redacted_arguments();
    assert!(arguments.iter().all(|argument| !argument.contains("${")));
    assert!(arguments.iter().any(|argument| argument
        .starts_with("-Dlog4j.configurationFile=") && !argument.contains("[REDACTED]")));
    assert_eq!(
        plan.game_directory().canonicalize().unwrap(),
        client.instance.game_dir.canonicalize().unwrap()
    );
    #[cfg(windows)]
    assert!(arguments
        .iter()
        .find(|argument| argument.starts_with("-Dlog4j.configurationFile="))
        .is_none_or(|argument| !argument.contains(r"\\?\")));
    eprintln!(
        "Resolved {} arguments, logging={:?}",
        arguments.len(),
        arguments
            .iter()
            .find(|argument| argument.starts_with("-Dlog4j.configurationFile="))
    );
}
