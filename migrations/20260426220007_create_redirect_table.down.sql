-- Down: drop website.redirects table
DROP TABLE IF EXISTS website.redirects CASCADE;
DROP FUNCTION IF EXISTS website.redirects_audit_timestamp() CASCADE;
