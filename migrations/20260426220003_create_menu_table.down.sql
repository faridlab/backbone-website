-- Down: drop website.menus table
DROP TABLE IF EXISTS website.menus CASCADE;
DROP FUNCTION IF EXISTS website.menus_audit_timestamp() CASCADE;
