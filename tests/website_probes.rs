//! The fail-hard probe suite (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! Every probe runs on its own DISPOSABLE scratch database on the
//! local scratch Postgres (127.0.0.1:5433 — NEVER the live dev
//! database on 5432). A probe that cannot reach the scratch server
//! PANICS — a skipped probe is a failed probe.

mod probes;
