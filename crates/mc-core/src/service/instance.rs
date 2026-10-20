use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{LauncherError, LauncherResult};
use crate::model::{CreateInstanceRequest, Instance};

/// 实例管理服务接口
#[async_trait]
pub trait InstanceService: Send + Sync {
    /// 获取当前所有实例列表
    async fn list_instances(&self) -> LauncherResult<Vec<Instance>>;

    /// 根据 ID 查询单个实例
    async fn get_instance(&self, id: &str) -> LauncherResult<Instance>;

    /// 创建一个新实例
    async fn create_instance(&self, req: CreateInstanceRequest) -> LauncherResult<Instance>;
}

/// 阶段 0 内存实现，用于契约验证与脱机调试
#[derive(Default, Clone)]
pub struct MockInstanceService {
    instances: Arc<RwLock<HashMap<String, Instance>>>,
}

impl MockInstanceService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入初始测试数据
    pub async fn with_sample_data(self) -> Self {
        let sample = Instance {
            id: "inst-default".to_string(),
            name: "默认原版 1.21.1".to_string(),
            mc_version: "1.21.1".to_string(),
            loader: None,
            game_dir: PathBuf::from("instances/inst-default"),
            created_at: Utc::now(),
            last_played_at: None,
        };
        self.instances.write().await.insert(sample.id.clone(), sample);
        self
    }
}

#[async_trait]
impl InstanceService for MockInstanceService {
    async fn list_instances(&self) -> LauncherResult<Vec<Instance>> {
        let map = self.instances.read().await;
        Ok(map.values().cloned().collect())
    }

    async fn get_instance(&self, id: &str) -> LauncherResult<Instance> {
        let map = self.instances.read().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| LauncherError::InstanceNotFound(id.to_string()))
    }

    async fn create_instance(&self, req: CreateInstanceRequest) -> LauncherResult<Instance> {
        let id = format!("inst-{}", uuid::Uuid::new_v4().simple());
        let game_dir = PathBuf::from(format!("instances/{}", id));
        let instance = Instance {
            id: id.clone(),
            name: req.name,
            mc_version: req.mc_version,
            loader: req.loader,
            game_dir,
            created_at: Utc::now(),
            last_played_at: None,
        };
        self.instances.write().await.insert(id, instance.clone());
        Ok(instance)
    }
}
