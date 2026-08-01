use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "approver_kind", rename_all = "snake_case")]
pub enum ApproverKind {
    SpecificEmployee,
    ManagerOfRequester,
    DepartmentHead,
    Role,
    Position,
}

impl std::fmt::Display for ApproverKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpecificEmployee => write!(f, "specific_employee"),
            Self::ManagerOfRequester => write!(f, "manager_of_requester"),
            Self::DepartmentHead => write!(f, "department_head"),
            Self::Role => write!(f, "role"),
            Self::Position => write!(f, "position"),
        }
    }
}

impl FromStr for ApproverKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "specific_employee" => Ok(Self::SpecificEmployee),
            "manager_of_requester" => Ok(Self::ManagerOfRequester),
            "department_head" => Ok(Self::DepartmentHead),
            "role" => Ok(Self::Role),
            "position" => Ok(Self::Position),
            _ => Err(format!("Unknown ApproverKind variant: {}", s)),
        }
    }
}

impl Default for ApproverKind {
    fn default() -> Self {
        Self::SpecificEmployee
    }
}
