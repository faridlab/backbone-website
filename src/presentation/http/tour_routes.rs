//! The tour principal tree (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The engine-facing half of the `web_tour` port: definitions read by
//! name, the caller's consumed set, and the idempotent consume verb.
//! Every route requires a VERIFIED portal principal (bearer token →
//! the host-installed [`WebsitePrincipalVerifier`] port — unwired,
//! every route reads the typed 401, fail-closed like every other
//! principal-gated surface in this module).
//!
//! The module DOES NOT SELF-MOUNT this tree: the host nests it where
//! its authenticated client surface lives (it is deliberately NOT part
//! of the frozen public read allowlist — nothing here answers
//! unauthenticated). Officer-side tour management (upsert / delete /
//! reset) lives on the admin tree instead.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;

use crate::application::service::principal_port::{
    RefusingPrincipalVerifier, WebsitePrincipal, WebsitePrincipalVerifier,
};
use crate::application::service::tour_service::TourService;
use crate::application::service::website_error::WebsiteError;
use crate::presentation::http::public_routes::website_error_response;

/// The principal tree's state: the tour service plus the fail-closed
/// principal port.
#[derive(Clone)]
pub struct TourPrincipalState {
    tours: Arc<TourService>,
    principal_port: Arc<dyn WebsitePrincipalVerifier>,
}

impl TourPrincipalState {
    /// Compose with the refusing default verifier (every route reads
    /// 401 until the host installs the real adapter).
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            tours: Arc::new(TourService::new(pool)),
            principal_port: Arc::new(RefusingPrincipalVerifier),
        }
    }

    /// The principal-port install seam (the host bridges portal's
    /// verification surface here).
    pub fn install_principal_verifier(&mut self, verifier: Arc<dyn WebsitePrincipalVerifier>) {
        self.principal_port = verifier;
    }

    async fn principal(&self, headers: &HeaderMap) -> Option<WebsitePrincipal> {
        let presented = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        self.principal_port.verify(presented).await
    }
}

/// The tour principal tree (`/tours…` — the host nests it under its
/// authenticated prefix).
pub fn tour_routes(state: TourPrincipalState) -> Router {
    Router::new()
        .route("/tours", get(list_tours))
        .route("/tours/consumed", get(consumed_set))
        .route("/tours/:name", get(tour_by_name))
        .route("/tours/:name/consume", axum::routing::post(consume))
        .with_state(state)
}

async fn require_principal(
    state: &TourPrincipalState,
    headers: &HeaderMap,
) -> Result<WebsitePrincipal, Response> {
    match state.principal(headers).await {
        Some(p) => Ok(p),
        None => Err(website_error_response(WebsiteError::TourPrincipalRequired)),
    }
}

async fn list_tours(
    State(state): State<TourPrincipalState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = require_principal(&state, &headers).await {
        return resp;
    }
    match state.tours.list_tours().await {
        Ok(tours) => (axum::http::StatusCode::OK, Json(json!({ "tours": tours }))).into_response(),
        Err(e) => website_error_response(e),
    }
}

async fn tour_by_name(
    State(state): State<TourPrincipalState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    if let Err(resp) = require_principal(&state, &headers).await {
        return resp;
    }
    match state.tours.tour_by_name(&name).await {
        Ok(Some(tour)) => (axum::http::StatusCode::OK, Json(tour)).into_response(),
        Ok(None) => website_error_response(WebsiteError::TourNotFound),
        Err(e) => website_error_response(e),
    }
}

async fn consumed_set(
    State(state): State<TourPrincipalState>,
    headers: HeaderMap,
) -> Response {
    let principal = match require_principal(&state, &headers).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match state.tours.consumed_for(principal.user_uuid()).await {
        Ok(consumed) => (
            axum::http::StatusCode::OK,
            Json(json!({ "consumed": consumed })),
        )
            .into_response(),
        Err(e) => website_error_response(e),
    }
}

async fn consume(
    State(state): State<TourPrincipalState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let principal = match require_principal(&state, &headers).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match state.tours.consume_tour(principal.user_uuid(), &name).await {
        Ok(outcome) => (axum::http::StatusCode::OK, Json(outcome)).into_response(),
        Err(e) => website_error_response(e),
    }
}
