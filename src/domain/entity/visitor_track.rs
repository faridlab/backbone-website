use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Strongly-typed ID for VisitorTrack
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VisitorTrackId(pub Uuid);

impl VisitorTrackId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for VisitorTrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for VisitorTrackId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for VisitorTrackId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<VisitorTrackId> for Uuid {
    fn from(id: VisitorTrackId) -> Self { id.0 }
}

impl AsRef<Uuid> for VisitorTrackId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for VisitorTrackId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VisitorTrack {
    pub id: Uuid,
    pub visitor_id: Uuid,
    pub page_id: Option<Uuid>,
    pub url: String,
    pub visited_at: DateTime<Utc>,
}

impl VisitorTrack {
    /// Create a builder for VisitorTrack
    pub fn builder() -> VisitorTrackBuilder {
        <VisitorTrackBuilder as Default>::default()
    }

    /// Create a new VisitorTrack with required fields
    pub fn new(visitor_id: Uuid, url: String, visited_at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            visitor_id,
            page_id: None,
            url,
            visited_at,
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> VisitorTrackId {
        VisitorTrackId(self.id)
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the page_id field (chainable)
    pub fn with_page_id(mut self, value: Uuid) -> Self {
        self.page_id = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "visitor_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.visitor_id = v; }
                }
                "page_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.page_id = v; }
                }
                "url" => {
                    if let Ok(v) = serde_json::from_value(value) { self.url = v; }
                }
                "visited_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.visited_at = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for VisitorTrack {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "VisitorTrack"
    }
}

impl backbone_core::PersistentEntity for VisitorTrack {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        let _ = ts;
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        let _ = ts;
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        let _ = ts;
    }
}

impl backbone_orm::EntityRepoMeta for VisitorTrack {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("visitor_id".to_string(), "uuid".to_string());
        m.insert("page_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["url"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("visitor", "visitors", "visitorId")]
    }
}

/// Builder for VisitorTrack entity
///
/// Provides a fluent API for constructing VisitorTrack instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct VisitorTrackBuilder {
    visitor_id: Option<Uuid>,
    page_id: Option<Uuid>,
    url: Option<String>,
    visited_at: Option<DateTime<Utc>>,
}

impl VisitorTrackBuilder {
    /// Set the visitor_id field (required)
    pub fn visitor_id(mut self, value: Uuid) -> Self {
        self.visitor_id = Some(value);
        self
    }

    /// Set the page_id field (optional)
    pub fn page_id(mut self, value: Uuid) -> Self {
        self.page_id = Some(value);
        self
    }

    /// Set the url field (required)
    pub fn url(mut self, value: String) -> Self {
        self.url = Some(value);
        self
    }

    /// Set the visited_at field (default: `Utc::now()`)
    pub fn visited_at(mut self, value: DateTime<Utc>) -> Self {
        self.visited_at = Some(value);
        self
    }

    /// Build the VisitorTrack entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<VisitorTrack, String> {
        let visitor_id = self.visitor_id.ok_or_else(|| "visitor_id is required".to_string())?;
        let url = self.url.ok_or_else(|| "url is required".to_string())?;

        Ok(VisitorTrack {
            id: Uuid::new_v4(),
            visitor_id,
            page_id: self.page_id,
            url,
            visited_at: self.visited_at.unwrap_or(Utc::now()),
        })
    }
}
