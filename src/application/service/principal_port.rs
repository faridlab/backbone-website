//! The principal port (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! FAIL-CLOSED, slot-shaped (the portal `credential_port` precedent):
//! the module NEVER verifies bearer tokens itself — the host installs
//! an adapter over backbone-portal's verification surface. Unwired,
//! every non-public tier on the public tree reads 403 and the admin
//! tree needs no portal principal at all (officers authenticate through
//! the host's company_auth).

use backbone_portal::exports::PortalUserId;

/// The verified portal principal, as the host's adapter reports it.
#[derive(Debug, Clone)]
pub struct WebsitePrincipal {
    pub user_id: PortalUserId,
    pub email: String,
}

impl WebsitePrincipal {
    /// The underlying portal-user row id (the logical-ref key every
    /// principal column in this schema carries).
    pub fn user_uuid(&self) -> uuid::Uuid {
        self.user_id.into()
    }
}

/// Bearer-token verification port. The host installs the adapter; the
/// module ships only the refusing default.
pub trait WebsitePrincipalVerifier: Send + Sync {
    /// Returns the verified portal principal for a presented bearer
    /// token, or `None` (unverified, refused, or malformed — the
    /// module does not distinguish; absence is a closed door).
    fn verify<'a>(
        &'a self,
        token: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<WebsitePrincipal>> + Send + 'a>>;
}

/// The refusing default: no bearer ever verifies. Installed until the
/// host wires an adapter — non-public tiers read 403, never fail-open.
#[derive(Debug, Default, Clone, Copy)]
pub struct RefusingPrincipalVerifier;

impl WebsitePrincipalVerifier for RefusingPrincipalVerifier {
    fn verify<'a>(
        &'a self,
        _token: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<WebsitePrincipal>> + Send + 'a>>
    {
        Box::pin(async { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unwired_port_refuses_every_bearer() {
        let port = RefusingPrincipalVerifier;
        assert!(port.verify("any-token").await.is_none());
        assert!(port.verify("").await.is_none());
    }
}
