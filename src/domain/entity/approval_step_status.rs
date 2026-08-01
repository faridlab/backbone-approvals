use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "approval_step_status", rename_all = "snake_case")]
pub enum ApprovalStepStatus {
    Pending,
    Approved,
    Rejected,
    Delegated,
    Skipped,
    Escalated,
}

impl std::fmt::Display for ApprovalStepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Approved => write!(f, "approved"),
            Self::Rejected => write!(f, "rejected"),
            Self::Delegated => write!(f, "delegated"),
            Self::Skipped => write!(f, "skipped"),
            Self::Escalated => write!(f, "escalated"),
        }
    }
}

impl FromStr for ApprovalStepStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "delegated" => Ok(Self::Delegated),
            "skipped" => Ok(Self::Skipped),
            "escalated" => Ok(Self::Escalated),
            _ => Err(format!("Unknown ApprovalStepStatus variant: {}", s)),
        }
    }
}

impl Default for ApprovalStepStatus {
    fn default() -> Self {
        Self::Pending
    }
}
