//! 本地客户端发现与启动。共享依赖，游戏工作目录按版本隔离。
mod auth;
mod java;
mod launch;
mod layout;
mod metadata;
mod natives;
mod process;
mod rules;
mod scanner;

pub use auth::{
    AuthAccount, AuthProvider, AuthSession, CredentialStore, LoginStart, MemoryCredentialStore,
    MicrosoftAuthConfig, MicrosoftAuthProvider, StoredCredential, UnavailableAuthProvider,
};
pub use java::JavaRuntime;
pub use launch::{GameLauncher, LaunchCheck, LaunchOptions, LaunchPlan};
pub use layout::GameLayout;
pub use process::{GameProcess, ProcessExit};
pub use scanner::{LocalClient, LocalClientService, ScanIssue, ScanReport};
