// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod admin_routes;
pub mod public_routes;
pub mod tour_routes;
pub use admin_routes::{website_admin_routes, WebsiteAdminState, WebsiteActor};
pub use public_routes::{website_public_routes, WebsitePublicState};
pub use tour_routes::{tour_routes, TourPrincipalState};
