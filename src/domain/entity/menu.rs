use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::WebsiteVisibility;
use super::AuditMetadata;

/// Strongly-typed ID for Menu
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MenuId(pub Uuid);

impl MenuId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MenuId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MenuId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MenuId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MenuId> for Uuid {
    fn from(id: MenuId) -> Self { id.0 }
}

impl AsRef<Uuid> for MenuId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MenuId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Menu {
    pub id: Uuid,
    pub website_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub page_id: Option<Uuid>,
    pub url: Option<String>,
    pub new_window: bool,
    pub sequence: i32,
    pub visibility: WebsiteVisibility,
    pub required_member_roles: Vec<String>,
    pub is_mega_menu: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Menu {
    /// Create a builder for Menu
    pub fn builder() -> MenuBuilder {
        <MenuBuilder as Default>::default()
    }

    /// Create a new Menu with required fields
    pub fn new(website_id: Uuid, name: String, new_window: bool, sequence: i32, visibility: WebsiteVisibility, required_member_roles: Vec<String>, is_mega_menu: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            website_id,
            parent_id: None,
            name,
            page_id: None,
            url: None,
            new_window,
            sequence,
            visibility,
            required_member_roles,
            is_mega_menu,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MenuId {
        MenuId(self.id)
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

    /// Set the parent_id field (chainable)
    pub fn with_parent_id(mut self, value: Uuid) -> Self {
        self.parent_id = Some(value);
        self
    }

    /// Set the page_id field (chainable)
    pub fn with_page_id(mut self, value: Uuid) -> Self {
        self.page_id = Some(value);
        self
    }

    /// Set the url field (chainable)
    pub fn with_url(mut self, value: String) -> Self {
        self.url = Some(value);
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
                "parent_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.parent_id = v; }
                }
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "page_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.page_id = v; }
                }
                "url" => {
                    if let Ok(v) = serde_json::from_value(value) { self.url = v; }
                }
                "new_window" => {
                    if let Ok(v) = serde_json::from_value(value) { self.new_window = v; }
                }
                "sequence" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sequence = v; }
                }
                "visibility" => {
                    if let Ok(v) = serde_json::from_value(value) { self.visibility = v; }
                }
                "required_member_roles" => {
                    if let Ok(v) = serde_json::from_value(value) { self.required_member_roles = v; }
                }
                "is_mega_menu" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_mega_menu = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Menu {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Menu"
    }
}

impl backbone_core::PersistentEntity for Menu {
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

impl backbone_orm::EntityRepoMeta for Menu {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("website_id".to_string(), "uuid".to_string());
        m.insert("parent_id".to_string(), "uuid".to_string());
        m.insert("page_id".to_string(), "uuid".to_string());
        m.insert("visibility".to_string(), "website_visibility".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["name"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("website", "websites", "websiteId"), ("parent", "menus", "parentId")]
    }
}

/// Builder for Menu entity
///
/// Provides a fluent API for constructing Menu instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MenuBuilder {
    website_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    name: Option<String>,
    page_id: Option<Uuid>,
    url: Option<String>,
    new_window: Option<bool>,
    sequence: Option<i32>,
    visibility: Option<WebsiteVisibility>,
    required_member_roles: Option<Vec<String>>,
    is_mega_menu: Option<bool>,
}

impl MenuBuilder {
    /// Set the website_id field (required)
    pub fn website_id(mut self, value: Uuid) -> Self {
        self.website_id = Some(value);
        self
    }

    /// Set the parent_id field (optional)
    pub fn parent_id(mut self, value: Uuid) -> Self {
        self.parent_id = Some(value);
        self
    }

    /// Set the name field (required)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the page_id field (optional)
    pub fn page_id(mut self, value: Uuid) -> Self {
        self.page_id = Some(value);
        self
    }

    /// Set the url field (optional)
    pub fn url(mut self, value: String) -> Self {
        self.url = Some(value);
        self
    }

    /// Set the new_window field (default: `false`)
    pub fn new_window(mut self, value: bool) -> Self {
        self.new_window = Some(value);
        self
    }

    /// Set the sequence field (default: `10`)
    pub fn sequence(mut self, value: i32) -> Self {
        self.sequence = Some(value);
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

    /// Set the is_mega_menu field (default: `false`)
    pub fn is_mega_menu(mut self, value: bool) -> Self {
        self.is_mega_menu = Some(value);
        self
    }

    /// Build the Menu entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Menu, String> {
        let website_id = self.website_id.ok_or_else(|| "website_id is required".to_string())?;
        let name = self.name.ok_or_else(|| "name is required".to_string())?;

        Ok(Menu {
            id: Uuid::new_v4(),
            website_id,
            parent_id: self.parent_id,
            name,
            page_id: self.page_id,
            url: self.url,
            new_window: self.new_window.unwrap_or(false),
            sequence: self.sequence.unwrap_or(10),
            visibility: self.visibility.unwrap_or_default(),
            required_member_roles: self.required_member_roles.unwrap_or_default(),
            is_mega_menu: self.is_mega_menu.unwrap_or(false),
            metadata: AuditMetadata::default(),
        })
    }
}
