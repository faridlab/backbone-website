use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::WebsiteMemberRole;
use super::AuditMetadata;

/// Strongly-typed ID for WebsiteMember
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WebsiteMemberId(pub Uuid);

impl WebsiteMemberId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for WebsiteMemberId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WebsiteMemberId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for WebsiteMemberId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<WebsiteMemberId> for Uuid {
    fn from(id: WebsiteMemberId) -> Self { id.0 }
}

impl AsRef<Uuid> for WebsiteMemberId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for WebsiteMemberId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebsiteMember {
    pub id: Uuid,
    pub website_id: Uuid,
    pub portal_user_id: Uuid,
    pub role: WebsiteMemberRole,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl WebsiteMember {
    /// Create a builder for WebsiteMember
    pub fn builder() -> WebsiteMemberBuilder {
        <WebsiteMemberBuilder as Default>::default()
    }

    /// Create a new WebsiteMember with required fields
    pub fn new(website_id: Uuid, portal_user_id: Uuid, role: WebsiteMemberRole) -> Self {
        Self {
            id: Uuid::new_v4(),
            website_id,
            portal_user_id,
            role,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> WebsiteMemberId {
        WebsiteMemberId(self.id)
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
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "website_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.website_id = v; }
                }
                "portal_user_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.portal_user_id = v; }
                }
                "role" => {
                    if let Ok(v) = serde_json::from_value(value) { self.role = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for WebsiteMember {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "WebsiteMember"
    }
}

impl backbone_core::PersistentEntity for WebsiteMember {
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

impl backbone_orm::EntityRepoMeta for WebsiteMember {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("website_id".to_string(), "uuid".to_string());
        m.insert("portal_user_id".to_string(), "uuid".to_string());
        m.insert("role".to_string(), "website_member_role".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("website", "websites", "websiteId")]
    }
}

/// Builder for WebsiteMember entity
///
/// Provides a fluent API for constructing WebsiteMember instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct WebsiteMemberBuilder {
    website_id: Option<Uuid>,
    portal_user_id: Option<Uuid>,
    role: Option<WebsiteMemberRole>,
}

impl WebsiteMemberBuilder {
    /// Set the website_id field (required)
    pub fn website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the portal_user_id field (required)
    pub fn portal_user_id(mut self, value: Uuid) -> Self {
        self.portal_user_id = Some(value);
        self
    }

    /// Set the role field (default: `WebsiteMemberRole::default()`)
    pub fn role(mut self, value: WebsiteMemberRole) -> Self {
        self.role = Some(value);
        self
    }

    /// Build the WebsiteMember entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<WebsiteMember, String> {
        let website_id = self.website_id.ok_or_else(|| "website_id is required".to_string())?;
        let portal_user_id = self.portal_user_id.ok_or_else(|| "portal_user_id is required".to_string())?;

        Ok(WebsiteMember {
            id: Uuid::new_v4(),
            website_id,
            portal_user_id,
            role: self.role.unwrap_or_default(),
            metadata: AuditMetadata::default(),
        })
    }
}
