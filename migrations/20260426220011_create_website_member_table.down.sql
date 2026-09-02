-- Down: drop website.website_members table
DROP TABLE IF EXISTS website.website_members CASCADE;
DROP FUNCTION IF EXISTS website.website_members_audit_timestamp() CASCADE;
