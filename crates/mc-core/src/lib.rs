//! MC AI Launcher 核心领域服务与实体模型定义

pub mod error;
pub mod model;
pub mod runtime;
pub mod service;

pub use error::{LauncherError, LauncherResult};
pub use model::{
    catalog::{GameVersion, GameVersionType, ModProject},
    instance::{CreateInstanceRequest, Instance, LoaderInfo, LoaderType},
    plan::{InstallPlan, PlanAction, PlanOperation},
    task::{Task, TaskProgress, TaskStatus, TaskType},
};
pub use runtime::*;
pub use service::{
    catalog::{CatalogProvider, MockCatalogProvider},
    instance::{InstanceService, MockInstanceService},
    task::{InMemoryTaskService, TaskService},
};
