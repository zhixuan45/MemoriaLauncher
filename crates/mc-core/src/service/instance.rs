use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{LauncherError, LauncherResult};
use crate::model::{CreateInstanceRequest, Instance, LoaderInfo, LoaderType};

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
        let now = Utc::now();
        let samples = vec![
            Instance {
                id: "inst-fabric-121".to_string(),
                name: "Fabric 1.21".to_string(),
                mc_version: "1.21".to_string(),
                loader: Some(LoaderInfo {
                    loader_type: LoaderType::Fabric,
                    version: "0.16.0".to_string(),
                }),
                game_dir: PathBuf::from("D:\\MC\\Instances\\Fabric-1.21"),
                created_at: now,
                last_played_at: Some(now),
            },
            Instance {
                id: "inst-vanilla-121".to_string(),
                name: "原版 1.21".to_string(),
                mc_version: "1.21".to_string(),
                loader: None,
                game_dir: PathBuf::from("D:\\MC\\Instances\\Vanilla-1.21"),
                created_at: now,
                last_played_at: None,
            },
            Instance {
                id: "inst-forge-1192".to_string(),
                name: "Forge 1.19.2".to_string(),
                mc_version: "1.19.2".to_string(),
                loader: Some(LoaderInfo {
                    loader_type: LoaderType::Forge,
                    version: "43.3.0".to_string(),
                }),
                game_dir: PathBuf::from("D:\\MC\\Instances\\Forge-1.19.2"),
                created_at: now,
                last_played_at: None,
            },
            Instance {
                id: "inst-modpack-1201".to_string(),
                name: "整合包实例".to_string(),
                mc_version: "1.20.1".to_string(),
                loader: Some(LoaderInfo {
                    loader_type: LoaderType::Fabric,
                    version: "Modpack".to_string(),
                }),
                game_dir: PathBuf::from("D:\\MC\\Instances\\Modpack-1.20.1"),
                created_at: now,
                last_played_at: None,
            },
        ];
        let mut map = self.instances.write().await;
        for s in samples {
            map.insert(s.id.clone(), s);
        }
        drop(map);
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
