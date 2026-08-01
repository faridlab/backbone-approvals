use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::ApprovalResourceType;
use super::ApprovalStatus;
use super::ApprovalPriority;
use super::AuditMetadata;

/// Strongly-typed ID for ApprovalRequest
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApprovalRequestId(pub Uuid);

impl ApprovalRequestId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for ApprovalRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ApprovalRequestId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for ApprovalRequestId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<ApprovalRequestId> for Uuid {
    fn from(id: ApprovalRequestId) -> Self { id.0 }
}

impl AsRef<Uuid> for ApprovalRequestId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for ApprovalRequestId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub company_id: Uuid,
    pub resource_type: ApprovalResourceType,
    pub resource_id: Uuid,
    pub policy_id: Option<Uuid>,
    pub requested_by: Uuid,
    pub status: ApprovalStatus,
    pub current_step: Option<i32>,
    pub priority: ApprovalPriority,
    pub submitted_at: Option<DateTime<Utc>>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by: Option<Uuid>,
    pub summary: Option<serde_json::Value>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl ApprovalRequest {
    /// Create a builder for ApprovalRequest
    pub fn builder() -> ApprovalRequestBuilder {
        ApprovalRequestBuilder::default()
    }

    /// Create a new ApprovalRequest with required fields
    pub fn new(company_id: Uuid, resource_type: ApprovalResourceType, resource_id: Uuid, requested_by: Uuid, status: ApprovalStatus, priority: ApprovalPriority) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            resource_type,
            resource_id,
            policy_id: None,
            requested_by,
            status,
            current_step: None,
            priority,
            submitted_at: None,
            decided_at: None,
            decided_by: None,
            summary: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> ApprovalRequestId {
        ApprovalRequestId(self.id)
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
    pub fn status(&self) -> &ApprovalStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the policy_id field (chainable)
    pub fn with_policy_id(mut self, value: Uuid) -> Self {
        self.policy_id = Some(value);
        self
    }

    /// Set the current_step field (chainable)
    pub fn with_current_step(mut self, value: i32) -> Self {
        self.current_step = Some(value);
        self
    }

    /// Set the submitted_at field (chainable)
    pub fn with_submitted_at(mut self, value: DateTime<Utc>) -> Self {
        self.submitted_at = Some(value);
        self
    }

    /// Set the decided_at field (chainable)
    pub fn with_decided_at(mut self, value: DateTime<Utc>) -> Self {
        self.decided_at = Some(value);
        self
    }

    /// Set the decided_by field (chainable)
    pub fn with_decided_by(mut self, value: Uuid) -> Self {
        self.decided_by = Some(value);
        self
    }

    /// Set the summary field (chainable)
    pub fn with_summary(mut self, value: serde_json::Value) -> Self {
        self.summary = Some(value);
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
                "resource_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.resource_type = v; }
                }
                "resource_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.resource_id = v; }
                }
                "policy_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.policy_id = v; }
                }
                "requested_by" => {
                    if let Ok(v) = serde_json::from_value(value) { self.requested_by = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "current_step" => {
                    if let Ok(v) = serde_json::from_value(value) { self.current_step = v; }
                }
                "priority" => {
                    if let Ok(v) = serde_json::from_value(value) { self.priority = v; }
                }
                "submitted_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.submitted_at = v; }
                }
                "decided_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.decided_at = v; }
                }
                "decided_by" => {
                    if let Ok(v) = serde_json::from_value(value) { self.decided_by = v; }
                }
                "summary" => {
                    if let Ok(v) = serde_json::from_value(value) { self.summary = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for ApprovalRequest {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "ApprovalRequest"
    }
}

impl backbone_core::PersistentEntity for ApprovalRequest {
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

impl backbone_orm::EntityRepoMeta for ApprovalRequest {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("resource_id".to_string(), "uuid".to_string());
        m.insert("policy_id".to_string(), "uuid".to_string());
        m.insert("resource_type".to_string(), "approval_resource_type".to_string());
        m.insert("status".to_string(), "approval_status".to_string());
        m.insert("priority".to_string(), "approval_priority".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for ApprovalRequest entity
///
/// Provides a fluent API for constructing ApprovalRequest instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct ApprovalRequestBuilder {
    company_id: Option<Uuid>,
    resource_type: Option<ApprovalResourceType>,
    resource_id: Option<Uuid>,
    policy_id: Option<Uuid>,
    requested_by: Option<Uuid>,
    status: Option<ApprovalStatus>,
    current_step: Option<i32>,
    priority: Option<ApprovalPriority>,
    submitted_at: Option<DateTime<Utc>>,
    decided_at: Option<DateTime<Utc>>,
    decided_by: Option<Uuid>,
    summary: Option<serde_json::Value>,
}

impl ApprovalRequestBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the resource_type field (required)
    pub fn resource_type(mut self, value: ApprovalResourceType) -> Self {
        self.resource_type = Some(value);
        self
    }

    /// Set the resource_id field (required)
    pub fn resource_id(mut self, value: Uuid) -> Self {
        self.resource_id = Some(value);
        self
    }

    /// Set the policy_id field (optional)
    pub fn policy_id(mut self, value: Uuid) -> Self {
        self.policy_id = Some(value);
        self
    }

    /// Set the requested_by field (required)
    pub fn requested_by(mut self, value: Uuid) -> Self {
        self.requested_by = Some(value);
        self
    }

    /// Set the status field (default: `ApprovalStatus::default()`)
    pub fn status(mut self, value: ApprovalStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the current_step field (optional)
    pub fn current_step(mut self, value: i32) -> Self {
        self.current_step = Some(value);
        self
    }

    /// Set the priority field (default: `ApprovalPriority::default()`)
    pub fn priority(mut self, value: ApprovalPriority) -> Self {
        self.priority = Some(value);
        self
    }

    /// Set the submitted_at field (optional)
    pub fn submitted_at(mut self, value: DateTime<Utc>) -> Self {
        self.submitted_at = Some(value);
        self
    }

    /// Set the decided_at field (optional)
    pub fn decided_at(mut self, value: DateTime<Utc>) -> Self {
        self.decided_at = Some(value);
        self
    }

    /// Set the decided_by field (optional)
    pub fn decided_by(mut self, value: Uuid) -> Self {
        self.decided_by = Some(value);
        self
    }

    /// Set the summary field (optional)
    pub fn summary(mut self, value: serde_json::Value) -> Self {
        self.summary = Some(value);
        self
    }

    /// Build the ApprovalRequest entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<ApprovalRequest, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let resource_type = self.resource_type.ok_or_else(|| "resource_type is required".to_string())?;
        let resource_id = self.resource_id.ok_or_else(|| "resource_id is required".to_string())?;
        let requested_by = self.requested_by.ok_or_else(|| "requested_by is required".to_string())?;

        Ok(ApprovalRequest {
            id: Uuid::new_v4(),
            company_id,
            resource_type,
            resource_id,
            policy_id: self.policy_id,
            requested_by,
            status: self.status.unwrap_or(ApprovalStatus::default()),
            current_step: self.current_step,
            priority: self.priority.unwrap_or(ApprovalPriority::default()),
            submitted_at: self.submitted_at,
            decided_at: self.decided_at,
            decided_by: self.decided_by,
            summary: self.summary,
            metadata: AuditMetadata::default(),
        })
    }
}
