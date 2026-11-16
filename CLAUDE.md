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

<!-- rust-rules:start (one identical copy in every Rust repo; full text: metaphora docs/handbook/rust-rules.md) -->
## Rust rules (NS01 to NS12)

Every Rust change in this repo follows the twelve Rust rules. The full text, with the reason and
the check for each rule, is the metaphora handbook page
[`docs/handbook/rust-rules.md`](https://github.com/faridlab/metaphora/blob/main/docs/handbook/rust-rules.md).
Cite the code (for example `NS04`) in reviews and commits.

- **NS01 Types carry the meaning:** newtypes and enums for ids, money and states; parse input once, at the boundary.
- **NS02 Never panic on input:** no `unwrap`, `expect`, indexing or unchecked arithmetic on request, file, env, row or message data; return a typed error.
- **NS03 Errors are typed, never swallowed:** `thiserror` in libraries, `anyhow` only in binaries; no `let _ =`, `.ok()` or `unwrap_or_default()` that hides a failure.
- **NS04 Ownership is designed, not appeased:** never add `clone`, `to_vec`, `to_owned`, `Rc`/`Arc`, `RefCell`/`Cell`, `Box`, indices, a split, a lifetime or `unsafe` only to make it compile. `Arc` for state shared across tasks is design, not appeasement.
- **NS05 Unsafe stays in one place:** `#![forbid(unsafe_code)]` by default; any exception lives in one module with `// SAFETY:` on every block.
- **NS06 Pure core, effects at the edge:** domain logic does no database, network, clock or env access; the edge passes those in.
- **NS07 Structure:** one concept per module, private or `pub(crate)` by default, dependencies point inward, generated code and hand code (`*.ext.rs`) kept apart.
- **NS08 Comments and naming:** domain names, comments say why, every public item documented, no session jargon.
- **NS09 Reuse before writing:** search std, current dependencies and the framework first; promote shared code upstream, never copy it.
- **NS10 Dependencies:** each new crate justified, unused default features off, `cargo deny` clean.
- **NS11 Proof, tests, speed:** a fix starts with a test that fails on the old code; a speed claim comes with a measurement.
- **NS12 The CI gate:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -W clippy::pedantic`, `cargo test` and `cargo deny check` all pass; every `#[allow]` says why. Adopted repo by repo: until this repo runs it, reviewers enforce it by hand.
<!-- rust-rules:end -->
