use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "website_visibility", rename_all = "snake_case")]
pub enum WebsiteVisibility {
    Public,
    Connected,
    Restricted,
}

impl std::fmt::Display for WebsiteVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::Connected => write!(f, "connected"),
            Self::Restricted => write!(f, "restricted"),
        }
    }
}

impl FromStr for WebsiteVisibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "public" => Ok(Self::Public),
            "connected" => Ok(Self::Connected),
            "restricted" => Ok(Self::Restricted),
            _ => Err(format!("Unknown WebsiteVisibility variant: {}", s)),
        }
    }
}

impl Default for WebsiteVisibility {
    fn default() -> Self {
        Self::Public
    }
}
