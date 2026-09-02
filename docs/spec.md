# backbone-website — the module specification (headless website engine)

This document is the build contract for the `backbone-website` module: every
decision in it is frozen. A implementer executes this file without re-deciding
anything; where a choice was deliberately left to a later increment, the file
says so explicitly and names the increment's re-entry condition. The upstream
reference is the Odoo website cohort (`docs/odoo/website/website/`, register
IDs WS-1..WS-20); every deviation from the upstream behavior is recorded with
its reason.

Identity, in one line each:

- **Crate** `backbone-website`, version `0.1.0`, Postgres **schema `website`**,
  HTTP mount base **`/api/v1/website`** (verified unoccupied in the host tree;
  the module itself mounts nothing — the host nests the exported routers).
- **Headless**: backend semantics only (pages/menus/publish/specificity,
  visitors, SEO/redirect, guarded intake). The webapps own presentation.
  Server-side template rendering does not exist in any form.
- **Sole module dependency**: `backbone-portal` at git tag `v0.2.0`
  (`https://github.com/faridlab/backbone-portal`). Every portal principal
  reference in this module keys on `portal.portal_users(id)` (the portal-user
  ROW — portal v0.2.0 carries no website axis and its own global email fence
  must not be weakened, so the per-website pairing lives HERE, in schema
  `website`). No edge to any marketing-tree, sales-graph, or events module.
- **Company fence**: `none` declared (ADR-0014 posture 4) — no company column
  that gates reads, no RLS policy, ever. Visibility is service-layer policy in
  every read verb. Tenant isolation remains the only DB-level fence.
- **No durable events**: the module stages no outbox events and subscribes to
  no integration events. Consequently the host's `outbox_schemas` producer
  sets in BOTH `config/application.yml` and `config/application-prod.yml` get
  NO `website` entry — absence is the truthful posture, and adding one would
  arm a relay runner that can never drain anything.

Family conventions this module follows verbatim (verified against
backbone-portal v0.2.0 and backbone-foundation-ext v0.1.0):

- Schema/model YAML is the source of truth; every hand-written file is
  declared under `user_owned:` in `metaphor.codegen.yaml` **in the same change
  that lands it** (see §12).
- Migrations carry NO `GRANT`s (owner-role DDL). The composing host re-runs
  `apps/serpa-service/scripts/rls_app_role.sql` as owner after
  `metaphor migration run-all` — that script's blanket per-schema grants cover
  `website`.
- Enum types are created UNQUALIFIED in schema `public`, names prefixed
  `website_`, census-checked for collisions against every module migration in
  the tree before landing (the portal precedent).
- Cross-module references are LOGICAL: indexed uuid columns with
  `@exclude_from_foreign_key_check`, never a `FOREIGN KEY` constraint across
  schema boundaries (verified: no cross-schema FK exists anywhere in the
  module family; portal's own sapiens link is logical for the same reason —
  cross-module migration independence).
- The clippy bar for this module's code is a gate-time CLI flag:
  `cargo clippy --all-targets -- -D clippy::expect_used` → EXIT=0 (no
  `[lints]` table in Cargo.toml; portal ships none either).
- Probes are fail-hard: one disposable scratch Postgres per test
  (`localhost:5433`, user/password `postgres`/`postgres`, overridable via
  `WEBSITE_TEST_ADMIN_URL`), raw-SQL migration runner in filename order, and a
  missing scratch server PANICS the suite rather than skipping. The live dev
  database on 5432 is never touched by tests.

---

## 1. Table set (schema `website`)

Timestamps/actors on every mutable table ride the shared `Metadata` type
(`metadata jsonb`: `created_at`, `updated_at`, `deleted_at?`, `created_by?`,
`updated_by?`, `deleted_by?` — uuid actors are LOGICAL refs to sapiens users;
this module has no sapiens edge, so the actor columns are plain nullable uuids
with `@exclude_from_foreign_key_check`). The audit-trigger migration stamps
`created_at`/`updated_at` automatically per table (the portal
`add_audit_triggers` shape).

### 1.1 `website.websites` — the website root

| column | type | constraints / notes |
|---|---|---|
| `id` | uuid PK | `gen_random_uuid()` |
| `name` | text | NOT NULL, max 120 |
| `domain` | text? | nullable; stored normalized (trim, lowercase, no scheme, no trailing `/`, no `/./` or `/../` segments — urlparse-validated at the service layer). ONE live website may have NULL domain (see hardening index) |
| `company_id` | uuid | NOT NULL — LOGICAL ref `organization.Company.id` (the family convention: `@exclude_from_foreign_key_check`; the company table lives in the organization module) |
| `public_user_id` | uuid | NOT NULL — LOGICAL ref `portal.portal_users(id)`. THE per-website public principal, minted by the `create_website` bootstrap verb. This is the enumeration principal: what is publicly enumerable is exactly what this principal's declared read verbs return |
| `default_lang_code` | text | NOT NULL DEFAULT `'en'` (BCP-47 primary tag). Single-default-language activation only — no language registry exists (named gap, §13) |
| `homepage_url` | text | NOT NULL DEFAULT `'/'`; must start with `/` |
| `robots_txt` | text? | designer/officer-writable free text; served verbatim by the public robots verb (§7). Sanitization is the webapp's concern; the module stores and serves bytes |
| `social_links` | jsonb? | map of platform → absolute URL (string values validated server-side). Data-only: no module verb dereferences or redirects these |
| `contact_recipients` | text[] | NOT NULL DEFAULT `'{}'` — officer mailboxes for intake notifications (§6). NEVER a submitter-supplied address |
| `sequence` | int | NOT NULL DEFAULT 10 — drives the derived primary-website read (§1.13) |
| `metadata` | jsonb | shared audit block |

Indexes (hardening migration, §10): live-domain sentinel unique;
live-public-user unique.

### 1.2 `website.pages` — the content grain (WS-5, WS-20)

The Odoo `_inherits ir.ui.view` shape is fenced: the page row IS the content.
The (key, website_id) specificity grain lives on this row directly.

| column | type | constraints / notes |
|---|---|---|
| `id` | uuid PK | |
| `key` | text | NOT NULL, immutable after create (service-enforced), max 120, slug format (lowercase kebab-case). THE specificity key — fork matching keys on it |
| `website_id` | uuid? | nullable — NULL = the GENERIC row; a value = the website-specific row. Real intra-schema FK `website.websites(id)` ON DELETE CASCADE (FK columns may be NULL; the generic arm is simply unbound). The family logical-ref rule applies only ACROSS schemas |
| `url` | text | NOT NULL — routing path, normalized: starts `/`, no trailing `/` except the root `'/'` itself, no `//`, no dot-segments. Mutations go through the rename verb (§8) |
| `title` | text | NOT NULL max 200 |
| `seo_name` | text? | max 120, slug format — the slug utility's preferred arm (§8.1) |
| `is_published` | bool | NOT NULL DEFAULT false — stored publish state; writable ONLY through the publish/unpublish verbs (§3), excluded from every generic patch whitelist |
| `date_publish` | timestamptz? | scheduled publish, evaluated LAZILY at read time (`date_publish IS NULL OR date_publish <= now()`); no cron flips it, ever (the read_time_lazy posture, consistent with blog's declared posture) |
| `website_indexed` | bool | NOT NULL DEFAULT true — gates sitemap/search inclusion |
| `visibility` | `website_visibility` enum | NOT NULL DEFAULT `'public'` — `public` \| `connected` \| `restricted`. The upstream password tier and its session-unlock machinery are fenced (§13) |
| `required_member_roles` | text[] | NOT NULL DEFAULT `'{}'` — when `visibility = 'restricted'`, which membership roles (`member`, `editor`) may read. Empty + restricted = nobody but officers |
| `forked_from` | uuid? | provenance: the row this specific was forked from (the generic's id). Logical self-reference within the table (no constraint) |
| `forked_at` | timestamptz? | provenance stamp — when the fork landed |
| `forked_by` | uuid? | provenance stamp — acting officer (logical sapiens ref, §1 preamble) |
| `metadata` | jsonb | shared audit block |

FK rule for the whole schema, stated once: references that stay INSIDE
schema `website` are real FK constraints (`ON DELETE CASCADE` for child
tables and per-website rows — deleting a website removes its specifics,
menus, redirects, visitors, tracks, memberships, contact messages);
references that CROSS schemas (`portal.portal_users`,
`organization.Company`, sapiens actors) are LOGICAL — indexed uuid columns
with `@exclude_from_foreign_key_check`, never a DB constraint.

Indexes: `(website_id)`, `(key)`, `(url)`; plus the two load-bearing
partial uniques in the hardening migration (§1.14).

### 1.3 `website.page_blocks` — structured content (WS-5 replacement)

| column | type | notes |
|---|---|---|
| `id` | uuid PK | |
| `page_id` | uuid | NOT NULL, FK `website.pages(id)` ON DELETE CASCADE |
| `kind` | `website_page_block_kind` enum | `heading` \| `rich_text` \| `image` \| `cta` \| `spacer` — CLOSED vocabulary; widening it is a schema change, never a runtime option |
| `position` | int | NOT NULL — render order within the page |
| `payload` | jsonb | NOT NULL — validated per-kind by typed serde structs with `deny_unknown_fields` (heading: `{text, level?}`; rich_text: `{markdown}` — markdown, never trusted HTML; image: `{url, alt?}` — url points at webapp/object-storage-owned media, the module stores no media (§13 BTF-5); cta: `{label, url, style?}`; spacer: `{}` ) |

Index `(page_id, position)`. `fork_to_website` copies the block set (§4).

### 1.4 `website.menus` — the menu tree (WS-15, WS-19)

| column | type | notes |
|---|---|---|
| `id` | uuid PK | |
| `website_id` | uuid | NOT NULL — menus are ALWAYS per-website (the upstream NULL-website "transient fan-out source" is dead: fan-out is an explicit verb option, §4.4) |
| `parent_id` | uuid? | nullable FK `website.menus(id)` ON DELETE CASCADE (NULL = root). Depth ≤ 2 below root and mega-menu isolation (no parent, no children) are SERVICE validations with probes — no trigger machinery |
| `name` | text | NOT NULL max 120 |
| `page_id` | uuid? | LOGICAL ref `website.pages(id)` — when set, the menu's target is the page's url; re-pointed by the fork/rename verbs |
| `url` | text? | external or anchor target (`https://…`, `#anchor`); NULL when page-bound |
| `new_window` | bool | NOT NULL DEFAULT false |
| `sequence` | int | NOT NULL DEFAULT 10 — tree order is `(parent, sequence, id)` |
| `visibility` | `website_visibility` enum | NOT NULL DEFAULT `'public'` (same ladder as pages) |
| `required_member_roles` | text[] | NOT NULL DEFAULT `'{}'` — the WS-19 group rule ported as declared service-layer read policy |
| `is_mega_menu` | bool | NOT NULL DEFAULT false — explicit hand-set flag (the upstream compute-on-content-truthiness dies with stored HTML); validated: a mega-menu has no parent and no children |
| `metadata` | jsonb | shared audit block |

### 1.5 `website.menu_blocks` — mega-menu content

Same shape as `page_blocks`: `id`, `menu_id` FK `website.menus(id)` CASCADE,
`kind` (`website_menu_block_kind`: `link` \| `link_group` \| `highlight`),
`position`, `payload` jsonb (per-kind typed serde validation: link
`{label, url, page_id?}`, link_group `{label, children: [link…]}`,
highlight `{label, url, blurb?}`). No un-sanitized HTML is stored anywhere in
this module — the upstream `mega_menu_content` (sanitize=False) shape is
dead.

### 1.6 `website.redirects` — the SEO redirect table (WS-12)

| column | type | notes |
|---|---|---|
| `id` | uuid PK | |
| `website_id` | uuid | NOT NULL — per-website |
| `url_from` | text | NOT NULL — normalized like `pages.url` |
| `redirect_type` | `website_redirect_type` enum | NOT NULL DEFAULT `'found_302'` — `moved_301` \| `found_302` \| `alias_308` \| `gone_404` |
| `url_to` | text? | NOT NULL unless `redirect_type = 'gone_404'`. For `alias_308`, PARAM PARITY is validated at write time: `url_to` must carry the same query-parameter names as `url_from` (the upstream 308 parity rule, ported as rewrite-verb validation) |
| `description` | text? | operator note |
| `metadata` | jsonb | shared audit block |

Hardening: live-unique `(url_from, website_id)` — one redirect answer per
path per website; ambiguous stacked rules are impossible at the DB.

### 1.7 `website.visitors` — the GDPR visitor shape (WS-10)

| column | type | notes |
|---|---|---|
| `id` | uuid PK | |
| `website_id` | uuid | NOT NULL — visitor rows are PER-WEBSITE (scope decision §5.1) |
| `access_token` | text | NOT NULL UNIQUE — RANDOM 32-byte value, base64url (43 chars), minted at insert for BOTH kinds. The upstream partner-id-as-token enumeration oracle and the token-length-encodes-kind trick are dead |
| `kind` | `website_visitor_kind` enum | NOT NULL — `anonymous` \| `identified` (the kind column; no length encoding) |
| `digest` | text | NOT NULL — hex HMAC-SHA256 over IP+UA+session-id keyed by the config pepper (§5.2) |
| `digest_algo` | text | NOT NULL DEFAULT `'hmac-sha256-v1'` — versions the construction; a future algorithm change is a new value + migration decision, never an in-place reinterpretation |
| `portal_user_id` | uuid? | LOGICAL ref `portal.portal_users(id)` — set on the identified kind (login merge, §5.4) |
| `country_code` | text? | max 2 — the RESOLVED country only. The raw IP is NEVER persisted anywhere in this schema (no column, no log line, no metadata key; §6.6 records the same death for intake) |
| `visit_count` | int | NOT NULL DEFAULT 0 — bumped when the previous connection is older than the 8-hour window |
| `last_connection_at` | timestamptz | NOT NULL DEFAULT now() |
| `metadata` | jsonb | shared audit block |

UNIQUE `(access_token)` and UNIQUE `(digest, website_id)` are table-level
constraints (hard-delete model — see below). `is_connected` (active within 5
minutes) is DERIVED at read time, never stored. Soft-delete does not apply to
visitors or tracks: the GC verb hard-DELETEs, and a GDPR erasure verb
hard-DELETEs by principal; a soft-dead row would poison the digest upsert.

### 1.8 `website.visitor_tracks` — append-only page-view log

`id` uuid PK, `visitor_id` uuid NOT NULL FK `website.visitors(id)` CASCADE,
`page_id` uuid? LOGICAL ref `website.pages(id)` (nullable — tracks may record
non-page urls), `url` text NOT NULL, `visited_at` timestamptz NOT NULL.
Index `(visitor_id, visited_at)` and `(url)`. Rows are only ever INSERTed or
re-parented by the merge verb; no update path exists.

### 1.9 `website.website_members` — the per-website principal pairing (WS-13#4's home)

The upstream `unique(login, website_id)` on res.users does NOT move into
portal (portal v0.2.0's email fence is deliberately GLOBAL among live rows
and portal has no website axis — verified against the released tree). It
lives HERE as the membership pairing: the same portal principal (login =
email, globally unique in portal) may hold at most one membership per
website, and may appear on many websites — the upstream constraint's
intention (same login legal on different websites) holds by construction,
without weakening portal's fence.

| column | type | notes |
|---|---|---|
| `id` | uuid PK | |
| `website_id` | uuid | NOT NULL, FK `website.websites(id)` CASCADE |
| `portal_user_id` | uuid | NOT NULL — LOGICAL ref `portal.portal_users(id)`, indexed |
| `role` | `website_member_role` enum | NOT NULL DEFAULT `'member'` — `member` \| `editor`. `member` unlocks `connected` reads and `restricted` reads that list it; `editor` additionally marks content authority for downstream consumers (the arm consumers activate; no P3 verb consults it beyond read visibility) |
| `metadata` | jsonb | shared audit block |

UNIQUE `(portal_user_id, website_id)` — plain (both arms NOT NULL), in the
table DDL. The bootstrap-minted public principal may never appear here
(service refusal, probe-asserted).

### 1.10 `website.contact_messages` — the reference intake target (WS-11)

The one concrete intake verb at this increment (§6) writes here:

`id` uuid PK, `website_id` uuid NOT NULL FK CASCADE, `name` text? max 120,
`email` text max 320 (the submitter's reply-to — retained for OFFICERS to
reply through their own mail tools; the module itself NEVER sends mail to it,
and no anonymous mail exists anywhere, §6.5), `message` text NOT NULL max
5000, `notified` bool NOT NULL DEFAULT false (flipped when the notifier port
delivers), `metadata` jsonb. NO request-metadata columns exist and none may
ever be added (the GDPR death, §13 BTF-9).

### 1.11 `website.website_audit_log` — the audit vocabulary (portal shape)

`id` uuid PK, `event` `website_audit_event` enum NOT NULL, `actor` uuid?,
`subject_type` text, `subject_id` uuid?, `detail` jsonb?, `occurred_at`
timestamptz NOT NULL DEFAULT now(). Vocabulary (closed enum):
`website_created`, `page_created`, `page_updated`, `page_published`,
`page_unpublished`, `page_forked`, `generic_deleted_with_fanout`,
`page_renamed`, `menu_created`, `menu_updated`, `menu_deleted`,
`menu_fanout`, `redirect_created`, `redirect_updated`, `redirect_deleted`,
`visitor_merged`, `visitor_gc_swept`, `intake_received`, `intake_refused`,
`publish_refused`. Append-only; service-emitted by the verbs.

### 1.12 RLS posture — none declared

`rowsecurity = false` on every `website.*` table; no policy is ever created.
ADR-0014 posture 4, declared in `index.model.yaml` exactly as portal
declares it (§11): this module's security axis is the verb surface (public
read allowlist + officer write gates + capability-checked tiers), not a
company fence. `rls_app_role.sql` blanket grants cover the schema.

### 1.13 Company ↔ website pairing — derived, never stored

There is NO stored "primary website" column anywhere (the upstream
`res.company.website_id` stored compute is not ported; the organization
module is untouched). The derived read is an exported helper:
`primary_website_of(company) -> Option<Website>` = the live website with the
lowest `(sequence, id)` for that `company_id`. The archive guard survives as
a service rule: `DELETE /admin/websites/:id` REFUSES (typed
`website_is_primary_for_company`) while the website is its company's derived
primary — an officer demotes it (sequence bump) first.

### 1.14 The hardening migration (hand-written, user-owned)

Every index the schema DSL cannot express lives in ONE user-owned migration
`…_website_hardening_constraints.up.sql`. `NIL_UUID` below is
`'00000000-0000-0000-0000-000000000000'::uuid`.

```sql
-- WS-1: the specificity fence. At most ONE generic row per key, and at most
-- ONE specific row per (key, website), among LIVE rows. The COALESCE
-- sentinel makes the generic (NULL-website) arm participate in uniqueness —
-- a plain unique would let two concurrent generic inserts both pass
-- (NULL <> NULL), which is exactly the upstream copy-on-write double-clone
-- race this index closes.
CREATE UNIQUE INDEX idx_website_pages_specificity
    ON website.pages (key, COALESCE(website_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE (metadata->>'deleted_at') IS NULL;

-- WS-20 replacement: per-website-scope url uniqueness (generic + its forks
-- share a url legally — different COALESCE arms — but two generics, or two
-- specifics of one website, may never collide). The upstream -1/-2 suffix
-- uniquification loop is dead; a collision is a typed refusal.
CREATE UNIQUE INDEX idx_website_pages_url_scope
    ON website.pages (url, COALESCE(website_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE (metadata->>'deleted_at') IS NULL;

-- WS-13#1: live-domain uniqueness with the NULL arm participating. ONE
-- domainless website may exist (the default slot); a second refuses. This
-- deliberately TIGHTENS upstream (which allows many domainless websites):
-- with hostname binding (§7.1) a domainless website is unreachable from
-- public traffic, and an unbounded pile of unreachable roots buys nothing.
CREATE UNIQUE INDEX idx_website_websites_domain_live
    ON website.websites (COALESCE(domain, ''))
    WHERE (metadata->>'deleted_at') IS NULL;

-- A portal principal is at most ONE website's public principal.
CREATE UNIQUE INDEX idx_website_websites_public_user_live
    ON website.websites (public_user_id)
    WHERE (metadata->>'deleted_at') IS NULL;

-- One redirect answer per path per website.
CREATE UNIQUE INDEX idx_website_redirects_url_from_live
    ON website.redirects (url_from, website_id)
    WHERE (metadata->>'deleted_at') IS NULL;
```

Table-level (generator-emitted) constraints in the model YAML:
`visitors` UNIQUE `(access_token)`, UNIQUE `(digest, website_id)`;
`website_members` UNIQUE `(portal_user_id, website_id)`.

---

## 2. The ONE specificity resolver (WS-1)

Upstream resolves generic-vs-specific in three separate places with no DB
constraint (view lookup, asset lookup, the copy-on-write search-then-copy),
and a fourth tolerant reader papers over the resulting duplicates at read.
This module implements the fold ONCE and enforces it at the DB (§1.14).

**Export site** — `src/application/service/specificity.rs`, re-exported
exactly once through the generated-seam CUSTOM SERVICES block
`src/exports/services.rs` (this module FILLS that block — the exports-first
discipline; portal left its own block empty, bulkops established the filled
shape). No other module file re-exports or re-implements it.

**The shared core** (one SQL shape, two thin entry points):

```rust
/// The single generic-vs-specific fold. Both resolution entry points
/// compose THIS scope; nothing else in the crate may write the
/// `website_id IS NULL` arm by hand.
pub struct SpecificityScope;

impl SpecificityScope {
    /// WHERE fragment: `(website_id = $1 OR website_id IS NULL)`
    pub fn live_scope(website_id: Uuid) -> ScopeFragment;
    /// ORDER fragment: `website_id NULLS LAST` — the specific always wins.
    pub fn prefer_specific() -> OrderFragment;
}

/// Resolve THE most-specific live row for a key on a website.
/// Specific(key, W) -> that row; else Generic(key, NULL) -> that row; else None.
pub async fn resolve_specific(
    tx: &mut PgConnection, key: &str, website_id: Uuid,
) -> Result<Resolution<PageRecord>>;

/// Resolve a page by routing url on a website — SAME fold over the url arm.
/// Used by the public page read and the redirect/canonical chain.
pub async fn resolve_page_by_url(
    tx: &mut PgConnection, url: &str, website_id: Uuid,
) -> Result<Resolution<PageRecord>>;
```

**Resolution semantics** (identical for both entry points, in order):

1. Look for the specific: `WHERE key = $k AND website_id = $w` (live).
2. Else look for the generic: `WHERE key = $k AND website_id IS NULL` (live).
3. Neither → `Resolution::None`.

The resolver NEVER copies rows — copying is exclusively the fork verb (§4).
`Resolution` carries which arm matched (`Specific(row)` | `Generic(row)` |
`None`) so callers never re-derive it. The tolerant duplicate reader is dead
by construction: the constraint makes duplicates impossible.

**The sentinel value**: `NIL_UUID` exists ONLY inside the partial-unique
index expressions (§1.14) as the device that makes NULL-website rows
participate in uniqueness. Runtime queries use `IS NULL` / `= $w`; the
sentinel never appears in application SQL.

**One-call-site guarantee (grep-able invariant, enforced at review + CI
grep gate)**: the generic-vs-specific fold pattern appears in exactly one
file. The build seat adds this to the probe suite as a filesystem assertion
(a test that walks `src/` and fails if `website_id IS NULL` occurs anywhere
outside `specificity.rs`, or if `resolve_specific`/`resolve_page_by_url` are
re-implemented — a grep for `fn resolve_` outside `specificity.rs` must be
empty). Generated id-based fetchers are unaffected: they select by `id` and
carry no fold.

**Concurrent-first-edit semantics**: two simultaneous `fork_to_website`
calls for the same (key, website) both INSERT; the sentinel unique index
makes exactly one land; the loser's `ON CONFLICT DO NOTHING` + re-select
returns the winner's row. Both callers observe the SAME specific row id.
This is the fork probe's assertion (§14).

---

## 3. The publish contract (WS-6)

Three pieces, replacing the upstream compute/inverse/search triple:

1. **Stored state**: `pages.is_published` (bool, default false) +
   `pages.date_publish` (nullable, read-time-lazy — §1.2). Nothing derives
   backwards into the stored state.

2. **Per-website derived visibility** — a read-time predicate over the
   resolver's answer, evaluated fresh (indexed SQL; no cache, §13 BTF-2):

```sql
visible_on_site(row, website, principal_tier) ::=
    row = resolver's pick for (key, website)          -- specificity first
AND row.is_published
AND (row.date_publish IS NULL OR row.date_publish <= now())
AND tier_passes(row.visibility, row.required_member_roles, principal_tier)

-- tier_passes:
--   anonymous request         -> visibility = 'public'
--   verified portal principal -> visibility IN ('public','connected')
--     OR (visibility = 'restricted' AND a website_members row exists for
--         (principal, website) with role = ANY(required_member_roles))
--   officer (admin tree)      -> everything (officers read their own trees)
```

`website_published`-style stored computes do not exist; the derived value is
computed in the read verbs' SQL and named `visible` in the DTO.

3. **The can-publish fence** — an explicit write-time refusal:
   - `is_published` is NOT in any generic patch whitelist. A PATCH carrying
     it refuses `422 website_field_not_patchable` naming the verb to use.
   - `POST /admin/pages/:id/publish` and `POST /admin/pages/:id/unpublish`
     are the only writers, gated by the module-write authority
     (`write:website`; see §9). A refusal there is loud:
     `403 website_publish_permission_required`.
   - Role granularity (upstream's per-record designer `can_publish`) is not
     ported at this increment: officers with `write:website` publish. The
     membership `editor` role is the declared seam a downstream increment
     activates; nothing at P3 consults it for publishing.

Publishing interplay with specificity: forking preserves `is_published`
(§4.2); publishing a GENERIC row publishes it on every website that has not
forked it (the specific shadows the generic per-site — resolver semantics,
not an ambient propagation job).

---

## 4. COW/COU as explicit versioning verbs (WS-2, WS-3)

No ambient `website_id`-in-context exists anywhere; there is no `no_cow`
escape hatch; nothing in the write path sniffs a request header to decide
copy semantics. Copy-on-write and copy-on-unlink are NAMED verbs with
provenance columns (`forked_from`, `forked_at`, `forked_by` on `pages`).

### 4.1 `fork_to_website`

```rust
/// Fork the GENERIC page for `key` into a website-specific copy.
/// Idempotent: if a specific already exists for (key, target), return it.
pub async fn fork_to_website(
    &self, actor: ActorRef, key: &str, target_website: Uuid,
) -> Result<ForkOutcome>;  // Created(PageRecord) | Existing(PageRecord)
```

Sequence, one transaction:

1. `resolve_specific(key, target)` → `Existing(row)` if the specific exists.
2. Load the generic `(key, NULL)`; missing → `422 website_fork_source_missing`.
3. `INSERT … SELECT` the specific copy: same `key`, `url`, `title`,
   `seo_name`, `is_published`, `date_publish`, `website_indexed`,
   `visibility`, `required_member_roles`; `website_id = target`; provenance
   `forked_from = generic.id`, `forked_at = now()`, `forked_by = actor`.
   `ON CONFLICT DO NOTHING` (any conflict on the hardening indexes) followed
   by a re-select — the concurrent-fork race resolves to the single winner.
4. Copy `page_blocks` (full set, positions preserved).
5. Re-point menus: `UPDATE website.menus SET page_id = <new id> WHERE
   website_id = target AND page_id = generic.id`.
6. Audit `page_forked` (detail: key, target, created id).

### 4.2 `delete_generic_with_fanout`

```rust
/// COU: before deleting a GENERIC page, force-fork it to every website that
/// has no specific yet, re-point that website's menus, then delete the
/// generic. All-or-nothing in one transaction.
pub async fn delete_generic_with_fanout(&self, actor: ActorRef, key: &str) -> Result<FanoutDeletion>;
```

Sequence: for every live website W lacking a specific for `key`: run the
fork internals (steps 3–4 above, provenance = the generic); then re-point
EACH website's menus bound to the generic to that website's new specific
(after the fanout loop every website has one); then soft-delete the generic.
Audit `generic_deleted_with_fanout` with the created-id list in `detail`.
If any website's fork fails, the transaction rolls back whole — a partial
fanout is never committed.

Plain `DELETE /admin/pages/:id` on a SPECIFIC row deletes that row and
re-points its website's menus back to the generic (resolver picks it up).
Plain DELETE on a generic row REFUSES (`422 website_generic_requires_fanout_verb`)
— the fanout verb is the only generic deletion. The upstream
context-triggered COU (delete-local silently copies everywhere) is dead.

### 4.3 What the fork probe asserts (the DoD gate)

Two concurrent `fork_to_website(key, W)` transactions → afterwards: exactly
ONE specific row for `(key, W)` in `website.pages`; both call results carry
the same row id; the row's provenance columns are set; the generic row is
untouched; the menu re-point happened exactly once. Run as a real
two-connection scratch-DB test, exit-code asserted.

### 4.4 Menu fan-out (WS-15) — explicit option only

`POST /admin/menus/:id/fanout` copies a menu to every other website
(re-parented under each target's root), returning the created-id list. The
create verb NEVER fans out implicitly — creating a menu with no website is
impossible (`website_id` NOT NULL). The upstream cross-website same-url
cascade delete is NOT PORTED (its blast radius — deleting one child menu
silently deleting same-url menus on every other website — is a recorded
upstream latent risk): menu deletion is local to its website, full stop.

---

## 5. Visitors (WS-10, the GDPR shape)

### 5.1 Scope decision: per-website rows, global token uniqueness

The digest identifies a BROWSER SESSION (IP+UA+session-id); it does not
include `website_id`. Visitor ROWS are per-website (`website_id` NOT NULL);
`UNIQUE (digest, website_id)` is the upsert target; `UNIQUE (access_token)`
is global (upstream's global arm kept). The same browser on two websites
yields two visitor rows sharing a digest value — cross-website visitor dedup
is NOT a requirement and the GDPR ruling minimizes joinable identity;
identified visitors dedup by `portal_user_id` per website through the merge.
Declared constants (module consts, probe-asserted): 8-hour visit window,
5-minute connected window (derived at read), 30-minute track dedup.

### 5.2 The keyed digest

```
digest = hex( HMAC-SHA256(
    key  = WEBSITE_VISITOR_PEPPER (config, §10),
    msg  = "website-visitor-v1\n" + ip + "\n" + user_agent + "\n" + session_id
) )
```

Domain-separated, newline-delimited arms (no ambiguity injection). The
session id is supplied by the webapp (its own session concept; a header
parameter on the heartbeat verb) — used in the digest, never stored. IP and
user-agent come from the request; the client IP is resolved by the module's
own trusted-proxy posture (§10.3): the RIGHTMOST `X-Forwarded-For` hop when
a trusted reverse proxy is declared, else the connection's socket address
with the forwarded header ignored entirely — a caller rotating the
client-supplied hops of that header can neither dodge the intake rate
windows (§6.2) nor mint fresh visitor identities. Bare sha1 is dead; the raw
IP is never persisted — the stored identity artifacts are the derived digest
token and the resolved `country_code` only.

**Pepper rotation**: rotatable by config change, no migration. Consequence,
recorded honestly: after rotation, a browser's next heartbeat misses the old
digest and mints a NEW visitor row (a fresh lineage); old rows age out
through the 60-day partnerless GC. Identified rows under an old pepper
become unreachable by digest but retain their `portal_user_id` link; a
follow-up sweep for digest-stale identified rows is a named future verb, not
required at this increment. `digest_algo` columns the construction for any
future algorithm change.

### 5.3 The upsert (one statement, SKIP LOCKED kept)

Heartbeat verb `POST /public/visitors/heartbeat`
`{session_id, url?, page_key?}` (+ implicit IP/UA), all in ONE statement:

```sql
WITH v AS (
    INSERT INTO website.visitors
        (id, website_id, access_token, kind, digest, digest_algo, country_code)
    VALUES ($id, $website, $token, 'anonymous', $digest, 'hmac-sha256-v1', $cc)
    ON CONFLICT (digest, website_id) DO UPDATE SET
        last_connection_at = now(),
        visit_count = website.visitors.visit_count
            + CASE WHEN website.visitors.last_connection_at
                       < now() - interval '8 hours' THEN 1 ELSE 0 END
    RETURNING id, (xmax = 0) AS inserted
), t AS (
    INSERT INTO website.visitor_tracks (id, visitor_id, page_id, url, visited_at)
    SELECT gen_random_uuid(), v.id, $page_id, $url, now() FROM v
    WHERE $with_track::bool AND NOT EXISTS (
        SELECT 1 FROM website.visitor_tracks x
        WHERE x.visitor_id = v.id AND x.url = $url
          AND x.visited_at > now() - interval '30 minutes')
)
SELECT id, inserted FROM v;
```

The track CTE rides the SAME statement (the upstream atomicity trick — a
visitor row and its first track land or neither does). Concurrent
last-visit/merge updates use `FOR NO KEY UPDATE SKIP LOCKED` subselects (the
upstream deliberate lock-skip, kept):

```sql
UPDATE website.visitors SET …
WHERE id IN (SELECT id FROM website.visitors
             WHERE … FOR NO KEY UPDATE SKIP LOCKED);
```

There is NO public or officer CREATE verb for visitors — the heartbeat path
is the only creator (the upstream `perm_create=0` posture). Bot suppression
and `X-Disable-Tracking` are honored headers on the heartbeat (a declared
no-op response, not an error).

### 5.4 Login merge

`POST /admin/visitors/merge` (the webapp calls it when a portal principal
authenticates with an anonymous visitor active; it is also the declared seam
livechat/WB-8 consume):

1. Find the principal's identified visitor on this website
   (`portal_user_id = $p AND website_id = $w`).
2. Exists → reparent: `UPDATE visitor_tracks SET visitor_id = <existing>
   WHERE visitor_id = <anon>`; `DELETE` the anonymous row.
3. Not exists → CLAIM the anonymous row in place: `kind = 'identified'`,
   `portal_user_id = $p` (the RANDOM token is kept — no token upgrade ever).
4. One transaction, idempotent, audited `visitor_merged`.

The upstream "set the anonymous token to the partner id in place" branch is
dead by design — tokens are random and stay random.

### 5.5 The 60-day GC

`sweep_partnerless_visitors`: hard `DELETE` (never archive) where
`portal_user_id IS NULL AND last_connection_at < now() - interval '<retention>'`
(retention default 60 days, config §10), batch 1000 per transaction with
commit-progress (each committed batch logs progress; tracks cascade).
Registered in `schema/hooks/index.hook.yaml`:

```yaml
scheduled_jobs:
  visitor-gc:
    schedule: "23 3 * * *"            # daily, off the :00 mark
    handler: "website::sweep_partnerless_visitors"
    posture: pull                     # plain pull — no self-arming trigger
    commit_policy: commit_per_batch
    pickup_lock: true                 # FOR UPDATE SKIP LOCKED on the batch claim
```

The verb is safe to run concurrently (batch claims lock-skip) and re-runnable;
safety never depends on the schedule firing (nothing gates on swept state).

---

## 6. Intake verbs (WS-11 replaced; website_cf_turnstile consumed)

### 6.1 Declarations, not an engine

The upstream `POST /website/form/<model>` generic engine (model name in the
URL, two-gate whitelist, SUPERUSER create, a column whose DEFAULT TRUE is
cleared by raw SQL) is dead. Intake verbs are DECLARED per target in Rust:

```rust
pub trait IntakeDeclaration: Send + Sync {
    const NAME: &'static str;                 // fixed route segment, e.g. "contact"
    const REQUIRES_TURNSTILE: bool;
    type Payload: for<'de> Deserialize<'de>;  // typed allowlist — deny_unknown_fields
    async fn validate(payload: &Self::Payload) -> Result<(), IntakeRejection>;
    async fn persist(tx: &mut PgConnection, website_id: Uuid,
                     payload: Self::Payload) -> Result<IntakeOutcome, IntakeRejection>;
}
```

The typed `Payload` struct IS the field allowlist — `deny_unknown_fields`
makes unknown keys unparseable (a 422 at the edge), and there is no column
list anywhere to drift. The executor is generic over the declaration; the
reference declaration `ContactIntake` (name `contact`, target
`website.contact_messages`) ships with the module. Downstream consumers
(blog comments, lead capture, event registration funnels) declare their own
`IntakeDeclaration` in their own crates and mount through the exported
executor — never a model-name-in-URL route, never a second engine.

### 6.2 The per-request executor contract (in order)

1. **Website resolution** — §7.1 (hostname header; miss → 404
   `website_not_resolved`).
2. **Turnstile, fail-closed, unset ≠ misconfigured** — when the declaration
   requires turnstile:
   - secret unset in config → `503 website_turnstile_not_configured`
     (typed; the host sees it at boot as a WARN too);
   - Cloudflare rejects the token → `400 website_turnstile_refused`;
   - Cloudflare rejects the SECRET (`invalid-input-secret` response code) →
     `503 website_turnstile_misconfigured` — distinguishable from both;
   - transport failure reaching the verifier → refuse (fail-closed), `503
     website_turnstile_unreachable`. No passthrough on any error path.
   The verify call is server-side (the module's ONLY outbound HTTP); the
   siteverify URL is config-overridable for probes.
3. **Tier B rate limits** — per-identity AND per-IP fixed-window buckets
   (identity key = visitor digest, armed only when a session is presented;
   an anonymous submission carries no identity to rate separately, so the
   IP window — always armed — is its only arm, keyed on the client IP
   resolved under the §5.2 posture). Limits are per-declaration
   consts; the reference contact verb ships 5 per identity per hour and 20
   per IP per hour. Exceeding → `429 website_intake_rate_limited` with
   `Retry-After`. In-memory per composing service (the accepted family
   trade; multi-instance hosts front a shared limiter).
4. **Per-verb savepoint** — the persist runs inside
   `SAVEPOINT intake_<name>` of the request transaction; any rejection
   rolls back to the savepoint and returns a loud typed 4xx. Upstream's
   silent `json False` on IntegrityError is dead.
5. **No sudo anywhere** — the write executes as the ordinary app role under
   the request's scope; there is no elevated context, no SUPERUSER path, no
   service-layer privilege escalation. Identity columns can never be
   admitted by an allowlist: the typed payload structs for the reference
   verb and every downstream declaration carry only the fields the verb
   owns (the intake design rule applied forward at every future consumer).
6. **Loud 4xx shapes** (the full refusal vocabulary):

| status | code | when |
|---|---|---|
| 404 | `website_not_resolved` | hostname binds to no live website |
| 400 | `website_turnstile_refused` | token invalid |
| 503 | `website_turnstile_not_configured` | required but no secret in config |
| 503 | `website_turnstile_misconfigured` | verifier rejects the secret |
| 503 | `website_turnstile_unreachable` | verifier transport failure |
| 422 | `website_intake_field_rejected` | unknown field (parse-level refusal) |
| 422 | `website_intake_validation_failed` | typed validation (lengths, formats) |
| 429 | `website_intake_rate_limited` | either bucket exceeded |
| 422 | `website_intake_refused` | declaration-specific persist refusal |

7. **NO anonymous mail** — the module sends nothing to any submitter
   address, ever. Fixed-recipient notification rides a port:

```rust
pub trait IntakeNotifier: Send + Sync {
    async fn notify_intake(&self, website: &WebsiteView, message: &ContactMessageView);
}
```

Host-installed (e.g. bridging to the notification module). Unwired → one
WARN at first use, the row lands with `notified = false`; the port never
refuses the write and never mails the submitter. There is no mail-module
dependency edge.

---

## 7. The public read surface (the declared allowlist + negative probe)

### 7.1 Request-to-website binding (BTF-1, frozen)

`resolve_website(host_header)`: lowercase, strip port, IDNA-normalize →
exact match against live `websites.domain` → the website; miss → typed
`404 website_not_resolved`. There is NO session force flag and NO
first-website fallback — upstream's silent fallback onto website #1 is the
cross-site content-bleed failure mode and is deliberately not ported
(recorded deviation). The admin tree operates by explicit website ids and
never binds by host.

### 7.2 The allowlist (exhaustive — nothing else answers unauthenticated)

| verb | route | returns |
|---|---|---|
| page read | `GET /public/pages/{*url}` | the resolved published page + blocks, or 404/403 per §3's tier rules (unpublished → 404; off-website → 404; published-but-restricted → `403 website_page_visibility_refused`) |
| menu tree | `GET /public/menus` | the website's menu tree, visible entries only (same tier rules) |
| sitemap | `GET /public/sitemap?cursor=` | urls of live pages where `is_published AND (date_publish IS NULL OR ≤ now) AND website_indexed` AND the page belongs to THIS website — cursor-paginated, at most 45,000 entries per page (the upstream split threshold, generalized as pagination; a bigger site pages, never truncates silently). Generated FRESH per request (no attachment cache — §13 BTF-2/BTF-10) |
| robots | `GET /public/robots.txt` | the stored `robots_txt` verbatim. The upstream "Disallow: / when the host is not the website's own domain" guard is structural here: the website was RESOLVED by that host, so a mismatched-host request never reaches this verb |
| resolve | `GET /public/resolve?path=&method=&accept_language=&bot=` | the routing answer: `{action: "serve" | "redirect" | "not_found", status?, location?, page?}` — the redirect table + the 9-case matcher (§8) answer here. This is the webapp's per-navigation routing seam |
| heartbeat | `POST /public/visitors/heartbeat` | §5.3 upsert (the only mutating public verb besides intake) |
| intake | `POST /public/intake/{verb}` | §6; the `{verb}` segment matches a DECLARATION name (a fixed closed set), never a model name |

Public = unauthenticated. A valid portal bearer may additionally unlock
`connected`/`restricted` tiers on page reads and menus — verification rides
the principal port (§9.3); with the port unwired, every non-public tier
reads as `403` (fail-closed, never fail-open).

### 7.3 The negative probe contract (DoD gate)

Seed: website A (published page P1, unpublished page P2, `website_indexed =
false` page P3, restricted page P4) + website B (published page P5, and a
SPECIFIC fork of P1's key). Assert from an unauthenticated caller bound to
A's host:

- `GET /public/pages/P2-url` → 404 (existence hidden);
- `GET /public/pages/P5-url` → 404 (off-website rows are structurally
  invisible — the resolver scope sees only A's specifics + generics);
- `GET /public/pages/P4-url` → 403 (published, restricted tier);
- `GET /public/pages/P1-forked-url` → 200 with the FORK's content (the
  specific shadows the generic on A);
- `GET /public/sitemap` → contains P1's url; contains neither P2, P3, nor
  P5's url;
- `GET /public/menus` → only visible-on-A entries;
- no list/enumerate verb exists on the public tree for pages (grep-asserted
  route table: the public router registers exactly §7.2's routes).

---

## 8. SEO / routing layer

### 8.1 The ONE slug utility (WS-16, PI-30)

`src/application/service/slug.rs`, exported once via `exports/services.rs`:

```rust
pub fn slug_from(id: SlugId, seo_name: Option<&str>, fallback_name: Option<&str>) -> String;
pub enum SlugId { Uuid(Uuid), Int(i64) }   // Int arm applies abs() — the
                                           // upstream negative-id retry guard
```

Rules: prefer `seo_name`, else kebab(`fallback_name`), else the literal
`"page"`; append `-{id}` — for `SlugId::Uuid` the FULL simple-form uuid
(36 chars, unambiguous), for `SlugId::Int` the decimal `abs(id)` (the
upstream negative-id retry guard). Output is lowercase kebab, max 120
characters. Every generated external reference in the module family (blog
slugs, event urls, stale-slug redirects) goes through THIS function — a
second slug implementation anywhere downstream is a review refusal.

### 8.2 Canonical-301 rules (PI-29) and the 9-case matcher (PI-28)

The `/public/resolve` verb and the page read apply, in FIRST-MATCH-WINS
order, this decision table (table-driven: one ordered `const` array of
`MatcherCase { name, predicate, action }` in
`src/application/service/lang_matcher.rs`; each row is unit-probed):

| # | case | condition | action |
|---|---|---|---|
| 1 | bot-never-redirect | caller-declared bot | serve/404 only — never a lang/normalize redirect |
| 2 | POST-never-bounce | method ≠ GET | never 3xx — 404 or 200 only |
| 3 | trailing-slash 301 | path ends `/` and ≠ `/` | 301 → path minus trailing slash |
| 4 | `//`-collapse 301 | path contains `//` | 301 → collapsed |
| 5 | default-lang redirect | path starts `/{default_lang_code}/` | 301 → same path unprefixed |
| 6 | lang-alias 301 | path starts a lang code that is an ALIAS of another | 301 → canonical-lang-prefixed path. W7 ACTIVATION: the alias registry is EMPTY (single-default-language only) — the row ships, matches nothing, and lights up when the language-registry increment lands (§13) |
| 7 | redirect-table reroute | live redirect row matches the normalized path | 301/302/308 → `url_to`; 404 → `not_found` |
| 8 | canonical-301 | page resolves at a url ≠ the requested one (via a 308 alias or lang variant) | 301 → the page's stored `url` |
| 9 | terminal | no rule, no page | `not_found` |

Single-default-language activation, frozen: `websites.default_lang_code` is
the ONLY language datum; there is no language table, no per-website M2M
(the named language-registry gap, §13); case 5 is live, case 6 dormant.

### 8.3 Page rename (the redirect generator)

`POST /admin/pages/:id/rename {url, create_redirect?: 301|302}` — validates
normalization, updates `pages.url`, cascades bound menus' targets, syncs
`homepage_url` when the renamed page is the homepage, and — when
`create_redirect` is set — writes a `redirects` row (`url_from` = old url,
`url_to` = new). Blog's stale-slug 301s consume this same redirect table
through the exported `WebsiteSurface` (§9.4).

---

## 9. Routes, mounts, and the exported surface

### 9.1 Mount plan

ONE base: `/api/v1/website`. The module exports two pure routers; the host
nests both (merge then nest once, or two nests on disjoint subtrees):

- **Public tree** — `website_public_routes(PublicState) -> Router`:
  §7.2's routes under `/public/**`. Mounted BARE of `company_auth` (the
  portal nest precedent — the module's own gates are the fence). The state
  composes from env (`PublicState::from_env(pool)`) mirroring
  `PortalPublicState`.
- **Admin tree** — `website_admin_routes(AdminState) -> Router`: reads +
  writes under `/admin/**`, mounted behind `company_auth` (company
  verifier state) with `ModuleWriteGate::new(pool, "website")` as the INNER
  route_layer (the foundation_ext pattern verbatim: gate innermost,
  company_auth outside). Authority names resolve through the host gate:
  `write:website` (POST/PUT/PATCH), `delete:website` (DELETE),
  `admin:website` / `ADMIN` / `*:*` supersets.

### 9.2 Admin route table (exhaustive)

```
GET    /admin/websites                     list (company_auth)
POST   /admin/websites                     create + bootstrap (write) — §5 website create:
                                            seeds homepage page (key 'homepage', url =
                                            THIS website's homepage_url),
                                            root menu + first-level defaults, mints the
                                            public portal principal (a portal row via the
                                            exported portal surface — officers act, the
                                            ghost principal never logs in)
GET    /admin/websites/:id                 detail
PATCH  /admin/websites/:id                 config fields (write) — never public_user_id
DELETE /admin/websites/:id                 (delete) — refuses while derived-primary (§1.13)
GET    /admin/pages?website_id=            list incl. unpublished (officer sight)
POST   /admin/pages                        create (write)
PATCH  /admin/pages/:id                    typed patch (write) — is_published refused (§3)
POST   /admin/pages/:id/publish            (write) — the can-publish fence verb
POST   /admin/pages/:id/unpublish          (write)
POST   /admin/pages/fork                   {key, target_website_id} → fork_to_website (write)
POST   /admin/pages/fanout-delete          {key} → delete_generic_with_fanout (write)
POST   /admin/pages/:id/rename             (write) — §8.3
DELETE /admin/pages/:id                    (delete) — specifics only (§4.2)
GET    /admin/menus?website_id=            tree
POST   /admin/menus                        create (write) — website_id required
POST   /admin/menus/:id/fanout             explicit per-website fan-out copy (write)
PATCH  /admin/menus/:id                    (write) — depth/mega validation service-side
DELETE /admin/menus/:id                    (delete) — local only (§4.4)
GET    /admin/redirects?website_id=
POST   /admin/redirects                    (write) — 308 param-parity validated
PATCH  /admin/redirects/:id                (write)
DELETE /admin/redirects/:id                (delete)
GET    /admin/visitors?website_id=         read (officer)
POST   /admin/visitors/merge               §5.4 (write)
POST   /admin/visitors/sweep               manual GC trigger (write) — same verb as the cron
GET    /admin/intake/contact-messages?website_id=
GET    /admin/blocks/page/:page_id         block reads ride the page DTOs; no separate block verbs
```

### 9.3 The principal port (fail-closed)

```rust
pub trait WebsitePrincipalVerifier: Send + Sync {
    /// Returns the verified portal principal for a presented bearer token,
    /// or None. The host installs the adapter (over backbone-portal's
    /// verification surface); the module never verifies bearers itself.
    async fn verify(&self, token: &str) -> Option<WebsitePrincipal>;
}
pub struct WebsitePrincipal { pub user_id: PortalUserId, pub email: String }
```

Slot + refusing default (the portal `credential_port` shape): unwired →
every non-public tier on the public tree reads `403` and the admin tree
needs no portal principal at all (it authenticates officers through the
host's company_auth). `PortalUserId` is imported from backbone-portal's
generated exports — the dependency edge is load-bearing at compile time.

### 9.4 The exported trait surface (what blog / livechat / W8 / WB-8 consume)

`WebsiteSurface` (in `src/application/service/website_surface.rs`,
re-exported through `exports/services.rs` — the artifact name downstream
modules hold this module to, mirroring portal's `PortalDocumentSurface`):

```rust
#[async_trait]
pub trait WebsiteSurface: Send + Sync {
    async fn resolve_website_by_host(&self, host: &str) -> WebsiteResult<WebsiteView>;
    async fn visible_page(&self, website_id: Uuid, url: &str,
                          principal: Option<&WebsitePrincipal>) -> WebsiteResult<Option<PageView>>;
    async fn menu_tree_visible(&self, website_id: Uuid,
                               principal: Option<&WebsitePrincipal>) -> WebsiteResult<Vec<MenuNode>>;
    async fn redirect_answer(&self, website_id: Uuid, url: &str) -> Option<RedirectAnswer>;
    async fn record_redirect(&self, website_id: Uuid, url_from: &str,
                             url_to: &str, kind: RedirectKind) -> WebsiteResult<()>;  // blog stale-slug 301s
    async fn company_allowlist(&self, principal: &WebsitePrincipal,
                               website_id: Uuid) -> Vec<Uuid>;
    async fn track_visit(&self, website_id: Uuid, session: &SessionFacts,
                         url: &str, page_key: Option<&str>) -> WebsiteResult<()>;  // livechat/WB-8 seam
    async fn sweep_visitors(&self) -> WebsiteResult<SweepSummary>;  // GC verb reuse
}
```

**The company allow-list derivation** (COMPANY-FENCE floor row — derived
website-side, keyed on the portal principal):

```
company_allowlist(principal, website) =
    principal IS the website's public principal  → [website.company_id]
    principal holds a membership on this website → [website.company_id]
    otherwise (verified principal, no membership) → []   (public tier only)
```

Officers are not in this function's domain (their reads ride company_auth).
The derivation is exported for family-side read models; this module itself
uses it only through the visibility tiers.

### 9.5 Host compose (the build seat's wiring, mirroring verified patterns)

- seam: `src/infrastructure/seams/website_compose.rs` (portal_compose /
  foundation_ext_compose precedents); state compose + principal-port install
  + notifier-port install.
- env-presence WARN loop at boot for `WEBSITE_VISITOR_PEPPER` and
  `WEBSITE_TURNSTILE_SECRET` (the portal secrets loop pattern).
- pin: `backbone-website = { git = "https://github.com/faridlab/backbone-website",
  tag = "v0.1.0" }` — an UNCONDITIONAL git-tag pin (no cargo feature, no
  cfg gate — nothing may compile the nest out silently), added to
  `COMPOSED_PIN_SET` in `scripts/pin-probe.sh` in the same change. The
  allowlist already names `backbone-website` (probe lines 423–424; verified
  pattern-liveness).
- workspace `metaphor.yaml`: module entry (`name: backbone-website`, type
  `module`, path `./modules/backbone-website`, remote + tag) + serpa-service
  `depends_on` addition; metaphora root manifest gets the same three-line
  module entry as backbone-foundation-ext's.
- `rls_app_role.sql` re-run as owner after `metaphor migration run-all`.
- NO outbox_schemas entry (§Identity).

---

## 10. Migrations, crons, config, Cargo.toml

### 10.1 Migration list (raw-SQL runner order; timestamps may shift, names/one-liners are the contract)

```
20260902000001_create_enums.{up,down}.sql
    public-schema enum types: website_visibility, website_member_role,
    website_visitor_kind, website_redirect_type, website_page_block_kind,
    website_menu_block_kind, website_audit_event (all website_-prefixed,
    census-checked against every module migration tree before landing).
20260902000002_create_website_table.{up,down}.sql      — websites
20260902000003_create_website_member_table.{up,down}.sql — website_members (+ plain unique)
20260902000004_create_page_table.{up,down}.sql          — pages
20260902000005_create_page_block_table.{up,down}.sql    — page_blocks
20260902000006_create_menu_table.{up,down}.sql          — menus
20260902000007_create_menu_block_table.{up,down}.sql    — menu_blocks
20260902000008_create_redirect_table.{up,down}.sql      — redirects
20260902000009_create_visitor_table.{up,down}.sql       — visitors (+ 2 plain uniques)
20260902000010_create_visitor_track_table.{up,down}.sql — visitor_tracks
20260902000011_create_contact_message_table.{up,down}.sql — contact_messages
20260902000012_create_website_audit_log_table.{up,down}.sql — website_audit_log
20260902000013_add_audit_triggers.up.sql                — metadata timestamp triggers
                                                          (up-only, the family shape)
20260902000020_website_hardening_constraints.{up,down}.sql — §1.14 (user-owned)
```

### 10.2 Crons

Exactly ONE: `visitor-gc` (§5.5). Nothing else owns a clock: scheduled
publish is read-time-lazy (§3), route/redirect reads are fresh (§13 BTF-2),
the upstream snippet-asset cron dies with asset bundles (§13).

### 10.3 Config knobs (module-owned env, host-declared in BOTH
`deployment/.env.dev.example` and `apps/serpa-service/.env.prod.example`)

| knob | type | default | env var |
|---|---|---|---|
| visitor digest pepper | string (secret) | none — REQUIRED at first visitor verb; unset → typed `website_visitor_pepper_not_configured` refusal, boot WARN | `WEBSITE_VISITOR_PEPPER` |
| turnstile secret key | string (secret) | unset → turnstile-required verbs 503-not-configured (§6.2) | `WEBSITE_TURNSTILE_SECRET` |
| turnstile siteverify URL | string | `https://challenges.cloudflare.com/turnstile/v0/siteverify` (probe override) | `WEBSITE_TURNSTILE_VERIFY_URL` |
| visitor GC retention days | string-parsed u64 | `60` | `WEBSITE_VISITOR_RETENTION_DAYS` |
| visitor GC batch size | string-parsed u64 | `1000` | `WEBSITE_VISITOR_GC_BATCH` |
| trusted reverse proxy | bool-tolerant string | unset/false — the forwarded header is client-controlled and ignored entirely; `true`/`1`/`yes`/`on` resolve the client IP from the RIGHTMOST forwarded hop (§5.2, §6.2) | `WEBSITE_TRUSTED_PROXY` |

**Exactly one boolean knob ships** (`WEBSITE_TRUSTED_PROXY`), and it
carries the bool-tolerant parse (truth: `"true"`/`"1"`/`"yes"`/`"on"` in
any case; unset or any other value reads false — the fail-closed arm).
The parse is string-typed on purpose — the config loader's `${VAR:default}`
overlay substitutes plain text, and a bare bool field crash-loops the
service (the recorded host boot-fix lesson). String-typed knobs are the
standing preference.

### 10.4 Cargo.toml (exact dependency block semantics)

```toml
[package]
name = "backbone-website"
version = "0.1.0"
edition = "2021"

[dependencies]
# Framework — single-rev, tag-equal, no branch floats, no [patch]:
backbone-core      = { git = "https://github.com/faridlab/backbone-framework", tag = "v2.7.11", features = ["postgres"] }
backbone-orm       = { git = "https://github.com/faridlab/backbone-framework", tag = "v2.7.11" }
backbone-auth      = { git = "https://github.com/faridlab/backbone-framework", tag = "v2.7.11" }
backbone-rate-limit = { git = "https://github.com/faridlab/backbone-framework", tag = "v2.7.11" }
# backbone-messaging is DELIBERATELY ABSENT: this module stages no outbox
# events and subscribes to none (recorded absence — add only with a real
# producer/consumer and an outbox_schemas decision in the same change).

# THE ONLY module edge (the dependency-edge contract): portal's exported
# type-safe ids + principal/verification surface are the identity source for
# every principal reference in this schema.
backbone-portal = { git = "https://github.com/faridlab/backbone-portal", tag = "v0.2.0" }

# Sibling modules NOT consumed by design — recorded so no later change adds
# them casually: backbone-events / any marketing-tree / sales-graph module
# (event surfaces consume THIS module's publish contract + intake engine;
# the build-order sequencing is not a dependency); backbone-sapiens (portal
# is the identity seam; officers authenticate through the host).

tokio, async-trait, serde, serde_json, uuid, chrono,
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres", "uuid", "chrono", "json", "migrate"] },
axum = { version = "0.7", features = ["macros"] }, tower, tower-http,
tracing, thiserror,
hmac = "0.12", sha2 = "0.10", rand = "0.8",        # the visitor digest + tokens
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }  # turnstile verify — the ONLY outbound call

[dev-dependencies]
tempfile, tokio-test, pretty_assertions
```

No `[lints]` table (the clippy bar is the gate-time CLI flag, §Family). The
`PortalUserId` import from backbone-portal's generated `exports/` is the
compile-time load-bearing use of the edge.

### 10.5 CLAUDE.md skeleton (module root)

```markdown
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
```

---

## 11. index.model.yaml (exact content)

```yaml
module: website
version: 2
schema: website
description: "website — the headless website engine: per-website pages/menus with a single generic-vs-specific resolver backed by real unique(key, website_id) constraints, explicit copy-on-write/copy-on-unlink versioning verbs with provenance, the stored+derived+fenced publish contract, GDPR-shaped visitors (keyed digest, random tokens, no raw IP), turnstile-gated declared intake verbs, the SEO redirect table + table-driven language matcher (single-default-language activation), and a declared public read allowlist with a negative-enumeration posture (Odoo website port, headless)"

# ADR-0014 company fence: the upstream addon ships ZERO company ir.rules —
# multi-website scoping is code (website_domain folded into every read).
# The port declares that posture honestly as `none`: no company-gating
# column, no RLS policy. Visibility is service-layer policy in every read
# verb (the declared public read allowlist + tier checks); tenant
# isolation remains the only DB fence. The company allow-list DERIVATION
# (public principal -> [website.company_id]; member -> same; otherwise
# []) is exported for family-side read models — derived, never stored.
company_fence: none

config:
  database: postgresql
  soft_delete: true          # visitors/visitor_tracks opt OUT (hard-delete
                             # models — the digest upsert forbids dead rows)
  audit: true
  default_timestamps: true
  generators:
    disabled: [graphql, grpc, proto]

external_imports:
  - module: portal           # principal refs are LOGICAL (no cross-schema
    types: [PortalUser]      # FK — family rule; indexed + service-checked)

shared_types:
  Timestamps:
    created_at: { type: datetime, attributes: ["@default(now)"] }
    updated_at: { type: datetime, attributes: ["@updated_at"] }
    deleted_at: { type: datetime? }
  Actors:                      # plain uuids — NO sapiens edge in this module,
    created_by: { type: uuid?, attributes: ["@exclude_from_foreign_key_check"] }
    updated_by: { type: uuid?, attributes: ["@exclude_from_foreign_key_check"] }
    deleted_by: { type: uuid?, attributes: ["@exclude_from_foreign_key_check"] }
  Metadata: [Timestamps, Actors]

imports:
  - website.model.yaml        # Website
  - website_member.model.yaml # WebsiteMember
  - page.model.yaml           # Page
  - page_block.model.yaml     # PageBlock
  - menu.model.yaml           # Menu
  - menu_block.model.yaml     # MenuBlock
  - redirect.model.yaml       # Redirect
  - visitor.model.yaml        # Visitor
  - visitor_track.model.yaml  # VisitorTrack
  - contact_message.model.yaml# ContactMessage
  - website_audit_log.model.yaml # WebsiteAuditLog

# FLAG-ID COVERAGE — the register rows this module closes
# (docs/odoo/website/website/; workspace register w7-register-deltas.md):
#  WS-1  → ONE resolver (specificity.rs) + the COALESCE-sentinel partial
#          unique (hardening migration) — constraint adopted, race closed.
#  WS-2/WS-3 → explicit fork_to_website / delete_generic_with_fanout verbs
#          with provenance columns; no ambient context, no no_cow hatch.
#  WS-4  → declared service-layer policy (company_fence: none), never RLS.
#  WS-5  → fenced: no QWeb view-is-content; pages ARE the content rows;
#          blocks are structured jsonb validated per kind.
#  WS-6  → publish contract: stored is_published + derived visibility +
#          the publish-verb fence (§3 of docs/spec.md).
#  WS-7  → public/connected/restricted tiers port as read policy; the
#          password tier + session-unlock machinery NOT ported (fenced).
#  WS-8  → per-website public principal = websites.public_user_id (logical
#          FK portal.portal_users(id)); enumeration posture = allowlist.
#  WS-9  → themes/asset bundles fenced (fence register, docs/spec.md §13).
#  WS-10 → GDPR digest (HMAC pepper, rotatable), random tokens + kind,
#          raw IP never persisted; SKIP LOCKED upsert; login merge + GC.
#  WS-11 → declared intake verbs, typed allowlists, turnstile fail-closed,
#          Tier B limits, per-verb savepoint, no sudo, loud 4xx, no
#          anonymous mail (§6 of docs/spec.md).
#  WS-12 → redirect table + 308 param-parity validation; URL-rewriting of
#          generated links dies with server rendering (webapp consumes
#          /public/resolve).
#  WS-13 → #1 domain sentinel-unique; #2 dies with ControllerPage (fenced);
#          #3 = unique(access_token); #4 = website_members pairing unique
#          (portal_user_id, website_id) — website-side, portal untouched.
#  WS-14 → cache machinery dead: fresh indexed reads; no cache tables.
#  WS-15 → explicit fan-out verb option; cross-website cascade delete NOT
#          ported; depth/mega validation service-side.
#  WS-16 → the ONE slug utility (seo_name preference, abs(id) retry).
#  WS-17 → all four public write paths + the /website/action runner
#          dead-by-design (ADR-0019/0021) — recorded, not rebuilt.
#  WS-18 → visitor GC ports (pull posture); snippet cron dies with assets.
#  WS-19 → menu group visibility = declared read policy in menu verbs.
#  WS-20 → real (key,website_id) + per-website url uniques; the suffix
#          uniquification loop is dead.
```

---

## 12. metaphor.codegen.yaml (user_owned — declared in the same change each file lands)

```yaml
user_owned:
  - "src/application/service/website_error.rs"      # typed error surface, HTTP mappings
  - "src/application/service/specificity.rs"        # THE resolver (§2)
  - "src/application/service/slug.rs"               # the ONE slug utility
  - "src/application/service/lang_matcher.rs"       # the 9-case table
  - "src/application/service/website_service.rs"    # create/bootstrap/resolve/allowlist
  - "src/application/service/page_service.rs"       # page CRUD + rename + publish verbs
  - "src/application/service/menu_service.rs"       # menu tree verbs + validations
  - "src/application/service/versioning_service.rs" # fork_to_website / fanout-delete
  - "src/application/service/redirect_service.rs"   # redirect CRUD + parity validation
  - "src/application/service/visitor_service.rs"    # upsert/merge/GDPR erase
  - "src/application/service/visitor_gc.rs"         # the sweep
  - "src/application/service/intake_engine.rs"      # the executor + trait
  - "src/application/service/intake_contact.rs"     # the reference declaration
  - "src/application/service/principal_port.rs"     # fail-closed portal verifier port
  - "src/application/service/notifier_port.rs"      # fixed-recipient intake notifier port
  - "src/application/service/website_surface.rs"    # the exported trait contract
  - "src/presentation/http/public_routes.rs"        # the declared public allowlist tree
  - "src/presentation/http/admin_routes.rs"         # the officer tree (host gates it)
  - "migrations/*hardening*"                        # the sentinel partial uniques
  - "README.md"
  - "SPEC.md"
  - "docs/**"                                       # this spec + fence records
  - "tests/**"                                      # fail-hard probe suite
```

The generated `src/exports/services.rs` CUSTOM SERVICES block re-exports
`specificity`, `slug`, `website_surface`, `intake_engine`, and
`principal_port` items — inside its `<<< CUSTOM` markers (the marker
mechanism preserves them across regen).

---

## 13. Fences, deaths, and deferred dispositions (the register with citations)

Every row is FROZEN; reopening one requires a new register entry.

**Fenced by design (never built here; rendering/frontend platform we do not own):**

| item | citation | note |
|---|---|---|
| QWeb view-is-content (`website.page` `_inherits ir.ui.view`) | WS-5; pillar 08 §WB-3 | pages are structured rows + blocks; the (key, website_id) grain moved onto the page row |
| html_editor / html_builder (the JS editor) | FENCE-BUILDER; pillar | the webapp owns editing UI |
| themes-as-modules + theme staging tables | WS-9; FENCE-THEMES | no theme concept ships |
| asset bundles / scss overrides / CDN trio | WS-9, WS-14; FENCE-THEMES | no bundler; `cdn_*` fields dropped |
| configurator + IAP surfaces | the W7 fence family | no IAP economy anywhere |
| ControllerPage + its `unique(name_slugified)` | WS-13#2 | dies with controller pages; route metadata is code |
| TechnicalPage / route catalog / lazy route refresh | WS-14; C15 fence list | routes are declared Rust routers |
| snippet filters (+ their public sudo render) | WS-17#4 | dead with the builder |
| password-visibility tier + session `views_unlock` | WS-7; FENCE-MISC | public/connected/restricted port; password tier does not |
| `_export_translation_` class surfaces | C15 fence list | no translation machinery at single-default-language |
| custom head/footer code injection, tracker stripping | C15; BTF-12 | fields dropped; the webapp owns its own tags |

**Dead-by-design (upstream public write/exposure paths — not merely fenced; ADR-0019/0021):**

| path | citation | death |
|---|---|---|
| `/google<16>.html` search-console sudo-write | WS-17#1 | no headless equivalent; prefix-extend oracle gone |
| `/website/google_maps_api_key` public key return | WS-17#2 | keys ship nowhere client-side |
| `/website/social/<name>` open redirect | WS-17#3 | no redirect service; `social_links` stays data-only |
| `/website/snippet/filters` public sudo render | WS-17#4 | dead with snippet filters |
| `/website/action/…` server-action runner | WS-17 | no stored code exists to run |
| `/website/translations` mods passthrough | PI-33 | dropped, not ported |
| install-time `_post_init_hook` is_frontend stamp | PI-34 | moot: installs are inert headless |

**Beyond-the-floor findings — frozen dispositions (BTF-1..13):**

| ID | frozen disposition |
|---|---|
| BTF-1 request binding | hostname-header match; NO session force, NO first-website fallback; loud 404 (§7.1) |
| BTF-2 cache posture | fresh indexed reads everywhere; NO cache tables; the hand-cleared channel bus and page-response cache are dead (no SSR ⇒ no CSRF re-injection either) |
| BTF-3 menu fan-out/cascade | fan-out = explicit verb only; cross-website same-url cascade delete NOT ported; delete is local |
| BTF-4 fuzzy search | DEFERRED on-demand: not in the allowlist, no pg_trgm dependency; a search verb joins the allowlist only with its own surface ruling |
| BTF-5 media | NO media tables/columns (favicon/logo/social_default_image dropped); imagery is webapp/object-storage-owned; the parked MinIO→rustfs migration is the storage backdrop. Re-entry = a media increment |
| BTF-6 per-website signup policy | contract declared, columns deferred: `signup_policy (off\|invite\|open, default off)` + `website_scoped_accounts` land WITH the first consumer (storefront accounts); the kill-switch itself stays portal's |
| BTF-7 date_publish | read-time-lazy, zero crons (the declared `read_time_lazy` posture, consistent with blog) |
| BTF-8 visitor scope/constants | digest EXCLUDES website_id; rows per website; (digest, website_id) unique upsert target; 8h/5min/30min declared consts (§5) |
| BTF-9 intake metadata capture | DEAD: no IP/UA/referer columns may ever exist on intake rows (GDPR ruling recorded so the verbs never grow a metadata flag) |
| BTF-10 robots/sitemap | robots verbatim (host-binding makes the cross-domain guard structural); sitemap fresh + cursor-paginated at the 45,000 threshold; no attachment cache |
| BTF-11 homepage bootstrap | `create_website` seeds the homepage page + menus; '/' resolution = homepage_url reroute → page serve → first reachable menu → 404 (declared chain in the page read) |
| BTF-12 config field set | kept: name/domain/company/public_user/default_lang_code/homepage_url/robots_txt/social_links/contact_recipients/sequence. Dropped: analytics keys, search-console, maps key, plausible, CDN trio, custom code, tracker blocking, cookies_bar behavior, `language_ids` M2M, `specific_user_account` (BTF-6), `configurator_done`, `theme_id` |
| BTF-13 company pairing | derived primary-website read (lowest sequence, live); NO stored column; delete guard while derived-primary (§1.13) |

**Deferred increments (named gaps, not silently skipped):**

- **Language registry** — no language entity exists anywhere in the family
  (verified); the matcher's alias row (§8.2 case 6) ships dormant and the
  registry is its own increment. W7 activates single-default-language only.
- **Downstream builds** — blog, livechat, W8 website_event/eCommerce, WB-8
  lead capture: their consumption contracts are §9.4's exported surface +
  §6's intake engine + §8.1's slug utility. NONE of them is built here;
  nothing in this module may special-case them.

---

## 14. The probe suite (fail-hard; every gate a verified exit code)

Classes and the load-bearing assertions (scratch 5433, one disposable DB per
test, `WEBSITE_TEST_ADMIN_URL` override, missing-scratch = panic):

1. `specificity` — resolver arm selection (specific beats generic; generic
   fallback; none); the grep invariant test (§2's one-file assertion);
   duplicate-key and duplicate-url refusals surface as typed errors.
2. `concurrent_fork` — the §4.3 DoD probe: two simultaneous forks → exactly
   one specific row, same id returned to both, provenance set, menus
   re-pointed once.
3. `publish` — publish/unpublish flip; PATCH carrying `is_published` → 422;
   unpublished bearer cannot see the page; date_publish future-dated hides,
   past-dated shows — no cron involved.
4. `public_allowlist_negative` — §7.3's full negative-probe contract.
5. `visitors` — upsert twice → one row, visit_count window arithmetic;
   pepper unset → typed refusal; NO column anywhere contains the raw IP
   string (full-row assertion); login merge both arms; GC batching;
   SKIP LOCKED concurrent merges.
6. `intake` — no token 4xx; unset-vs-misconfigured turnstile typed apart;
   unknown field 422 at parse; rate-limit 429 + Retry-After; savepoint
   rollback leaves zero rows; the write path runs with no elevated role.
7. `lang_matcher` — each of the 9 table rows, one test each (case 6 against
   a fixture alias registry proving the table drives, the live registry
   empty).
8. `slug` — seo_name preference; fallback kebab; abs(negative-int) retry.
9. `redirects` — 308 param-parity refusal; ambiguity impossible
   (url_from unique); the resolve verb's answers.
10. `menus` — depth ≤ 2 and mega-menu isolation refusvals; fan-out verb
    creates per-website copies; local-only delete.
11. `mount` — public router registers EXACTLY §7.2's routes; admin router's
    write verbs all sit under the module-write gate name `website`
    (route-table introspection, the no-write-routes probe pattern).

Serial runs, exit codes recorded (`echo "GATE <name> EXIT=$?"`), never
output-text judgement. `rls_app_role.sql` is exercised by the host compose
probes, not the module suite.
