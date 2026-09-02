# backbone-website

> Type: **module** — the headless website engine (schema `website`):
> pages/menus/publish, multi-website specificity (ONE resolver + real
> unique(key, website_id)), explicit COW/COU versioning verbs, GDPR
> visitors, turnstile-gated intake verbs, SEO/redirect layer, and the
> declared public read allowlist. Rendering is the webapps'; server-side
> template rendering does not exist.

## Rules
- Schema/model YAML is the source of truth; hand files are user_owned in
  metaphor.codegen.yaml — declare before landing.
- NEVER run cargo from the metaphora root; `cd` into this module first.
- Clippy bar: `cargo clippy --all-targets -- -D clippy::expect_used` EXIT=0.
- Tests: scratch Postgres 127.0.0.1:5433 (WEBSITE_TEST_ADMIN_URL override);
  a missing scratch server panics the suite. NEVER the live 5432.
- The generic-vs-specific fold lives ONLY in
  src/application/service/specificity.rs (grep gate).
- Sole module dependency: backbone-portal v0.2.0. Adding any other module
  edge reopens a frozen contract — see docs/spec.md §Identity.
- The contract of record: docs/spec.md (frozen decisions) + SPEC.md
  (orientation summary). Read both before changing behavior.
- **MUST** read and follow the target repo's own `CLAUDE.md` when working
  across repos — before editing in another repo, read its rules; the more
  local `CLAUDE.md` always wins.
