use mc_core::service::mcp_policy::McpPolicy;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};

async fn request(
    input: &mut ChildStdin,
    output: &mut Lines<BufReader<ChildStdout>>,
    value: Value,
) -> Value {
    input
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
    input.flush().await.unwrap();
    let line = tokio::time::timeout(std::time::Duration::from_secs(5), output.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn running_server_reloads_saved_permissions_and_never_falls_back_from_a_broken_policy() {
    let directory = std::env::temp_dir().join(format!(
        "memoria-mcp-reload-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let game_root = directory.join("games");
    for id in ["one", "two"] {
        std::fs::create_dir_all(game_root.join(format!("versions/{id}/logs"))).unwrap();
        std::fs::write(
            game_root.join(format!("versions/{id}/{id}.json")),
            json!({"id":id,"mainClass":"net.minecraft.client.main.Main"}).to_string(),
        )
        .unwrap();
        std::fs::write(
            game_root.join(format!("versions/{id}/logs/latest.log")),
            format!("{id} private log"),
        )
        .unwrap();
    }
    let path = directory.join("mcp.json");
    let mut policy = McpPolicy {
        enabled: true,
        game_root,
        allowed_instances: vec!["versions/one".into()],
        ..Default::default()
    };
    policy.permissions.instances = true;
    policy.permissions.logs = true;
    policy.save(&path).await.unwrap();
    let mut process = Command::new(env!("CARGO_BIN_EXE_mc-mcp"))
        .env("MEMORIA_DATA_DIR", &directory)
        .arg("--game-root=ignored-by-saved-policy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = process.stdin.take().unwrap();
    let mut output = BufReader::new(process.stdout.take().unwrap()).lines();
    let listed = request(&mut input, &mut output, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"minecraft_list_instances"}})).await;
    let instances = listed["result"]["structuredContent"]["instances"]
        .as_array()
        .unwrap();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0]["id"], "versions/one");
    let denied = request(&mut input, &mut output, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"minecraft_read_instance_log","arguments":{"instance_id":"versions/two","path":"logs/latest.log"}}})).await;
    assert_eq!(
        denied["result"]["structuredContent"]["code"],
        "PERMISSION_DENIED"
    );
    assert!(!denied.to_string().contains("two private log"));
    let analysis=request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"minecraft_analyze_logs","arguments":{"instance_id":"versions/one","path":"logs/latest.log"}}})).await;
    assert_eq!(analysis["result"]["structuredContent"]["analysis"]["findings"][0]["category"],"unknown");
    policy.permissions.logs = false;
    policy.save(&path).await.unwrap();
    let revoked = request(&mut input, &mut output, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"minecraft_read_instance_log","arguments":{"instance_id":"versions/one","path":"logs/latest.log"}}})).await;
    assert_eq!(
        revoked["result"]["structuredContent"]["code"],
        "PERMISSION_DENIED"
    );
    let analysis=request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"minecraft_analyze_logs","arguments":{"instance_id":"versions/one","path":"logs/latest.log"}}})).await;
    assert_eq!(analysis["result"]["structuredContent"]["code"],"PERMISSION_DENIED");
    policy.enabled = false;
    policy.save(&path).await.unwrap();
    let disabled = request(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}),
    )
    .await;
    assert!(disabled["result"]["tools"].as_array().unwrap().is_empty());
    std::fs::write(&path, "invalid json").unwrap();
    let corrupt = request(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","id":5,"method":"tools/list"}),
    )
    .await;
    assert_eq!(corrupt["error"]["code"], -32001);
    std::fs::remove_file(&path).unwrap();
    let missing = request(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","id":6,"method":"tools/list"}),
    )
    .await;
    assert_eq!(missing["error"]["code"], -32001);
    process.kill().await.unwrap();
    let _ = process.wait().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn stdio_prepares_applies_and_revokes_write_plans_without_scope_bypass() {
    let directory = std::env::temp_dir().join(format!(
        "memoria-write-stdio-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = directory.join("games");
    let base = root.join("versions/pack");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("pack.json"), "{}").unwrap();
    let mut policy = McpPolicy {
        enabled: true,
        game_root: root,
        allowed_instances: vec!["versions/pack".into()],
        ..Default::default()
    };
    policy.permissions.write_configs = true;
    let path = directory.join("mcp.json");
    policy.save(&path).await.unwrap();
    let mut process = Command::new(env!("CARGO_BIN_EXE_mc-mcp"))
        .env("MEMORIA_DATA_DIR", &directory)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = process.stdin.take().unwrap();
    let mut output = BufReader::new(process.stdout.take().unwrap()).lines();
    let list = request(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "minecraft_write_config"));
    assert!(!tools
        .iter()
        .any(|t| t["name"] == "minecraft_write_mod_file"));
    let prepare = |id, content: &str| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"minecraft_write_config","arguments":{"instance_id":"versions/pack","path":"config/stdio.json","content":content}}});
    let prepared = request(&mut input, &mut output, prepare(2, "{\"enabled\":true}")).await;
    let plan = prepared["result"]["structuredContent"].clone();
    assert!(plan["plan_id"].is_string());
    assert!(!base.join("config/stdio.json").exists());
    let apply = |id, plan: &Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"minecraft_apply_instance_write","arguments":{"instance_id":"versions/pack","plan_id":plan["plan_id"],"plan_hash":plan["plan_hash"]}}});
    let applied = request(&mut input, &mut output, apply(3, &plan)).await;
    assert_eq!(applied["result"]["structuredContent"]["status"], "applied");
    assert_eq!(
        std::fs::read_to_string(base.join("config/stdio.json")).unwrap(),
        "{\"enabled\":true}"
    );
    let prepared = request(&mut input, &mut output, prepare(4, "{\"enabled\":false}")).await;
    let plan = prepared["result"]["structuredContent"].clone();
    policy.permissions.write_configs = false;
    policy.permissions.write_mod_files = true;
    policy.save(&path).await.unwrap();
    let denied = request(&mut input, &mut output, apply(5, &plan)).await;
    assert_eq!(
        denied["result"]["structuredContent"]["code"],
        "PERMISSION_DENIED"
    );
    let denied=request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"minecraft_write_mod_file","arguments":{"instance_id":"versions/other","filename":"a.jar","data_base64":"a","sha256":"bad"}}})).await;
    assert_eq!(
        denied["result"]["structuredContent"]["code"],
        "PERMISSION_DENIED"
    );
    // Larger than the old 1 MiB frame bound: report an argument error, then continue serving.
    let big = "x".repeat(1024 * 1024 + 100);
    let response=request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"minecraft_write_mod_file","arguments":{"instance_id":"versions/pack","filename":"a.jar","data_base64":big,"sha256":"bad"}}})).await;
    assert_eq!(response["result"]["isError"], true);
    let ping = request(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","id":8,"method":"ping"}),
    )
    .await;
    assert!(ping["result"].is_object());
    process.kill().await.unwrap();
    let _ = process.wait().await;
    std::fs::remove_dir_all(directory).unwrap();
}
