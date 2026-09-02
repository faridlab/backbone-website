//! The intake notification port (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! FIXED-RECIPIENT notification rides a host-installed port. The module
//! sends NOTHING to any submitter address, ever (no anonymous mail).
//! Unwired → one WARN at first use and the row lands with
//! `notified = false`; the port never refuses the write. There is no
//! mail-module dependency edge.

use tracing::warn;

use super::intake_contact::ContactMessageView;
use super::website_service::WebsiteView;

/// Host-installed notifier (e.g. bridging to the notification module).
#[async_trait::async_trait]
pub trait IntakeNotifier: Send + Sync {
    /// True when this adapter actually delivers (a host bridge);
    /// false for the unwired default, which only WARNs.
    fn wired(&self) -> bool {
        true
    }

    async fn notify_intake(&self, website: &WebsiteView, message: &ContactMessageView);
}

/// The unwired default: WARN once per call site instance, never refuse.
#[derive(Debug, Default)]
pub struct UnwiredIntakeNotifier {
    warned: std::sync::atomic::AtomicBool,
}

impl UnwiredIntakeNotifier {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl IntakeNotifier for UnwiredIntakeNotifier {
    fn wired(&self) -> bool {
        false
    }

    async fn notify_intake(&self, website: &WebsiteView, message: &ContactMessageView) {
        if !self.warned.swap(true, std::sync::atomic::Ordering::Relaxed) {
            warn!(
                website_id = %website.id,
                "intake notifier port is unwired — messages land with notified=false; \
                 install a host adapter to bridge officers' mailboxes"
            );
        }
        let _ = message; // nothing else to do unwired
    }
}
