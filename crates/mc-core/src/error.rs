use thiserror::Error;

/// 启动器统一错误枚举与语义化错误码
#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("无效的游戏版本: {0}")]
    InvalidVersion(String),

    #[error("未找到指定的实例: {0}")]
    InstanceNotFound(String),

    #[error("Mod 依赖冲突: {0}")]
    DependencyConflict(String),

    #[error("文件校验哈希不匹配: 期望 {expected}, 实际 {actual}")]
    DownloadHashMismatch { expected: String, actual: String },

    #[error("该操作需要用户授权: {0}")]
    AuthorizationRequired(String),

    #[error("安装计划已过期或目标实例已被并发修改")]
    PlanStale,

    #[error("认证服务暂不可用或未获得官方批准: {0}")]
    AuthUnavailable(String),

    #[error("本地文件或路径异常: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON 序列化或解析失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("未预期的内部错误: {0}")]
    Internal(String),
}

pub type LauncherResult<T> = Result<T, LauncherError>;
