use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::WebsiteVisibility;
use super::AuditMetadata;

/// Strongly-typed ID for Page
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageId(pub Uuid);

impl PageId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PageId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for PageId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<PageId> for Uuid {
    fn from(id: PageId) -> Self { id.0 }
}

impl AsRef<Uuid> for PageId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for PageId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Page {
    pub id: Uuid,
    pub key: String,
    pub website_id: Option<Uuid>,
    pub url: String,
    pub title: String,
    pub seo_name: Option<String>,
    pub is_published: bool,
    pub date_publish: Option<DateTime<Utc>>,
    pub website_indexed: bool,
    pub visibility: WebsiteVisibility,
    pub required_member_roles: Vec<String>,
    pub forked_from: Option<Uuid>,
    pub forked_at: Option<DateTime<Utc>>,
    pub forked_by: Option<Uuid>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Page {
    /// Create a builder for Page
    pub fn builder() -> PageBuilder {
        <PageBuilder as Default>::default()
    }

    /// Create a new Page with required fields
    pub fn new(key: String, url: String, title: String, is_published: bool, website_indexed: bool, visibility: WebsiteVisibility, required_member_roles: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            key,
            website_id: None,
            url,
            title,
            seo_name: None,
            is_published,
            date_publish: None,
            website_indexed,
            visibility,
            required_member_roles,
            forked_from: None,
            forked_at: None,
            forked_by: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> PageId {
        PageId(self.id)
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

    /// Set the website_id field (chainable)
    pub fn with_website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the seo_name field (chainable)
    pub fn with_seo_name(mut self, value: String) -> Self {
        self.seo_name = Some(value);
        self
    }

    /// Set the date_publish field (chainable)
    pub fn with_date_publish(mut self, value: DateTime<Utc>) -> Self {
        self.date_publish = Some(value);
        self
    }

    /// Set the forked_from field (chainable)
    pub fn with_forked_from(mut self, value: Uuid) -> Self {
        self.forked_from = Some(value);
        self
    }

    /// Set the forked_at field (chainable)
    pub fn with_forked_at(mut self, value: DateTime<Utc>) -> Self {
        self.forked_at = Some(value);
        self
    }

    /// Set the forked_by field (chainable)
    pub fn with_forked_by(mut self, value: Uuid) -> Self {
        self.forked_by = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "key" => {
                    if let Ok(v) = serde_json::from_value(value) { self.key = v; }
                }
                "website_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.website_id = v; }
                }
                "url" => {
                    if let Ok(v) = serde_json::from_value(value) { self.url = v; }
                }
                "title" => {
                    if let Ok(v) = serde_json::from_value(value) { self.title = v; }
                }
                "seo_name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.seo_name = v; }
                }
                "is_published" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_published = v; }
                }
                "date_publish" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date_publish = v; }
                }
                "website_indexed" => {
                    if let Ok(v) = serde_json::from_value(value) { self.website_indexed = v; }
                }
                "visibility" => {
                    if let Ok(v) = serde_json::from_value(value) { self.visibility = v; }
                }
                "required_member_roles" => {
                    if let Ok(v) = serde_json::from_value(value) { self.required_member_roles = v; }
                }
                "forked_from" => {
                    if let Ok(v) = serde_json::from_value(value) { self.forked_from = v; }
                }
                "forked_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.forked_at = v; }
                }
                "forked_by" => {
                    if let Ok(v) = serde_json::from_value(value) { self.forked_by = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Page {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Page"
    }
}

impl backbone_core::PersistentEntity for Page {
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

impl backbone_orm::EntityRepoMeta for Page {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("website_id".to_string(), "uuid".to_string());
        m.insert("visibility".to_string(), "website_visibility".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["key", "url", "title"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("website", "websites", "websiteId")]
    }
}

/// Builder for Page entity
///
/// Provides a fluent API for constructing Page instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct PageBuilder {
    key: Option<String>,
    website_id: Option<Uuid>,
    url: Option<String>,
    title: Option<String>,
    seo_name: Option<String>,
    is_published: Option<bool>,
    date_publish: Option<DateTime<Utc>>,
    website_indexed: Option<bool>,
    visibility: Option<WebsiteVisibility>,
    required_member_roles: Option<Vec<String>>,
    forked_from: Option<Uuid>,
    forked_at: Option<DateTime<Utc>>,
    forked_by: Option<Uuid>,
}

impl PageBuilder {
    /// Set the key field (required)
    pub fn key(mut self, value: String) -> Self {
        self.key = Some(value);
        self
    }

    /// Set the website_id field (optional)
    pub fn website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the url field (required)
    pub fn url(mut self, value: String) -> Self {
        self.url = Some(value);
        self
    }

    /// Set the title field (required)
    pub fn title(mut self, value: String) -> Self {
        self.title = Some(value);
        self
    }

    /// Set the seo_name field (optional)
    pub fn seo_name(mut self, value: String) -> Self {
        self.seo_name = Some(value);
        self
    }

    /// Set the is_published field (default: `false`)
    pub fn is_published(mut self, value: bool) -> Self {
        self.is_published = Some(value);
        self
    }

    /// Set the date_publish field (optional)
    pub fn date_publish(mut self, value: DateTime<Utc>) -> Self {
        self.date_publish = Some(value);
        self
    }

    /// Set the website_indexed field (default: `true`)
    pub fn website_indexed(mut self, value: bool) -> Self {
        self.website_indexed = Some(value);
        self
    }

    /// Set the visibility field (default: `WebsiteVisibility::default()`)
    pub fn visibility(mut self, value: WebsiteVisibility) -> Self {
        self.visibility = Some(value);
        self
    }

    /// Set the required_member_roles field (default: `serde_json::json!({})`)
    pub fn required_member_roles(mut self, value: Vec<String>) -> Self {
        self.required_member_roles = Some(value);
        self
    }

    /// Set the forked_from field (optional)
    pub fn forked_from(mut self, value: Uuid) -> Self {
        self.forked_from = Some(value);
        self
    }

    /// Set the forked_at field (optional)
    pub fn forked_at(mut self, value: DateTime<Utc>) -> Self {
        self.forked_at = Some(value);
        self
    }

    /// Set the forked_by field (optional)
    pub fn forked_by(mut self, value: Uuid) -> Self {
        self.forked_by = Some(value);
        self
    }

    /// Build the Page entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Page, String> {
        let key = self.key.ok_or_else(|| "key is required".to_string())?;
        let url = self.url.ok_or_else(|| "url is required".to_string())?;
        let title = self.title.ok_or_else(|| "title is required".to_string())?;

        Ok(Page {
            id: Uuid::new_v4(),
            key,
            website_id: self.website_id,
            url,
            title,
            seo_name: self.seo_name,
            is_published: self.is_published.unwrap_or(false),
            date_publish: self.date_publish,
            website_indexed: self.website_indexed.unwrap_or(true),
            visibility: self.visibility.unwrap_or_default(),
            required_member_roles: self.required_member_roles.unwrap_or_default(),
            forked_from: self.forked_from,
            forked_at: self.forked_at,
            forked_by: self.forked_by,
            metadata: AuditMetadata::default(),
        })
    }
}
