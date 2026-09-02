use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "website_audit_event", rename_all = "snake_case")]
pub enum WebsiteAuditEvent {
    WebsiteCreated,
    PageCreated,
    PageUpdated,
    PagePublished,
    PageUnpublished,
    PageForked,
    GenericDeletedWithFanout,
    PageRenamed,
    MenuCreated,
    MenuUpdated,
    MenuDeleted,
    MenuFanout,
    RedirectCreated,
    RedirectUpdated,
    RedirectDeleted,
    VisitorMerged,
    VisitorGcSwept,
    IntakeReceived,
    IntakeRefused,
    PublishRefused,
}

impl std::fmt::Display for WebsiteAuditEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WebsiteCreated => write!(f, "website_created"),
            Self::PageCreated => write!(f, "page_created"),
            Self::PageUpdated => write!(f, "page_updated"),
            Self::PagePublished => write!(f, "page_published"),
            Self::PageUnpublished => write!(f, "page_unpublished"),
            Self::PageForked => write!(f, "page_forked"),
            Self::GenericDeletedWithFanout => write!(f, "generic_deleted_with_fanout"),
            Self::PageRenamed => write!(f, "page_renamed"),
            Self::MenuCreated => write!(f, "menu_created"),
            Self::MenuUpdated => write!(f, "menu_updated"),
            Self::MenuDeleted => write!(f, "menu_deleted"),
            Self::MenuFanout => write!(f, "menu_fanout"),
            Self::RedirectCreated => write!(f, "redirect_created"),
            Self::RedirectUpdated => write!(f, "redirect_updated"),
            Self::RedirectDeleted => write!(f, "redirect_deleted"),
            Self::VisitorMerged => write!(f, "visitor_merged"),
            Self::VisitorGcSwept => write!(f, "visitor_gc_swept"),
            Self::IntakeReceived => write!(f, "intake_received"),
            Self::IntakeRefused => write!(f, "intake_refused"),
            Self::PublishRefused => write!(f, "publish_refused"),
        }
    }
}

impl FromStr for WebsiteAuditEvent {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "website_created" => Ok(Self::WebsiteCreated),
            "page_created" => Ok(Self::PageCreated),
            "page_updated" => Ok(Self::PageUpdated),
            "page_published" => Ok(Self::PagePublished),
            "page_unpublished" => Ok(Self::PageUnpublished),
            "page_forked" => Ok(Self::PageForked),
            "generic_deleted_with_fanout" => Ok(Self::GenericDeletedWithFanout),
            "page_renamed" => Ok(Self::PageRenamed),
            "menu_created" => Ok(Self::MenuCreated),
            "menu_updated" => Ok(Self::MenuUpdated),
            "menu_deleted" => Ok(Self::MenuDeleted),
            "menu_fanout" => Ok(Self::MenuFanout),
            "redirect_created" => Ok(Self::RedirectCreated),
            "redirect_updated" => Ok(Self::RedirectUpdated),
            "redirect_deleted" => Ok(Self::RedirectDeleted),
            "visitor_merged" => Ok(Self::VisitorMerged),
            "visitor_gc_swept" => Ok(Self::VisitorGcSwept),
            "intake_received" => Ok(Self::IntakeReceived),
            "intake_refused" => Ok(Self::IntakeRefused),
            "publish_refused" => Ok(Self::PublishRefused),
            _ => Err(format!("Unknown WebsiteAuditEvent variant: {}", s)),
        }
    }
}

impl Default for WebsiteAuditEvent {
    fn default() -> Self {
        Self::WebsiteCreated
    }
}
