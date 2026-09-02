use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
// Per-variant renames, not `rename_all = "snake_case"`: the generator's
// snake_case mapping of `Moved301` yields `moved301`, which cannot
// round-trip the schema label `moved_301` (digits take no underscore
// boundary in that mapping). The explicit labels keep the DB enum and
// the derived decoder in agreement.
#[sqlx(type_name = "website_redirect_type")]
pub enum WebsiteRedirectType {
    #[sqlx(rename = "moved_301")]
    #[serde(rename = "moved_301")]
    Moved301,
    #[sqlx(rename = "found_302")]
    #[serde(rename = "found_302")]
    Found302,
    #[sqlx(rename = "alias_308")]
    #[serde(rename = "alias_308")]
    Alias308,
    #[sqlx(rename = "gone_404")]
    #[serde(rename = "gone_404")]
    Gone404,
}

impl std::fmt::Display for WebsiteRedirectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Moved301 => write!(f, "moved_301"),
            Self::Found302 => write!(f, "found_302"),
            Self::Alias308 => write!(f, "alias_308"),
            Self::Gone404 => write!(f, "gone_404"),
        }
    }
}

impl FromStr for WebsiteRedirectType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "moved_301" => Ok(Self::Moved301),
            "found_302" => Ok(Self::Found302),
            "alias_308" => Ok(Self::Alias308),
            "gone_404" => Ok(Self::Gone404),
            _ => Err(format!("Unknown WebsiteRedirectType variant: {}", s)),
        }
    }
}

impl Default for WebsiteRedirectType {
    fn default() -> Self {
        Self::Found302
    }
}
