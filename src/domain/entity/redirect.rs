use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::WebsiteRedirectType;
use super::AuditMetadata;

/// Strongly-typed ID for Redirect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RedirectId(pub Uuid);

impl RedirectId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for RedirectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for RedirectId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for RedirectId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<RedirectId> for Uuid {
    fn from(id: RedirectId) -> Self { id.0 }
}

impl AsRef<Uuid> for RedirectId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for RedirectId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Redirect {
    pub id: Uuid,
    pub website_id: Uuid,
    pub url_from: String,
    pub redirect_type: WebsiteRedirectType,
    pub url_to: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Redirect {
    /// Create a builder for Redirect
    pub fn builder() -> RedirectBuilder {
        <RedirectBuilder as Default>::default()
    }

    /// Create a new Redirect with required fields
    pub fn new(website_id: Uuid, url_from: String, redirect_type: WebsiteRedirectType) -> Self {
        Self {
            id: Uuid::new_v4(),
            website_id,
            url_from,
            redirect_type,
            url_to: None,
            description: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> RedirectId {
        RedirectId(self.id)
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

    /// Set the url_to field (chainable)
    pub fn with_url_to(mut self, value: String) -> Self {
        self.url_to = Some(value);
        self
    }

    /// Set the description field (chainable)
    pub fn with_description(mut self, value: String) -> Self {
        self.description = Some(value);
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
                "url_from" => {
                    if let Ok(v) = serde_json::from_value(value) { self.url_from = v; }
                }
                "redirect_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.redirect_type = v; }
                }
                "url_to" => {
                    if let Ok(v) = serde_json::from_value(value) { self.url_to = v; }
                }
                "description" => {
                    if let Ok(v) = serde_json::from_value(value) { self.description = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Redirect {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Redirect"
    }
}

impl backbone_core::PersistentEntity for Redirect {
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

impl backbone_orm::EntityRepoMeta for Redirect {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("website_id".to_string(), "uuid".to_string());
        m.insert("redirect_type".to_string(), "website_redirect_type".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["url_from"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("website", "websites", "websiteId")]
    }
}

/// Builder for Redirect entity
///
/// Provides a fluent API for constructing Redirect instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct RedirectBuilder {
    website_id: Option<Uuid>,
    url_from: Option<String>,
    redirect_type: Option<WebsiteRedirectType>,
    url_to: Option<String>,
    description: Option<String>,
}

impl RedirectBuilder {
    /// Set the website_id field (required)
    pub fn website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the url_from field (required)
    pub fn url_from(mut self, value: String) -> Self {
        self.url_from = Some(value);
        self
    }

    /// Set the redirect_type field (default: `WebsiteRedirectType::default()`)
    pub fn redirect_type(mut self, value: WebsiteRedirectType) -> Self {
        self.redirect_type = Some(value);
        self
    }

    /// Set the url_to field (optional)
    pub fn url_to(mut self, value: String) -> Self {
        self.url_to = Some(value);
        self
    }

    /// Set the description field (optional)
    pub fn description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    /// Build the Redirect entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Redirect, String> {
        let website_id = self.website_id.ok_or_else(|| "website_id is required".to_string())?;
        let url_from = self.url_from.ok_or_else(|| "url_from is required".to_string())?;

        Ok(Redirect {
            id: Uuid::new_v4(),
            website_id,
            url_from,
            redirect_type: self.redirect_type.unwrap_or_default(),
            url_to: self.url_to,
            description: self.description,
            metadata: AuditMetadata::default(),
        })
    }
}
