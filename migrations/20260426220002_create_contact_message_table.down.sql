-- Down: drop website.contact_messages table
DROP TABLE IF EXISTS website.contact_messages CASCADE;
DROP FUNCTION IF EXISTS website.contact_messages_audit_timestamp() CASCADE;
