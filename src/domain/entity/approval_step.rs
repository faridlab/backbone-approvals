use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::ApproverKind;
use super::ApprovalStepStatus;
use super::AuditMetadata;

/// Strongly-typed ID for ApprovalStep
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApprovalStepId(pub Uuid);

impl ApprovalStepId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for ApprovalStepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ApprovalStepId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for ApprovalStepId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<ApprovalStepId> for Uuid {
    fn from(id: ApprovalStepId) -> Self { id.0 }
}

impl AsRef<Uuid> for ApprovalStepId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for ApprovalStepId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApprovalStep {
    pub id: Uuid,
    pub company_id: Uuid,
    pub request_id: Uuid,
    pub step_no: i32,
    pub approver_kind: ApproverKind,
    pub approver_ref: Option<Uuid>,
    pub assigned_to: Uuid,
    pub delegated_from: Option<Uuid>,
    pub status: ApprovalStepStatus,
    pub acted_at: Option<DateTime<Utc>>,
    pub comment: Option<String>,
    pub sla_due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl ApprovalStep {
    /// Create a builder for ApprovalStep
    pub fn builder() -> ApprovalStepBuilder {
        <ApprovalStepBuilder as Default>::default()
    }

    /// Create a new ApprovalStep with required fields
    pub fn new(company_id: Uuid, request_id: Uuid, step_no: i32, approver_kind: ApproverKind, assigned_to: Uuid, status: ApprovalStepStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            request_id,
            step_no,
            approver_kind,
            approver_ref: None,
            assigned_to,
            delegated_from: None,
            status,
            acted_at: None,
            comment: None,
            sla_due_at: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> ApprovalStepId {
        ApprovalStepId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }

    /// Get the current status
    pub fn status(&self) -> &ApprovalStepStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the approver_ref field (chainable)
    pub fn with_approver_ref(mut self, value: Uuid) -> Self {
        self.approver_ref = Some(value);
        self
    }

    /// Set the delegated_from field (chainable)
    pub fn with_delegated_from(mut self, value: Uuid) -> Self {
        self.delegated_from = Some(value);
        self
    }

    /// Set the acted_at field (chainable)
    pub fn with_acted_at(mut self, value: DateTime<Utc>) -> Self {
        self.acted_at = Some(value);
        self
    }

    /// Set the comment field (chainable)
    pub fn with_comment(mut self, value: String) -> Self {
        self.comment = Some(value);
        self
    }

    /// Set the sla_due_at field (chainable)
    pub fn with_sla_due_at(mut self, value: DateTime<Utc>) -> Self {
        self.sla_due_at = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "request_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.request_id = v; }
                }
                "step_no" => {
                    if let Ok(v) = serde_json::from_value(value) { self.step_no = v; }
                }
                "approver_kind" => {
                    if let Ok(v) = serde_json::from_value(value) { self.approver_kind = v; }
                }
                "approver_ref" => {
                    if let Ok(v) = serde_json::from_value(value) { self.approver_ref = v; }
                }
                "assigned_to" => {
                    if let Ok(v) = serde_json::from_value(value) { self.assigned_to = v; }
                }
                "delegated_from" => {
                    if let Ok(v) = serde_json::from_value(value) { self.delegated_from = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "acted_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.acted_at = v; }
                }
                "comment" => {
                    if let Ok(v) = serde_json::from_value(value) { self.comment = v; }
                }
                "sla_due_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sla_due_at = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for ApprovalStep {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "ApprovalStep"
    }
}

impl backbone_core::PersistentEntity for ApprovalStep {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for ApprovalStep {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("request_id".to_string(), "uuid".to_string());
        m.insert("approver_kind".to_string(), "approver_kind".to_string());
        m.insert("status".to_string(), "approval_step_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for ApprovalStep entity
///
/// Provides a fluent API for constructing ApprovalStep instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct ApprovalStepBuilder {
    company_id: Option<Uuid>,
    request_id: Option<Uuid>,
    step_no: Option<i32>,
    approver_kind: Option<ApproverKind>,
    approver_ref: Option<Uuid>,
    assigned_to: Option<Uuid>,
    delegated_from: Option<Uuid>,
    status: Option<ApprovalStepStatus>,
    acted_at: Option<DateTime<Utc>>,
    comment: Option<String>,
    sla_due_at: Option<DateTime<Utc>>,
}

impl ApprovalStepBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the request_id field (required)
    pub fn request_id(mut self, value: Uuid) -> Self {
        self.request_id = Some(value);
        self
    }

    /// Set the step_no field (required)
    pub fn step_no(mut self, value: i32) -> Self {
        self.step_no = Some(value);
        self
    }

    /// Set the approver_kind field (required)
    pub fn approver_kind(mut self, value: ApproverKind) -> Self {
        self.approver_kind = Some(value);
        self
    }

    /// Set the approver_ref field (optional)
    pub fn approver_ref(mut self, value: Uuid) -> Self {
        self.approver_ref = Some(value);
        self
    }

    /// Set the assigned_to field (required)
    pub fn assigned_to(mut self, value: Uuid) -> Self {
        self.assigned_to = Some(value);
        self
    }

    /// Set the delegated_from field (optional)
    pub fn delegated_from(mut self, value: Uuid) -> Self {
        self.delegated_from = Some(value);
        self
    }

    /// Set the status field (default: `ApprovalStepStatus::default()`)
    pub fn status(mut self, value: ApprovalStepStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the acted_at field (optional)
    pub fn acted_at(mut self, value: DateTime<Utc>) -> Self {
        self.acted_at = Some(value);
        self
    }

    /// Set the comment field (optional)
    pub fn comment(mut self, value: String) -> Self {
        self.comment = Some(value);
        self
    }

    /// Set the sla_due_at field (optional)
    pub fn sla_due_at(mut self, value: DateTime<Utc>) -> Self {
        self.sla_due_at = Some(value);
        self
    }

    /// Build the ApprovalStep entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<ApprovalStep, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let request_id = self.request_id.ok_or_else(|| "request_id is required".to_string())?;
        let step_no = self.step_no.ok_or_else(|| "step_no is required".to_string())?;
        let approver_kind = self.approver_kind.ok_or_else(|| "approver_kind is required".to_string())?;
        let assigned_to = self.assigned_to.ok_or_else(|| "assigned_to is required".to_string())?;

        Ok(ApprovalStep {
            id: Uuid::new_v4(),
            company_id,
            request_id,
            step_no,
            approver_kind,
            approver_ref: self.approver_ref,
            assigned_to,
            delegated_from: self.delegated_from,
            status: self.status.unwrap_or_default(),
            acted_at: self.acted_at,
            comment: self.comment,
            sla_due_at: self.sla_due_at,
            metadata: AuditMetadata::default(),
        })
    }
}
