use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "website_member_role", rename_all = "snake_case")]
pub enum WebsiteMemberRole {
    Member,
    Editor,
}

impl std::fmt::Display for WebsiteMemberRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Member => write!(f, "member"),
            Self::Editor => write!(f, "editor"),
        }
    }
}

impl FromStr for WebsiteMemberRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "member" => Ok(Self::Member),
            "editor" => Ok(Self::Editor),
            _ => Err(format!("Unknown WebsiteMemberRole variant: {}", s)),
        }
    }
}

impl Default for WebsiteMemberRole {
    fn default() -> Self {
        Self::Member
    }
}
