-- Migration: Tour persistence (web_tour port — the persistence layer only)
-- Hand-written; user-owned (no generator marker — survives regen sweeps).
--
-- The port of the upstream `web_tour.tour` persistence shape with the
-- module's own fences applied:
--   * tour NAME is unique among LIVE rows (partial unique index over
--     the soft-delete convention — the same live-row-uniqueness family
--     as the hardening constraints), because the onboarding engine
--     consumes tours BY NAME from the client;
--   * per-user consumption is a join table keyed on the LOGICAL
--     portal-user row id (the module's established principal pattern —
--     no cross-schema FK, portal keeps no tour axis), with the
--     (tour, principal) pair unique so consumption is a set-membership
--     fact, never a counter;
--   * the engine itself (trigger/run/tooltip) is client-side and is
--     deliberately NOT ported — only the definitions it reads and the
--     consumption flags it writes.
--
-- Tours are NOT website-scoped (the upstream rows are global registry
-- entries) — same posture as website.website_audit_log.

CREATE SCHEMA IF NOT EXISTS website;

-- Audit vocabulary: the officer tour verbs emit their own events (the
-- closed website_audit_event enum grows three values; consumption is
-- deliberately NOT audited — its trail IS the consumption table).
ALTER TYPE website_audit_event ADD VALUE IF NOT EXISTS 'tour_upserted';
ALTER TYPE website_audit_event ADD VALUE IF NOT EXISTS 'tour_deleted';
ALTER TYPE website_audit_event ADD VALUE IF NOT EXISTS 'tour_reset';

-- ── tour definitions ────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS website.tours (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    rainbow_man_message TEXT,
    steps JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

-- ONE live tour per name: the engine resolves definitions by name, so a
-- duplicate live name is a typed refusal, never a last-write-wins.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_tours_name_live
    ON website.tours (name)
    WHERE (metadata->>'deleted_at') IS NULL;

CREATE INDEX IF NOT EXISTS idx_website_tours_metadata_gin ON website.tours USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_website_tours_metadata_deleted_at ON website.tours ((metadata->>'deleted_at'));
CREATE INDEX IF NOT EXISTS idx_website_tours_metadata_updated_at ON website.tours ((metadata->>'updated_at'));

-- Audit timestamps (the module's per-table convention).
CREATE OR REPLACE FUNCTION website.tours_audit_timestamp() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{created_at}', to_jsonb(NOW()));
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    ELSIF TG_OP = 'UPDATE' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS tours_insert_audit ON website.tours;
CREATE TRIGGER tours_insert_audit BEFORE INSERT ON website.tours
    FOR EACH ROW EXECUTE FUNCTION website.tours_audit_timestamp();

DROP TRIGGER IF EXISTS tours_update_audit ON website.tours;
CREATE TRIGGER tours_update_audit BEFORE UPDATE ON website.tours
    FOR EACH ROW EXECUTE FUNCTION website.tours_audit_timestamp();

-- ── per-principal consumption (the M2M) ─────────────────────────────

CREATE TABLE IF NOT EXISTS website.tour_consumptions (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    tour_id UUID NOT NULL,
    portal_user_id UUID NOT NULL,
    consumed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

-- The membership fact: at most ONE consumption row per (tour,
-- principal). Re-consume is an idempotent no-op, never a second row.
CREATE UNIQUE INDEX IF NOT EXISTS idx_website_tour_consumptions_pair
    ON website.tour_consumptions (tour_id, portal_user_id);

-- The engine's per-user lookup (which tours has THIS user consumed).
CREATE INDEX IF NOT EXISTS idx_website_tour_consumptions_user
    ON website.tour_consumptions (portal_user_id);

CREATE INDEX IF NOT EXISTS idx_website_tour_consumptions_metadata_gin ON website.tour_consumptions USING GIN (metadata);

-- Consumption is an append-only fact: NO delete/soft-delete arm ships
-- in this migration. The officer reset verb (delete rows so a tour can
-- run again) is a service verb over live SQL, not a table shape.

-- Real FK within the schema (tour rows are owned here; deleting a tour
-- definition takes its consumption facts with it).
ALTER TABLE website.tour_consumptions
    ADD CONSTRAINT fk_tour_consumptions_tour_id
    FOREIGN KEY (tour_id) REFERENCES website.tours (id) ON DELETE CASCADE;
