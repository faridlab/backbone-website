use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::WebsitePageBlockKind;

/// Strongly-typed ID for PageBlock
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageBlockId(pub Uuid);

impl PageBlockId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for PageBlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PageBlockId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for PageBlockId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<PageBlockId> for Uuid {
    fn from(id: PageBlockId) -> Self { id.0 }
}

impl AsRef<Uuid> for PageBlockId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for PageBlockId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PageBlock {
    pub id: Uuid,
    pub page_id: Uuid,
    pub kind: WebsitePageBlockKind,
    pub position: i32,
    pub payload: serde_json::Value,
}

impl PageBlock {
    /// Create a builder for PageBlock
    pub fn builder() -> PageBlockBuilder {
        <PageBlockBuilder as Default>::default()
    }

    /// Create a new PageBlock with required fields
    pub fn new(page_id: Uuid, kind: WebsitePageBlockKind, position: i32, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            page_id,
            kind,
            position,
            payload,
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> PageBlockId {
        PageBlockId(self.id)
    }


    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "page_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.page_id = v; }
                }
                "kind" => {
                    if let Ok(v) = serde_json::from_value(value) { self.kind = v; }
                }
                "position" => {
                    if let Ok(v) = serde_json::from_value(value) { self.position = v; }
                }
                "payload" => {
                    if let Ok(v) = serde_json::from_value(value) { self.payload = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for PageBlock {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "PageBlock"
    }
}

impl backbone_core::PersistentEntity for PageBlock {
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

impl backbone_orm::EntityRepoMeta for PageBlock {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("page_id".to_string(), "uuid".to_string());
        m.insert("kind".to_string(), "website_page_block_kind".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("page", "pages", "pageId")]
    }
}

/// Builder for PageBlock entity
///
/// Provides a fluent API for constructing PageBlock instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct PageBlockBuilder {
    page_id: Option<Uuid>,
    kind: Option<WebsitePageBlockKind>,
    position: Option<i32>,
    payload: Option<serde_json::Value>,
}

impl PageBlockBuilder {
    /// Set the page_id field (required)
    pub fn page_id(mut self, value: Uuid) -> Self {
        self.page_id = Some(value);
        self
    }

    /// Set the kind field (required)
    pub fn kind(mut self, value: WebsitePageBlockKind) -> Self {
        self.kind = Some(value);
        self
    }

    /// Set the position field (required)
    pub fn position(mut self, value: i32) -> Self {
        self.position = Some(value);
        self
    }

    /// Set the payload field (required)
    pub fn payload(mut self, value: serde_json::Value) -> Self {
        self.payload = Some(value);
        self
    }

    /// Build the PageBlock entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<PageBlock, String> {
        let page_id = self.page_id.ok_or_else(|| "page_id is required".to_string())?;
        let kind = self.kind.ok_or_else(|| "kind is required".to_string())?;
        let position = self.position.ok_or_else(|| "position is required".to_string())?;
        let payload = self.payload.ok_or_else(|| "payload is required".to_string())?;

        Ok(PageBlock {
            id: Uuid::new_v4(),
            page_id,
            kind,
            position,
            payload,
        })
    }
}
