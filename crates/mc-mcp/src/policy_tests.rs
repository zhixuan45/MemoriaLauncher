use super::*;

#[tokio::test]
async fn permissions_filter_tool_discovery_and_deny_ungranted_tools_and_instances() {
    let catalog = OnlineCatalog::new().unwrap();
    let directory = std::env::temp_dir().join(format!("memoria-mcp-policy-{}", std::process::id()));
    for id in ["allowed", "denied"] {
        std::fs::create_dir_all(directory.join(format!("versions/{id}/logs"))).unwrap();
        std::fs::write(
            directory.join(format!("versions/{id}/{id}.json")),
            json!({"id":id,"type":"release","mainClass":"net.minecraft.client.main.Main"})
                .to_string(),
        )
        .unwrap();
        std::fs::write(
            directory.join(format!("versions/{id}/logs/latest.log")),
            format!("{id} log"),
        )
        .unwrap();
    }
    let mut policy = McpPolicy {
        enabled: true,
        game_root: directory.clone(),
        allowed_instances: vec!["versions/allowed".into()],
        ..Default::default()
    };
    policy.permissions.catalog = false;
    policy.permissions.instances = true;
    policy.permissions.logs = true;
    let list = dispatch_with_policy(
        json!({"jsonrpc":"2.0", "id":1, "method":"tools/list"}),
        &catalog,
        Path::new("ignored"),
        Some(&policy),
    )
    .await
    .unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 6);
    for name in ["minecraft_list_versions", "run_shell"] {
        let denied = dispatch_with_policy(
            json!({"jsonrpc":"2.0", "id":2,"method":"tools/call", "params":{"name":name}}),
            &catalog,
            Path::new("ignored"),
            Some(&policy),
        )
        .await
        .unwrap();
        assert_eq!(
            denied["result"]["structuredContent"]["code"],
            "PERMISSION_DENIED"
        );
    }
    let instances = dispatch_with_policy(json!({"jsonrpc":"2.0", "id":3,"method":"tools/call", "params":{"name":"minecraft_list_instances"}}), &catalog, Path::new("ignored"), Some(&policy)).await.unwrap();
    let instances = instances["result"]["structuredContent"]["instances"]
        .as_array()
        .unwrap();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0]["id"], "versions/allowed");
    for id in ["versions/denied", "versions/../denied"] {
        let denied = dispatch_with_policy(json!({"jsonrpc":"2.0", "id":4,"method":"tools/call", "params":{"name":"minecraft_read_instance_log", "arguments":{"instance_id":id,"path":"logs/latest.log"}}}), &catalog, &directory, Some(&policy)).await.unwrap();
        assert_eq!(
            denied["result"]["structuredContent"]["code"],
            "PERMISSION_DENIED"
        );
    }
    let read = dispatch_with_policy(json!({"jsonrpc":"2.0", "id":5,"method":"tools/call", "params":{"name":"minecraft_read_instance_log", "arguments":{"instance_id":"versions/allowed","path":"logs/latest.log"}}}), &catalog, Path::new("ignored"), Some(&policy)).await.unwrap();
    assert_eq!(
        read["result"]["structuredContent"]["log"]["text"],
        "allowed log"
    );
    let analyzed=dispatch_with_policy(json!({"jsonrpc":"2.0","id":51,"method":"tools/call","params":{"name":"minecraft_analyze_logs","arguments":{"instance_id":"versions/allowed","path":"logs/latest.log"}}}),&catalog,Path::new("ignored"),Some(&policy)).await.unwrap();
    assert_eq!(analyzed["result"]["structuredContent"]["analysis"]["findings"][0]["category"],"unknown");
    for (enabled,id) in [(true,"versions/denied"),(false,"versions/allowed")] {
        policy.permissions.logs=enabled;
        let analyzed=dispatch_with_policy(json!({"jsonrpc":"2.0","id":52,"method":"tools/call","params":{"name":"minecraft_analyze_logs","arguments":{"instance_id":id,"path":"logs/latest.log"}}}),&catalog,Path::new("ignored"),Some(&policy)).await.unwrap();
        assert_eq!(analyzed["result"]["structuredContent"]["code"],"PERMISSION_DENIED");
    }
    policy.permissions.logs=true;
    let dependencies=dispatch_with_policy(json!({"jsonrpc":"2.0","id":61,"method":"tools/call","params":{"name":"minecraft_resolve_dependencies","arguments":{"instance_id":"versions/allowed"}}}),&catalog,Path::new("ignored"),Some(&policy)).await.unwrap();
    assert_eq!(dependencies["result"]["structuredContent"]["read_only"],true);
    assert_eq!(dependencies["result"]["structuredContent"]["local_report"]["status"],"resolved");
    for args in [json!({"instance_id":"versions/denied"}),json!({"instance_id":"versions/allowed","roots":[{"project":"project","version":"version"}]})] {
        let denied=dispatch_with_policy(json!({"jsonrpc":"2.0","id":62,"method":"tools/call","params":{"name":"minecraft_resolve_dependencies","arguments":args}}),&catalog,Path::new("ignored"),Some(&policy)).await.unwrap();assert_eq!(denied["result"]["structuredContent"]["code"],"PERMISSION_DENIED");
    }
    policy.enabled = false;
    let disabled = dispatch_with_policy(
        json!({"jsonrpc":"2.0", "id":6,"method":"tools/list"}),
        &catalog,
        &directory,
        Some(&policy),
    )
    .await
    .unwrap();
    assert!(disabled["result"]["tools"].as_array().unwrap().is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}
