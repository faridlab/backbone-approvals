use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AuditMetadata;
use super::DelegationStatus;

/// Strongly-typed ID for Delegation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DelegationId(pub Uuid);

impl DelegationId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for DelegationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for DelegationId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for DelegationId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<DelegationId> for Uuid {
    fn from(id: DelegationId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for DelegationId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for DelegationId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Delegation {
    pub id: Uuid,
    pub company_id: Uuid,
    pub approver_id: Uuid,
    pub delegate_to_id: Uuid,
    pub valid_from: NaiveDate,
    pub valid_to: NaiveDate,
    pub reason: Option<String>,
    pub status: DelegationStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Delegation {
    /// Create a builder for Delegation
    pub fn builder() -> DelegationBuilder {
        <DelegationBuilder as Default>::default()
    }

    /// Create a new Delegation with required fields
    pub fn new(
        company_id: Uuid,
        approver_id: Uuid,
        delegate_to_id: Uuid,
        valid_from: NaiveDate,
        valid_to: NaiveDate,
        status: DelegationStatus,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            approver_id,
            delegate_to_id,
            valid_from,
            valid_to,
            reason: None,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> DelegationId {
        DelegationId(self.id)
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
    pub fn status(&self) -> &DelegationStatus {
        &self.status
    }

    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the reason field (chainable)
    pub fn with_reason(mut self, value: String) -> Self {
        self.reason = Some(value);
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
                    if let Ok(v) = serde_json::from_value(value) {
                        self.company_id = v;
                    }
                }
                "approver_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.approver_id = v;
                    }
                }
                "delegate_to_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.delegate_to_id = v;
                    }
                }
                "valid_from" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.valid_from = v;
                    }
                }
                "valid_to" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.valid_to = v;
                    }
                }
                "reason" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.reason = v;
                    }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.status = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Delegation {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Delegation"
    }
}

impl backbone_core::PersistentEntity for Delegation {
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

impl backbone_orm::EntityRepoMeta for Delegation {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("approver_id".to_string(), "uuid".to_string());
        m.insert("delegate_to_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "delegation_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for Delegation entity
///
/// Provides a fluent API for constructing Delegation instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct DelegationBuilder {
    company_id: Option<Uuid>,
    approver_id: Option<Uuid>,
    delegate_to_id: Option<Uuid>,
    valid_from: Option<NaiveDate>,
    valid_to: Option<NaiveDate>,
    reason: Option<String>,
    status: Option<DelegationStatus>,
}

impl DelegationBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the approver_id field (required)
    pub fn approver_id(mut self, value: Uuid) -> Self {
        self.approver_id = Some(value);
        self
    }

    /// Set the delegate_to_id field (required)
    pub fn delegate_to_id(mut self, value: Uuid) -> Self {
        self.delegate_to_id = Some(value);
        self
    }

    /// Set the valid_from field (required)
    pub fn valid_from(mut self, value: NaiveDate) -> Self {
        self.valid_from = Some(value);
        self
    }

    /// Set the valid_to field (required)
    pub fn valid_to(mut self, value: NaiveDate) -> Self {
        self.valid_to = Some(value);
        self
    }

    /// Set the reason field (optional)
    pub fn reason(mut self, value: String) -> Self {
        self.reason = Some(value);
        self
    }

    /// Set the status field (default: `DelegationStatus::default()`)
    pub fn status(mut self, value: DelegationStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the Delegation entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Delegation, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;
        let approver_id = self
            .approver_id
            .ok_or_else(|| "approver_id is required".to_string())?;
        let delegate_to_id = self
            .delegate_to_id
            .ok_or_else(|| "delegate_to_id is required".to_string())?;
        let valid_from = self
            .valid_from
            .ok_or_else(|| "valid_from is required".to_string())?;
        let valid_to = self
            .valid_to
            .ok_or_else(|| "valid_to is required".to_string())?;

        Ok(Delegation {
            id: Uuid::new_v4(),
            company_id,
            approver_id,
            delegate_to_id,
            valid_from,
            valid_to,
            reason: self.reason,
            status: self.status.unwrap_or_default(),
            metadata: AuditMetadata::default(),
        })
    }
}
