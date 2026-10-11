// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod captcha_recaptcha;
pub mod intake_contact;
pub mod intake_engine;
pub mod lang_matcher;
pub mod notifier_port;
pub mod principal_port;
pub mod slug;
pub mod specificity;
pub mod tour_service;
pub mod versioning_service;
pub mod visitor_gc;
pub mod website_error;
pub mod website_surface;
pub use captcha_recaptcha::{
    CaptchaProvider, CaptchaVerifier, RecaptchaClient, RecaptchaConfig,
    WEBSITE_CAPTCHA_PROVIDER_ENV, WEBSITE_RECAPTCHA_SECRET_ENV, WEBSITE_RECAPTCHA_VERIFY_URL_ENV,
};
pub use intake_contact::{ContactIntake, ContactMessageView};
pub use intake_engine::{
    IntakeEngine, IntakeOutcome, IntakeRatePolicy, TurnstileClient, TurnstileConfig,
};
pub use lang_matcher::{
    apply_matcher, RedirectAnswer as MatcherRedirectAnswer, MatchInput, ResolveAnswer,
};
pub use principal_port::{RefusingPrincipalVerifier, WebsitePrincipal, WebsitePrincipalVerifier};
pub use slug::{slug_from, SlugId};
pub use tour_service::{ConsumeOutcome, ConsumedTour, TourService, TourView, UpsertTourInput};
pub use specificity::{
    live_scope, prefer_specific, page_entity_by_id, resolve_page_by_url, resolve_sitemap_page,
    resolve_specific, Resolution, ResolvedPage, ScopeFragment,
};
pub use versioning_service::{delete_generic_with_fanout, fork_to_website, FanoutDeletion, ForkOutcome};
pub use visitor_gc::{sweep_partnerless_visitors, SweepSummary};
pub use website_error::{map_unique_violation, WebsiteError};
pub use website_surface::{PgWebsiteSurface, WebsiteSurface};
pub use notifier_port::{IntakeNotifier, UnwiredIntakeNotifier};
pub use website_service::{
    normalize_host, record_audit, ActorRef, WebsiteRootService, WebsiteView,
};
