use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "website_visitor_kind", rename_all = "snake_case")]
pub enum WebsiteVisitorKind {
    Anonymous,
    Identified,
}

impl std::fmt::Display for WebsiteVisitorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anonymous => write!(f, "anonymous"),
            Self::Identified => write!(f, "identified"),
        }
    }
}

impl FromStr for WebsiteVisitorKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anonymous" => Ok(Self::Anonymous),
            "identified" => Ok(Self::Identified),
            _ => Err(format!("Unknown WebsiteVisitorKind variant: {}", s)),
        }
    }
}

impl Default for WebsiteVisitorKind {
    fn default() -> Self {
        Self::Anonymous
    }
}
