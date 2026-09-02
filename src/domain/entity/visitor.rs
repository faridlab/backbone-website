use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::WebsiteVisitorKind;
use super::AuditMetadata;

/// Strongly-typed ID for Visitor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VisitorId(pub Uuid);

impl VisitorId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for VisitorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for VisitorId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for VisitorId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<VisitorId> for Uuid {
    fn from(id: VisitorId) -> Self { id.0 }
}

impl AsRef<Uuid> for VisitorId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for VisitorId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Visitor {
    pub id: Uuid,
    pub website_id: Uuid,
    pub access_token: String,
    pub kind: WebsiteVisitorKind,
    pub digest: String,
    pub digest_algo: String,
    pub portal_user_id: Option<Uuid>,
    pub country_code: Option<String>,
    pub visit_count: i32,
    pub last_connection_at: DateTime<Utc>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Visitor {
    /// Create a builder for Visitor
    pub fn builder() -> VisitorBuilder {
        <VisitorBuilder as Default>::default()
    }

    /// Create a new Visitor with required fields
    pub fn new(website_id: Uuid, access_token: String, kind: WebsiteVisitorKind, digest: String, digest_algo: String, visit_count: i32, last_connection_at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            website_id,
            access_token,
            kind,
            digest,
            digest_algo,
            portal_user_id: None,
            country_code: None,
            visit_count,
            last_connection_at,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> VisitorId {
        VisitorId(self.id)
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

    /// Set the portal_user_id field (chainable)
    pub fn with_portal_user_id(mut self, value: Uuid) -> Self {
        self.portal_user_id = Some(value);
        self
    }

    /// Set the country_code field (chainable)
    pub fn with_country_code(mut self, value: String) -> Self {
        self.country_code = Some(value);
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
                "access_token" => {
                    if let Ok(v) = serde_json::from_value(value) { self.access_token = v; }
                }
                "kind" => {
                    if let Ok(v) = serde_json::from_value(value) { self.kind = v; }
                }
                "digest" => {
                    if let Ok(v) = serde_json::from_value(value) { self.digest = v; }
                }
                "digest_algo" => {
                    if let Ok(v) = serde_json::from_value(value) { self.digest_algo = v; }
                }
                "portal_user_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.portal_user_id = v; }
                }
                "country_code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.country_code = v; }
                }
                "visit_count" => {
                    if let Ok(v) = serde_json::from_value(value) { self.visit_count = v; }
                }
                "last_connection_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.last_connection_at = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Visitor {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Visitor"
    }
}

impl backbone_core::PersistentEntity for Visitor {
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

impl backbone_orm::EntityRepoMeta for Visitor {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("website_id".to_string(), "uuid".to_string());
        m.insert("portal_user_id".to_string(), "uuid".to_string());
        m.insert("kind".to_string(), "website_visitor_kind".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["access_token", "digest", "digest_algo"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("website", "websites", "websiteId")]
    }
}

/// Builder for Visitor entity
///
/// Provides a fluent API for constructing Visitor instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct VisitorBuilder {
    website_id: Option<Uuid>,
    access_token: Option<String>,
    kind: Option<WebsiteVisitorKind>,
    digest: Option<String>,
    digest_algo: Option<String>,
    portal_user_id: Option<Uuid>,
    country_code: Option<String>,
    visit_count: Option<i32>,
    last_connection_at: Option<DateTime<Utc>>,
}

impl VisitorBuilder {
    /// Set the website_id field (required)
    pub fn website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the access_token field (required)
    pub fn access_token(mut self, value: String) -> Self {
        self.access_token = Some(value);
        self
    }

    /// Set the kind field (default: `WebsiteVisitorKind::default()`)
    pub fn kind(mut self, value: WebsiteVisitorKind) -> Self {
        self.kind = Some(value);
        self
    }

    /// Set the digest field (required)
    pub fn digest(mut self, value: String) -> Self {
        self.digest = Some(value);
        self
    }

    /// Set the digest_algo field (default: `"hmac-sha256-v1".to_string()`)
    pub fn digest_algo(mut self, value: String) -> Self {
        self.digest_algo = Some(value);
        self
    }

    /// Set the portal_user_id field (optional)
    pub fn portal_user_id(mut self, value: Uuid) -> Self {
        self.portal_user_id = Some(value);
        self
    }

    /// Set the country_code field (optional)
    pub fn country_code(mut self, value: String) -> Self {
        self.country_code = Some(value);
        self
    }

    /// Set the visit_count field (default: `0`)
    pub fn visit_count(mut self, value: i32) -> Self {
        self.visit_count = Some(value);
        self
    }

    /// Set the last_connection_at field (default: `Utc::now()`)
    pub fn last_connection_at(mut self, value: DateTime<Utc>) -> Self {
        self.last_connection_at = Some(value);
        self
    }

    /// Build the Visitor entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Visitor, String> {
        let website_id = self.website_id.ok_or_else(|| "website_id is required".to_string())?;
        let access_token = self.access_token.ok_or_else(|| "access_token is required".to_string())?;
        let digest = self.digest.ok_or_else(|| "digest is required".to_string())?;

        Ok(Visitor {
            id: Uuid::new_v4(),
            website_id,
            access_token,
            kind: self.kind.unwrap_or_default(),
            digest,
            digest_algo: self.digest_algo.unwrap_or("hmac-sha256-v1".to_string()),
            portal_user_id: self.portal_user_id,
            country_code: self.country_code,
            visit_count: self.visit_count.unwrap_or(0),
            last_connection_at: self.last_connection_at.unwrap_or(Utc::now()),
            metadata: AuditMetadata::default(),
        })
    }
}
