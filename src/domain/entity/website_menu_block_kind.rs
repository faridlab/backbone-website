use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "website_menu_block_kind", rename_all = "snake_case")]
pub enum WebsiteMenuBlockKind {
    Link,
    LinkGroup,
    Highlight,
}

impl std::fmt::Display for WebsiteMenuBlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link => write!(f, "link"),
            Self::LinkGroup => write!(f, "link_group"),
            Self::Highlight => write!(f, "highlight"),
        }
    }
}

impl FromStr for WebsiteMenuBlockKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "link" => Ok(Self::Link),
            "link_group" => Ok(Self::LinkGroup),
            "highlight" => Ok(Self::Highlight),
            _ => Err(format!("Unknown WebsiteMenuBlockKind variant: {}", s)),
        }
    }
}

impl Default for WebsiteMenuBlockKind {
    fn default() -> Self {
        Self::Link
    }
}
