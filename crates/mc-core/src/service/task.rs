use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{LauncherError, LauncherResult};
use crate::model::{Task, TaskProgress, TaskStatus, TaskType};

/// 异步后台任务调度与查询服务接口
#[async_trait]
pub trait TaskService: Send + Sync {
    /// 根据任务 ID 查询当前状态
    async fn get_task(&self, task_id: &str) -> LauncherResult<Task>;

    /// 获取全部任务清单
    async fn list_tasks(&self) -> LauncherResult<Vec<Task>>;

    /// 发起取消指定任务
    async fn cancel_task(&self, task_id: &str) -> LauncherResult<()>;
}

/// 阶段 0 内存任务服务实现
#[derive(Default, Clone)]
pub struct InMemoryTaskService {
    tasks: Arc<RwLock<HashMap<String, Task>>>,
}

impl InMemoryTaskService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入示例后台任务数据，供桌面端任务队列联调展示
    pub async fn with_sample_data(self) -> Self {
        let now = Utc::now();
        let t1 = Task {
            id: "task-mc-121".to_string(),
            task_type: TaskType::DownloadVersion,
            status: TaskStatus::Running,
            progress: TaskProgress {
                percentage: 56.0,
                step_description: "Minecraft 1.21 游戏本体|684 MB / 1.2 GB (56%)|12.3 MB/s|grass"
                    .to_string(),
            },
            error: None,
            created_at: now,
            updated_at: now,
        };
        let t2 = Task {
            id: "task-sodium-0511".to_string(),
            task_type: TaskType::InstallMod,
            status: TaskStatus::Running,
            progress: TaskProgress {
                percentage: 63.0,
                step_description: "Sodium-0.5.11.jar|663 MB / 1.1 GB (63%)|2.1 MB/s|sodium"
                    .to_string(),
            },
            error: None,
            created_at: now,
            updated_at: now,
        };
        let mut map = self.tasks.write().await;
        map.insert(t1.id.clone(), t1);
        map.insert(t2.id.clone(), t2);
        drop(map);
        self
    }

    /// 创建并注册一个新任务
    pub async fn register_task(&self, task_type: TaskType) -> Task {
        let id = format!("task-{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();
        let task = Task {
            id: id.clone(),
            task_type,
            status: TaskStatus::Pending,
            progress: TaskProgress {
                percentage: 0.0,
                step_description: "任务已排队".to_string(),
            },
            error: None,
            created_at: now,
            updated_at: now,
        };
        self.tasks.write().await.insert(id, task.clone());
        task
    }
}

#[async_trait]
impl TaskService for InMemoryTaskService {
    async fn get_task(&self, task_id: &str) -> LauncherResult<Task> {
        let map = self.tasks.read().await;
        map.get(task_id)
            .cloned()
            .ok_or_else(|| LauncherError::Internal(format!("任务不存在: {}", task_id)))
    }

    async fn list_tasks(&self) -> LauncherResult<Vec<Task>> {
        let map = self.tasks.read().await;
        Ok(map.values().cloned().collect())
    }

    async fn cancel_task(&self, task_id: &str) -> LauncherResult<()> {
        let mut map = self.tasks.write().await;
        if let Some(task) = map.get_mut(task_id) {
            task.status = TaskStatus::Cancelled;
            task.updated_at = Utc::now();
            Ok(())
        } else {
            Err(LauncherError::Internal(format!(
                "无法取消不存在的任务: {}",
                task_id
            )))
        }
    }
}
