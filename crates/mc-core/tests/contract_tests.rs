use mc_core::{
    CatalogProvider, CreateInstanceRequest, GameVersionType, InMemoryTaskService, InstanceService,
    MockCatalogProvider, MockInstanceService, TaskService, TaskStatus, TaskType,
};

#[tokio::test]
async fn test_instance_service_contract() {
    let service = MockInstanceService::new().with_sample_data().await;

    // 验证初始样例实例
    let list = service.list_instances().await.expect("获取实例列表失败");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "inst-default");

    // 验证新建实例
    let new_inst = service
        .create_instance(CreateInstanceRequest {
            name: "测试纯净版".to_string(),
            mc_version: "1.20.1".to_string(),
            loader: None,
        })
        .await
        .expect("创建新实例失败");

    assert_eq!(new_inst.name, "测试纯净版");
    assert_eq!(new_inst.mc_version, "1.20.1");

    // 再次获取应有 2 个实例
    let updated_list = service.list_instances().await.expect("获取更新列表失败");
    assert_eq!(updated_list.len(), 2);
}

#[tokio::test]
async fn test_catalog_provider_contract() {
    let provider = MockCatalogProvider::new();
    let versions = provider.list_game_versions().await.expect("获取版本失败");
    assert!(!versions.is_empty());
    assert_eq!(versions[0].version_type, GameVersionType::Release);

    let mods = provider
        .search_mods("fabric", None)
        .await
        .expect("检索模组失败");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].id, "fabric-api");
}

#[tokio::test]
async fn test_task_service_contract() {
    let service = InMemoryTaskService::new();
    let task = service.register_task(TaskType::DownloadVersion).await;

    assert_eq!(task.status, TaskStatus::Pending);

    // 取消任务
    service.cancel_task(&task.id).await.expect("取消任务失败");
    let updated = service.get_task(&task.id).await.expect("查询任务失败");
    assert_eq!(updated.status, TaskStatus::Cancelled);
}
