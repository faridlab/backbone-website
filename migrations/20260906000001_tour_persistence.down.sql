-- Migration: Tour persistence (down)
-- Hand-written; user-owned.

ALTER TABLE website.tour_consumptions
    DROP CONSTRAINT IF EXISTS fk_tour_consumptions_tour_id;

DROP TRIGGER IF EXISTS tours_update_audit ON website.tours;
DROP TRIGGER IF EXISTS tours_insert_audit ON website.tours;
DROP FUNCTION IF EXISTS website.tours_audit_timestamp();

DROP TABLE IF EXISTS website.tour_consumptions;
DROP TABLE IF EXISTS website.tours;
