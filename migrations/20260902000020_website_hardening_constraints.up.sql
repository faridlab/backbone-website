-- Hardening constraints the schema DSL cannot express (hand-written;
-- user-owned — see metaphor.codegen.yaml).
--
-- The COALESCE sentinel ('00000000-0000-0000-0000-000000000000'::uuid
-- here, '' for the domain arm) makes NULL-Valued rows participate in
-- uniqueness — a plain unique would let two concurrent generic inserts
-- both pass (NULL <> NULL), which is exactly the copy-on-write
-- double-clone race this index family closes. The sentinel exists ONLY
-- in these index expressions; application SQL never uses it (runtime
-- queries use IS NULL / = $w).

-- The specificity fence: at most ONE generic row per key, and at most
-- ONE specific row per (key, website), among LIVE rows. This is the DB
-- half of the single generic-vs-specific resolver — the constraint makes
-- the tolerant duplicate reader dead by construction.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_pages_specificity
    ON website.pages (key, COALESCE(website_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE (metadata->>'deleted_at') IS NULL;

-- Per-website-scope url uniqueness: a generic and its forks share a url
-- legally (different COALESCE arms), but two generics, or two specifics
-- of one website, may never collide. A collision is a typed refusal —
-- the upstream -1/-2 suffix uniquification loop is dead.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_pages_url_scope
    ON website.pages (url, COALESCE(website_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE (metadata->>'deleted_at') IS NULL;

-- Live-domain uniqueness with the NULL arm participating: ONE domainless
-- website may exist (the default slot); a second refuses. This
-- deliberately TIGHTENS upstream (which allows many domainless
-- websites): with hostname binding a domainless website is unreachable
-- from public traffic, and an unbounded pile of unreachable roots buys
-- nothing.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_websites_domain_live
    ON website.websites (COALESCE(domain, ''))
    WHERE (metadata->>'deleted_at') IS NULL;

-- A portal principal is at most ONE website's public principal.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_websites_public_user_live
    ON website.websites (public_user_id)
    WHERE (metadata->>'deleted_at') IS NULL;

-- One redirect answer per path per website — ambiguous stacked rules
-- are impossible at the DB.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_redirects_url_from_live
    ON website.redirects (url_from, website_id)
    WHERE (metadata->>'deleted_at') IS NULL;
