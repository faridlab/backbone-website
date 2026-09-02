-- Down: drop website.websites table
DROP TABLE IF EXISTS website.websites CASCADE;
DROP FUNCTION IF EXISTS website.websites_audit_timestamp() CASCADE;
