-- Down: drop website.pages table
DROP TABLE IF EXISTS website.pages CASCADE;
DROP FUNCTION IF EXISTS website.pages_audit_timestamp() CASCADE;
