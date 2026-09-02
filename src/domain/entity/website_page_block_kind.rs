use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "website_page_block_kind", rename_all = "snake_case")]
pub enum WebsitePageBlockKind {
    Heading,
    RichText,
    Image,
    Cta,
    Spacer,
}

impl std::fmt::Display for WebsitePageBlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Heading => write!(f, "heading"),
            Self::RichText => write!(f, "rich_text"),
            Self::Image => write!(f, "image"),
            Self::Cta => write!(f, "cta"),
            Self::Spacer => write!(f, "spacer"),
        }
    }
}

impl FromStr for WebsitePageBlockKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "heading" => Ok(Self::Heading),
            "rich_text" => Ok(Self::RichText),
            "image" => Ok(Self::Image),
            "cta" => Ok(Self::Cta),
            "spacer" => Ok(Self::Spacer),
            _ => Err(format!("Unknown WebsitePageBlockKind variant: {}", s)),
        }
    }
}

impl Default for WebsitePageBlockKind {
    fn default() -> Self {
        Self::Heading
    }
}
