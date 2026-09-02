use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for Website
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WebsiteId(pub Uuid);

impl WebsiteId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for WebsiteId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WebsiteId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for WebsiteId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<WebsiteId> for Uuid {
    fn from(id: WebsiteId) -> Self { id.0 }
}

impl AsRef<Uuid> for WebsiteId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for WebsiteId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Website {
    pub id: Uuid,
    pub name: String,
    pub domain: Option<String>,
    pub company_id: Uuid,
    pub public_user_id: Uuid,
    pub default_lang_code: String,
    pub homepage_url: String,
    pub robots_txt: Option<String>,
    pub social_links: Option<serde_json::Value>,
    pub contact_recipients: Vec<String>,
    pub sequence: i32,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Website {
    /// Create a builder for Website
    pub fn builder() -> WebsiteBuilder {
        <WebsiteBuilder as Default>::default()
    }

    /// Create a new Website with required fields
    pub fn new(name: String, company_id: Uuid, public_user_id: Uuid, default_lang_code: String, homepage_url: String, contact_recipients: Vec<String>, sequence: i32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            domain: None,
            company_id,
            public_user_id,
            default_lang_code,
            homepage_url,
            robots_txt: None,
            social_links: None,
            contact_recipients,
            sequence,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> WebsiteId {
        WebsiteId(self.id)
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

    /// Set the domain field (chainable)
    pub fn with_domain(mut self, value: String) -> Self {
        self.domain = Some(value);
        self
    }

    /// Set the robots_txt field (chainable)
    pub fn with_robots_txt(mut self, value: String) -> Self {
        self.robots_txt = Some(value);
        self
    }

    /// Set the social_links field (chainable)
    pub fn with_social_links(mut self, value: serde_json::Value) -> Self {
        self.social_links = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "domain" => {
                    if let Ok(v) = serde_json::from_value(value) { self.domain = v; }
                }
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "public_user_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.public_user_id = v; }
                }
                "default_lang_code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.default_lang_code = v; }
                }
                "homepage_url" => {
                    if let Ok(v) = serde_json::from_value(value) { self.homepage_url = v; }
                }
                "robots_txt" => {
                    if let Ok(v) = serde_json::from_value(value) { self.robots_txt = v; }
                }
                "social_links" => {
                    if let Ok(v) = serde_json::from_value(value) { self.social_links = v; }
                }
                "contact_recipients" => {
                    if let Ok(v) = serde_json::from_value(value) { self.contact_recipients = v; }
                }
                "sequence" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sequence = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Website {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Website"
    }
}

impl backbone_core::PersistentEntity for Website {
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

impl backbone_orm::EntityRepoMeta for Website {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("public_user_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["name", "default_lang_code", "homepage_url"]
    }
}

/// Builder for Website entity
///
/// Provides a fluent API for constructing Website instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct WebsiteBuilder {
    name: Option<String>,
    domain: Option<String>,
    company_id: Option<Uuid>,
    public_user_id: Option<Uuid>,
    default_lang_code: Option<String>,
    homepage_url: Option<String>,
    robots_txt: Option<String>,
    social_links: Option<serde_json::Value>,
    contact_recipients: Option<Vec<String>>,
    sequence: Option<i32>,
}

impl WebsiteBuilder {
    /// Set the name field (required)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the domain field (optional)
    pub fn domain(mut self, value: String) -> Self {
        self.domain = Some(value);
        self
    }

    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the public_user_id field (required)
    pub fn public_user_id(mut self, value: Uuid) -> Self {
        self.public_user_id = Some(value);
        self
    }

    /// Set the default_lang_code field (default: `"en".to_string()`)
    pub fn default_lang_code(mut self, value: String) -> Self {
        self.default_lang_code = Some(value);
        self
    }

    /// Set the homepage_url field (default: `"/".to_string()`)
    pub fn homepage_url(mut self, value: String) -> Self {
        self.homepage_url = Some(value);
        self
    }

    /// Set the robots_txt field (optional)
    pub fn robots_txt(mut self, value: String) -> Self {
        self.robots_txt = Some(value);
        self
    }

    /// Set the social_links field (optional)
    pub fn social_links(mut self, value: serde_json::Value) -> Self {
        self.social_links = Some(value);
        self
    }

    /// Set the contact_recipients field (default: `serde_json::json!({})`)
    pub fn contact_recipients(mut self, value: Vec<String>) -> Self {
        self.contact_recipients = Some(value);
        self
    }

    /// Set the sequence field (default: `10`)
    pub fn sequence(mut self, value: i32) -> Self {
        self.sequence = Some(value);
        self
    }

    /// Build the Website entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Website, String> {
        let name = self.name.ok_or_else(|| "name is required".to_string())?;
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let public_user_id = self.public_user_id.ok_or_else(|| "public_user_id is required".to_string())?;

        Ok(Website {
            id: Uuid::new_v4(),
            name,
            domain: self.domain,
            company_id,
            public_user_id,
            default_lang_code: self.default_lang_code.unwrap_or("en".to_string()),
            homepage_url: self.homepage_url.unwrap_or("/".to_string()),
            robots_txt: self.robots_txt,
            social_links: self.social_links,
            contact_recipients: self.contact_recipients.unwrap_or_default(),
            sequence: self.sequence.unwrap_or(10),
            metadata: AuditMetadata::default(),
        })
    }
}
