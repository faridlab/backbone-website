use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for ContactMessage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContactMessageId(pub Uuid);

impl ContactMessageId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for ContactMessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ContactMessageId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for ContactMessageId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<ContactMessageId> for Uuid {
    fn from(id: ContactMessageId) -> Self { id.0 }
}

impl AsRef<Uuid> for ContactMessageId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for ContactMessageId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContactMessage {
    pub id: Uuid,
    pub website_id: Uuid,
    pub name: Option<String>,
    pub email: String,
    pub message: String,
    pub notified: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl ContactMessage {
    /// Create a builder for ContactMessage
    pub fn builder() -> ContactMessageBuilder {
        <ContactMessageBuilder as Default>::default()
    }

    /// Create a new ContactMessage with required fields
    pub fn new(website_id: Uuid, email: String, message: String, notified: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            website_id,
            name: None,
            email,
            message,
            notified,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> ContactMessageId {
        ContactMessageId(self.id)
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

    /// Set the name field (chainable)
    pub fn with_name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "website_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.website_id = v; }
                }
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "email" => {
                    if let Ok(v) = serde_json::from_value(value) { self.email = v; }
                }
                "message" => {
                    if let Ok(v) = serde_json::from_value(value) { self.message = v; }
                }
                "notified" => {
                    if let Ok(v) = serde_json::from_value(value) { self.notified = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for ContactMessage {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "ContactMessage"
    }
}

impl backbone_core::PersistentEntity for ContactMessage {
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

impl backbone_orm::EntityRepoMeta for ContactMessage {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("website_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["email", "message"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("website", "websites", "websiteId")]
    }
}

/// Builder for ContactMessage entity
///
/// Provides a fluent API for constructing ContactMessage instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct ContactMessageBuilder {
    website_id: Option<Uuid>,
    name: Option<String>,
    email: Option<String>,
    message: Option<String>,
    notified: Option<bool>,
}

impl ContactMessageBuilder {
    /// Set the website_id field (required)
    pub fn website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the name field (optional)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the email field (required)
    pub fn email(mut self, value: String) -> Self {
        self.email = Some(value);
        self
    }

    /// Set the message field (required)
    pub fn message(mut self, value: String) -> Self {
        self.message = Some(value);
        self
    }

    /// Set the notified field (default: `false`)
    pub fn notified(mut self, value: bool) -> Self {
        self.notified = Some(value);
        self
    }

    /// Build the ContactMessage entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<ContactMessage, String> {
        let website_id = self.website_id.ok_or_else(|| "website_id is required".to_string())?;
        let email = self.email.ok_or_else(|| "email is required".to_string())?;
        let message = self.message.ok_or_else(|| "message is required".to_string())?;

        Ok(ContactMessage {
            id: Uuid::new_v4(),
            website_id,
            name: self.name,
            email,
            message,
            notified: self.notified.unwrap_or(false),
            metadata: AuditMetadata::default(),
        })
    }
}
