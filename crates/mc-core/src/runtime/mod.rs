//! 本地客户端发现与启动。共享依赖，游戏工作目录按版本隔离。
mod auth;
mod install;
mod java;
mod launch;
mod layout;
mod logs;
pub mod diagnostics;
pub mod diagnostic_rules;
pub mod loaders;
mod metadata;
mod natives;
mod process;
mod rules;
mod scanner;

pub use auth::{
    offline_uuid, AuthAccount, AuthProvider, AuthSession, CredentialStore, DeviceCodeStart,
    DevicePollResult, LoginStart, MemoryCredentialStore, MicrosoftAuthConfig,
    MicrosoftAuthProvider, OfflineAuthProvider, StoredCredential, UnavailableAuthProvider,
};
pub use install::{install_vanilla, preview_vanilla, install_vanilla_plan, VanillaPlan};
pub use java::JavaRuntime;
pub use launch::{GameLauncher, LaunchCheck, LaunchOptions, LaunchPlan};
pub use layout::GameLayout;
pub use logs::{
    list_instance_logs, read_instance_log, InstanceLog, InstanceLogContent, MAX_LOG_BYTES,
};
pub use process::{GameProcess, ProcessExit};
pub use scanner::{LocalClient, LocalClientService, ScanIssue, ScanReport};
