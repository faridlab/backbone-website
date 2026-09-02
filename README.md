# backbone-website

The headless website engine: multi-website page/menu/redirect tables
with a generic-vs-specific fold, an explicit copy-on-write /
copy-on-unlink pair, a GDPR-shaped visitor tracker, declared intake
verbs, and the SEO routing layer. Schema name: `website` (routes mount
under `/api/v1/website`).

## What this module is — and is not

- **Is**: a domain module in the backbone family. Content TABLES live
  here (`website.websites`, `pages`, `page_blocks`, `menus`,
  `menu_blocks`, `redirects`, `website_members`, `visitors`,
  `visitor_tracks`, `contact_messages`, `website_audit_log`); the
  rendering/frontend platform is fenced (no themes, no asset bundles,
  no html editor — the webapp owns all of that).
- **Is not**: a consuming service, and it does not SELF-MOUNT or
  SELF-GATE. It exports two pure axum routers — `website_public_routes`
  (the §7 allowlist, mounted BARE of `company_auth`) and
  `website_admin_routes` (mounted behind the host's `company_auth` with
  `ModuleWriteGate::new(pool, "website")` as the innermost
  `route_layer`). Nothing listens on its own.
- **Dependencies**: `backbone-framework` (core/orm/auth/messaging) at
  tag `v2.7.11`, plus exactly one module edge: `backbone-portal` at tag
  `v0.2.0` (`PortalUserId` in the principal port — the edge is
  load-bearing at compile time).

## The load-bearing shapes

- **ONE specificity resolver** (`src/application/service/specificity.rs`):
  every generic-vs-specific fold goes through it; the `website_id IS
  NULL` arm is written nowhere else (a probe walks `src/` and fails the
  suite on a violation), and the COALESCE-sentinel partial uniques in
  the hand-owned hardening migration make duplicates impossible at the
  DB.
- **COW/COU are named verbs**, never ambient: `fork_to_website`
  (idempotent, concurrency-safe — two simultaneous forks land exactly
  one row and both callers receive the same id) and
  `delete_generic_with_fanout` (all-or-nothing).
- **Publish contract**: `is_published` moves only through
  `publish`/`unpublish`; a PATCH carrying it refuses the typed
  `422 website_field_not_patchable` and is audited. `date_publish` is
  read-time-lazy; visibility is derived per read.
- **Visitors (GDPR shape)**: identity is a keyed HMAC-SHA256 digest
  (`WEBSITE_VISITOR_PEPPER`, rotatable); the raw IP is never persisted
  (probe-asserted over whole rows); tokens are random 32-byte
  base64url values that stay random; a 60-day partnerless GC sweeps in
  batches (`23 3 * * *`).
- **Intake verbs are DECLARED**: an `IntakeDeclaration` trait with a
  typed `deny_unknown_fields` payload, turnstile fail-closed with four
  typed answers (unset ≠ misconfigured ≠ refused ≠ unreachable),
  per-identity and per-IP fixed-window limits, a per-verb SAVEPOINT,
  and no sudo anywhere. The reference declaration is `contact`.
- **The public read allowlist is exhaustive** (seven route groups,
  grep-asserted): hostname-header binding with no fallback, published
  pages, visible menus, fresh cursor-paginated sitemap (45k/page),
  verbatim robots, the 9-case routing matcher, the visitor heartbeat,
  and declared intake. Everything else 404s.

## The contract of record

`docs/spec.md` is the frozen design; `SPEC.md` is the durable module
contract a host or downstream module can hold this crate to. The probe
suite under `tests/` is fail-hard: every probe mints a disposable
scratch database on the local scratch Postgres (127.0.0.1:5433) — a
probe that cannot reach it panics, and a vacuous skip is a failure.
