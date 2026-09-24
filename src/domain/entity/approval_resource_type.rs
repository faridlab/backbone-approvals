use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "approval_resource_type", rename_all = "snake_case")]
pub enum ApprovalResourceType {
    Promotion,
    Onboarding,
    OnboardingTask,
    Offboarding,
    Clearance,
    Leave,
    Timesheet,
    Offer,
    Appraisal,
    Custom,
    Expense,
    EmployeeRecord,
    OvertimeRequest,
    Resignation,
    Requisition,
    AttendanceCorrection,
}

impl std::fmt::Display for ApprovalResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Promotion => write!(f, "promotion"),
            Self::Onboarding => write!(f, "onboarding"),
            Self::OnboardingTask => write!(f, "onboarding_task"),
            Self::Offboarding => write!(f, "offboarding"),
            Self::Clearance => write!(f, "clearance"),
            Self::Leave => write!(f, "leave"),
            Self::Timesheet => write!(f, "timesheet"),
            Self::Offer => write!(f, "offer"),
            Self::Appraisal => write!(f, "appraisal"),
            Self::Custom => write!(f, "custom"),
            Self::Expense => write!(f, "expense"),
            Self::EmployeeRecord => write!(f, "employee_record"),
            Self::OvertimeRequest => write!(f, "overtime_request"),
            Self::Resignation => write!(f, "resignation"),
            Self::Requisition => write!(f, "requisition"),
            Self::AttendanceCorrection => write!(f, "attendance_correction"),
        }
    }
}

impl FromStr for ApprovalResourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "promotion" => Ok(Self::Promotion),
            "onboarding" => Ok(Self::Onboarding),
            "onboarding_task" => Ok(Self::OnboardingTask),
            "offboarding" => Ok(Self::Offboarding),
            "clearance" => Ok(Self::Clearance),
            "leave" => Ok(Self::Leave),
            "timesheet" => Ok(Self::Timesheet),
            "offer" => Ok(Self::Offer),
            "appraisal" => Ok(Self::Appraisal),
            "custom" => Ok(Self::Custom),
            "expense" => Ok(Self::Expense),
            "employee_record" => Ok(Self::EmployeeRecord),
            "overtime_request" => Ok(Self::OvertimeRequest),
            "resignation" => Ok(Self::Resignation),
            "requisition" => Ok(Self::Requisition),
            "attendance_correction" => Ok(Self::AttendanceCorrection),
            _ => Err(format!("Unknown ApprovalResourceType variant: {}", s)),
        }
    }
}

impl Default for ApprovalResourceType {
    fn default() -> Self {
        Self::Custom
    }
}
