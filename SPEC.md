# backbone-website — the module contract

The durable record of what this module promises and why it is shaped
the way it is. The orientation lives in [README.md](README.md); the
frozen design with the full decision citations is
[docs/spec.md](docs/spec.md). This document is what a host or a
downstream module (blog, livechat, website_event, storefront) can hold
the crate to.

## Identity and dependency posture

- Schema `website`; the host nests the exported routers at
  `/api/v1/website`. The module mounts nothing and gates nothing
  itself: the public router is composed BARE of `company_auth`; the
  admin router is mounted behind the host's `company_auth` with
  `ModuleWriteGate::new(pool, "website")` as the innermost
  `route_layer` (authority names `write:website`, `delete:website`,
  and their supersets resolve through the host gate).
- Dependency edges: `backbone-framework` at tag `v2.7.11` (one rev, no
  mixing), plus exactly ONE module edge — `backbone-portal` at tag
  `v0.2.0` for `PortalUserId` in the principal port. Cross-schema
  principal references are LOGICAL (no FK across module boundaries);
  the bootstrap verb's ghost-principal mint writes a portal row
  directly (portal v0.2.0 exports no mint verb).
- No RLS posture, no company fence (ADR-0014 posture 4): the security
  axis is the verb surface. No outbox entry: this module emits no
  integration events.

## The frozen contracts (probe-asserted)

1. **ONE resolver** — the generic-vs-specific fold exists only in
   `src/application/service/specificity.rs`; the `website_id IS NULL`
   arm and the `fn resolve_*` family are defined nowhere else (the
   sole sanctioned `resolve_` name outside it is the exported trait's
   frozen `resolve_website_by_host`, declaration + delegation, in
   `website_surface.rs`). DB fences: partial uniques on
   `(key, COALESCE(website_id, nil))`, `(url, COALESCE(website_id, nil))`,
   live domain, live public principal, live redirect path.
2. **COW/COU verbs** — `fork_to_website` (idempotent; concurrent forks
   land exactly one specific row and every caller receives the same
   id; blocks copied by the winner only; menus re-pointed once;
   provenance columns stamped) and `delete_generic_with_fanout`
   (force-forks every lacking website, re-points menus, soft-deletes
   the generic — all-or-nothing).
3. **Publish** — `is_published`/`date_publish` are excluded from every
   patch whitelist (typed 422 + audit `publish_refused`); the verbs
   are the only writers; visibility is derived per read: resolver pick
   AND published AND date-past AND tier. Tier ladder:
   public / connected / restricted (no password tier — fenced).
4. **Visitors** — digest = hex HMAC-SHA256(pepper,
   "website-visitor-v1\n"+ip+"\n"+ua+"\n"+session), digest excludes
   website_id, rows per website, `UNIQUE(digest, website_id)` upsert
   target; raw IP never persisted anywhere (whole-row probe); tokens
   are random 32-byte base64url (43 chars) and never upgrade on login
   merge (claim-in-place or reparent); GC hard-deletes partnerless
   rows past the retention horizon in batches.
5. **Intake** — `IntakeDeclaration` in Rust, never a model-name route;
   turnstile fail-closed with the four typed answers apart (the
   config-selected recaptcha sibling answers the same four under the
   same codes — §6.3); per-identity
   AND per-IP fixed windows with `Retry-After`; `SAVEPOINT
   intake_<name>` around every persist (a refusal leaves zero rows);
   no sudo; no anonymous mail — the notifier port is host-installed,
   unwired means WARN + `notified=false`, and the write never depends
   on the port.
6. **Public allowlist** — hostname-header binding (normalized, exact
   match, NO fallback; miss = `404 website_not_resolved`); exactly the
   seven route groups of README §load-bearing; everything else 404s;
   the principal port is fail-closed (unwired = every non-public tier
   403).
7. **SEO/routing** — the ONE slug utility `slug_from` (seo_name →
   kebab fallback → "page"; `-{id}` suffix with `abs()` on the int
   arm; max 120); the ordered 9-case matcher table (bot never
   redirects, POST never bounces, trailing-slash 301, `//` collapse,
   default-lang 301, lang-alias 301 dormant on the empty registry,
   redirect-table reroute, canonical-301, terminal); redirects
   validate `alias_308` parameter-name parity; one answer per path
   per website.

## Config knobs (all string-typed; the host declares them)

- `WEBSITE_VISITOR_PEPPER` — required at first visitor verb (unset =
  typed loud refusal, never a zero-secret fallback).
- `WEBSITE_TURNSTILE_SECRET` — unset ≠ misconfigured (two different
  typed 503s).
- `WEBSITE_TURNSTILE_VERIFY_URL` — default the Cloudflare siteverify
  endpoint; probe-overridable.
- `WEBSITE_RECAPTCHA_SECRET` / `WEBSITE_RECAPTCHA_VERIFY_URL` — the
  Google reCAPTCHA sibling of the turnstile pair (§6.3): same fail-closed
  four-verdict contract, unset ≠ misconfigured, probe-overridable default
  `https://www.google.com/recaptcha/api/siteverify`.
- `WEBSITE_CAPTCHA_PROVIDER` — `turnstile` (default) or `recaptcha`;
  any other value → typed 503 `website_captcha_provider_unknown`,
  never a silent fallback.
- `WEBSITE_VISITOR_RETENTION_DAYS` (default 60),
  `WEBSITE_VISITOR_GC_BATCH` (default 1000).

## Fences (never built here)

Rendering/frontend platform: themes, asset bundles, the html
editor/builder, controller/technical pages, snippet filters, the
password visibility tier, translation machinery, custom code
injection, media tables, per-website signup policy columns (contract
declared, columns deferred), and the language registry (the matcher's
alias row ships dormant). The full register with citations is
[docs/spec.md](docs/spec.md) §13.
