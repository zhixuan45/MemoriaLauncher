use serde::{Deserialize, Serialize};

/// 变更动作类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    AddMod,
    RemoveMod,
    UpdateMod,
    InstallLoader,
}

/// 单项变更操作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOperation {
    pub action: PlanAction,
    pub item_name: String,
    pub version: String,
    pub reason: String,
}

/// 安装变更计划（先计划再执行的核心实体）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub plan_id: String,
    pub instance_id: String,
    pub operations: Vec<PlanOperation>,
    pub risk_warnings: Vec<String>,
    pub approved: bool,
    pub plan_hash: String,
}
