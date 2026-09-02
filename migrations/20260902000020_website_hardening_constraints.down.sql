-- Down migration for the website hardening constraints.

DROP INDEX IF EXISTS website.idx_website_redirects_url_from_live;
DROP INDEX IF EXISTS website.idx_website_websites_public_user_live;
DROP INDEX IF EXISTS website.idx_website_websites_domain_live;
DROP INDEX IF EXISTS website.idx_website_pages_url_scope;
DROP INDEX IF EXISTS website.idx_website_pages_specificity;
