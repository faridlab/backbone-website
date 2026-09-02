-- Down: drop website.visitors table
DROP TABLE IF EXISTS website.visitors CASCADE;
DROP FUNCTION IF EXISTS website.visitors_audit_timestamp() CASCADE;
