use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::ApproverKind;
use super::AuditMetadata;

/// Strongly-typed ID for ApprovalStepTemplate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApprovalStepTemplateId(pub Uuid);

impl ApprovalStepTemplateId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for ApprovalStepTemplateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ApprovalStepTemplateId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for ApprovalStepTemplateId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<ApprovalStepTemplateId> for Uuid {
    fn from(id: ApprovalStepTemplateId) -> Self { id.0 }
}

impl AsRef<Uuid> for ApprovalStepTemplateId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for ApprovalStepTemplateId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApprovalStepTemplate {
    pub id: Uuid,
    pub company_id: Uuid,
    pub policy_id: Uuid,
    pub step_no: i32,
    pub approver_kind: ApproverKind,
    pub approver_ref: Option<Uuid>,
    pub sla_hours: Option<i32>,
    pub all_of: Option<serde_json::Value>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl ApprovalStepTemplate {
    /// Create a builder for ApprovalStepTemplate
    pub fn builder() -> ApprovalStepTemplateBuilder {
        ApprovalStepTemplateBuilder::default()
    }

    /// Create a new ApprovalStepTemplate with required fields
    pub fn new(company_id: Uuid, policy_id: Uuid, step_no: i32, approver_kind: ApproverKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            policy_id,
            step_no,
            approver_kind,
            approver_ref: None,
            sla_hours: None,
            all_of: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> ApprovalStepTemplateId {
        ApprovalStepTemplateId(self.id)
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


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the approver_ref field (chainable)
    pub fn with_approver_ref(mut self, value: Uuid) -> Self {
        self.approver_ref = Some(value);
        self
    }

    /// Set the sla_hours field (chainable)
    pub fn with_sla_hours(mut self, value: i32) -> Self {
        self.sla_hours = Some(value);
        self
    }

    /// Set the all_of field (chainable)
    pub fn with_all_of(mut self, value: serde_json::Value) -> Self {
        self.all_of = Some(value);
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
                "policy_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.policy_id = v; }
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
                "sla_hours" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sla_hours = v; }
                }
                "all_of" => {
                    if let Ok(v) = serde_json::from_value(value) { self.all_of = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for ApprovalStepTemplate {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "ApprovalStepTemplate"
    }
}

impl backbone_core::PersistentEntity for ApprovalStepTemplate {
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

impl backbone_orm::EntityRepoMeta for ApprovalStepTemplate {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("policy_id".to_string(), "uuid".to_string());
        m.insert("approver_kind".to_string(), "approver_kind".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for ApprovalStepTemplate entity
///
/// Provides a fluent API for constructing ApprovalStepTemplate instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct ApprovalStepTemplateBuilder {
    company_id: Option<Uuid>,
    policy_id: Option<Uuid>,
    step_no: Option<i32>,
    approver_kind: Option<ApproverKind>,
    approver_ref: Option<Uuid>,
    sla_hours: Option<i32>,
    all_of: Option<serde_json::Value>,
}

impl ApprovalStepTemplateBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the policy_id field (required)
    pub fn policy_id(mut self, value: Uuid) -> Self {
        self.policy_id = Some(value);
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

    /// Set the sla_hours field (optional)
    pub fn sla_hours(mut self, value: i32) -> Self {
        self.sla_hours = Some(value);
        self
    }

    /// Set the all_of field (optional)
    pub fn all_of(mut self, value: serde_json::Value) -> Self {
        self.all_of = Some(value);
        self
    }

    /// Build the ApprovalStepTemplate entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<ApprovalStepTemplate, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let policy_id = self.policy_id.ok_or_else(|| "policy_id is required".to_string())?;
        let step_no = self.step_no.ok_or_else(|| "step_no is required".to_string())?;
        let approver_kind = self.approver_kind.ok_or_else(|| "approver_kind is required".to_string())?;

        Ok(ApprovalStepTemplate {
            id: Uuid::new_v4(),
            company_id,
            policy_id,
            step_no,
            approver_kind,
            approver_ref: self.approver_ref,
            sla_hours: self.sla_hours,
            all_of: self.all_of,
            metadata: AuditMetadata::default(),
        })
    }
}
