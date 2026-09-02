-- Down: drop enum types for website module
DROP TYPE IF EXISTS website_member_role CASCADE;
DROP TYPE IF EXISTS website_audit_event CASCADE;
DROP TYPE IF EXISTS website_visitor_kind CASCADE;
DROP TYPE IF EXISTS website_redirect_type CASCADE;
DROP TYPE IF EXISTS website_page_block_kind CASCADE;
DROP TYPE IF EXISTS website_visibility CASCADE;
DROP TYPE IF EXISTS website_menu_block_kind CASCADE;
