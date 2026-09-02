//! The table-driven language/routing matcher (hand-written; user-owned;
//! see `metaphor.codegen.yaml`).
//!
//! ONE ordered `const` array of `MatcherCase { name, predicate, action }`,
//! FIRST-MATCH-WINS, frozen by spec §8.2. The `/public/resolve` verb and
//! the page read both apply this table. Each row is unit-probed (tests
//! below, one per row); row 6 ships DORMANT — the alias registry is
//! empty at single-default-language activation and lights up when the
//! language-registry increment lands.

/// The language-alias registry (canonical code -> alias codes it
/// absorbs). EMPTY at single-default-language activation: row 6 matches
/// nothing until the language-registry increment populates this table.
pub const LANG_ALIASES: &[(&str, &[&str])] = &[];

/// What the redirect table answered for the normalized path (case 7's
/// input), already validated at write time (308 param parity).
#[derive(Debug, Clone)]
pub struct RedirectAnswer {
    pub redirect_type: String, // moved_301 | found_302 | alias_308 | gone_404
    pub url_to: Option<String>,
}

/// Everything one match needs. Built by the caller after the resolver
/// and redirect reads; the matcher itself touches no IO.
#[derive(Debug, Clone)]
pub struct MatchInput<'a> {
    /// The request path, verbatim (the table's normalize rows see the
    /// raw form).
    pub path: &'a str,
    /// The HTTP method, uppercase ("GET", "POST", ...).
    pub method: &'a str,
    /// Caller-declared bot flag (bots never see lang/normalize
    /// redirects).
    pub bot: bool,
    /// The website's single default language code (the only language
    /// datum at this activation).
    pub default_lang_code: &'a str,
    /// The live redirect-table answer for the path, when one exists.
    pub redirect: Option<RedirectAnswer>,
    /// The stored url of the page the resolver resolved for this
    /// request, when one resolved.
    pub resolved_url: Option<&'a str>,
}

impl<'a> MatchInput<'a> {
    fn page_serves(&self) -> bool {
        // A page serves when the resolver resolved one at exactly this
        // url (case 8 already claimed url-mismatched resolutions).
        match self.resolved_url {
            Some(u) => u == self.path,
            None => false,
        }
    }
}

/// The routing action one row emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatcherAction {
    /// Serve the resolved page (status 200), or 404 when none serves.
    Serve,
    /// Emit the redirect. `status` is 301, 302, or 308.
    Redirect { status: u16, location: String },
    /// The gone/terminal answer (404).
    NotFound,
}

/// One row of the frozen decision table.
pub struct MatcherCase {
    pub name: &'static str,
    pub predicate: fn(&MatchInput<'_>) -> bool,
    pub action: fn(&MatchInput<'_>) -> MatcherAction,
}

// ── predicates ─────────────────────────────────────────────────────────────

fn is_bot(i: &MatchInput<'_>) -> bool {
    i.bot
}

fn not_get(i: &MatchInput<'_>) -> bool {
    i.method != "GET"
}

fn trailing_slash(i: &MatchInput<'_>) -> bool {
    i.path.len() > 1 && i.path.ends_with('/')
}

fn double_slash(i: &MatchInput<'_>) -> bool {
    let p = i.path.strip_prefix('/').unwrap_or(i.path);
    p.contains("//")
}

fn default_lang_prefixed(i: &MatchInput<'_>) -> bool {
    let lang = i.default_lang_code;
    if lang.is_empty() {
        return false;
    }
    let Some(rest) = i.path.strip_prefix('/') else { return false };
    // "/{lang}/..." — the bare "/{lang}" form counts too (it
    // normalizes to the unprefixed root).
    match rest.strip_prefix(lang) {
        Some(after) => after.is_empty() || after.starts_with('/'),
        None => false,
    }
}

fn lang_alias_prefixed(i: &MatchInput<'_>) -> bool {
    // Row 6: dormant while LANG_ALIASES is empty — the predicate can
    // never fire, and that is the shipped state.
    let Some(rest) = i.path.strip_prefix('/') else { return false };
    let first = rest.split('/').next().unwrap_or("");
    LANG_ALIASES
        .iter()
        .any(|(_, aliases)| aliases.iter().any(|a| *a == first))
}

fn redirect_row_present(i: &MatchInput<'_>) -> bool {
    i.redirect.is_some()
}

fn url_mismatch(i: &MatchInput<'_>) -> bool {
    match i.resolved_url {
        Some(stored) => stored != i.path,
        None => false,
    }
}

fn always(_i: &MatchInput<'_>) -> bool {
    true
}

// ── actions ────────────────────────────────────────────────────────────────

fn serve_or_404(i: &MatchInput<'_>) -> MatcherAction {
    if i.page_serves() {
        MatcherAction::Serve
    } else {
        MatcherAction::NotFound
    }
}

fn r301_trim_trailing(i: &MatchInput<'_>) -> MatcherAction {
    let trimmed = i.path.trim_end_matches('/');
    let target = if trimmed.is_empty() { "/" } else { trimmed };
    MatcherAction::Redirect { status: 301, location: target.to_string() }
}

fn r301_collapse(i: &MatchInput<'_>) -> MatcherAction {
    let mut out = String::with_capacity(i.path.len());
    let mut prev_slash = false;
    for c in i.path.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    MatcherAction::Redirect { status: 301, location: out }
}

fn r301_unprefix_default_lang(i: &MatchInput<'_>) -> MatcherAction {
    let lang = i.default_lang_code;
    let rest = i.path.strip_prefix('/').unwrap_or(i.path);
    let after = rest
        .strip_prefix(lang)
        .unwrap_or(rest);
    let target = if after.is_empty() { "/".to_string() } else { after.to_string() };
    MatcherAction::Redirect { status: 301, location: target }
}

fn r301_canonical_lang(i: &MatchInput<'_>) -> MatcherAction {
    // Unreachable while LANG_ALIASES is empty; kept complete so the
    // row lights up with the registry.
    let rest = i.path.strip_prefix('/').unwrap_or(i.path);
    let first = rest.split('/').next().unwrap_or("");
    let canonical = LANG_ALIASES
        .iter()
        .find(|(_, aliases)| aliases.iter().any(|a| *a == first))
        .map(|(c, _)| *c)
        .unwrap_or(first);
    let after = rest.strip_prefix(first).unwrap_or("");
    MatcherAction::Redirect { status: 301, location: format!("/{canonical}{after}") }
}

fn redirect_reroute(i: &MatchInput<'_>) -> MatcherAction {
    match &i.redirect {
        None => MatcherAction::NotFound, // predicate guards this
        Some(r) => match r.redirect_type.as_str() {
            "moved_301" => MatcherAction::Redirect {
                status: 301,
                location: r.url_to.clone().unwrap_or_default(),
            },
            "found_302" => MatcherAction::Redirect {
                status: 302,
                location: r.url_to.clone().unwrap_or_default(),
            },
            "alias_308" => MatcherAction::Redirect {
                status: 308,
                location: r.url_to.clone().unwrap_or_default(),
            },
            _ => MatcherAction::NotFound, // gone_404
        },
    }
}

fn r301_stored_url(i: &MatchInput<'_>) -> MatcherAction {
    MatcherAction::Redirect {
        status: 301,
        location: i.resolved_url.unwrap_or_default().to_string(),
    }
}

// ── the frozen table (first-match-wins) ────────────────────────────────────

/// The 9-row decision table, in frozen order.
pub const MATCHER_TABLE: [MatcherCase; 9] = [
    MatcherCase { name: "bot-never-redirect", predicate: is_bot, action: serve_or_404 },
    MatcherCase { name: "post-never-bounce", predicate: not_get, action: serve_or_404 },
    MatcherCase { name: "trailing-slash-301", predicate: trailing_slash, action: r301_trim_trailing },
    MatcherCase { name: "double-slash-301", predicate: double_slash, action: r301_collapse },
    MatcherCase { name: "default-lang-301", predicate: default_lang_prefixed, action: r301_unprefix_default_lang },
    MatcherCase { name: "lang-alias-301", predicate: lang_alias_prefixed, action: r301_canonical_lang },
    MatcherCase { name: "redirect-table-reroute", predicate: redirect_row_present, action: redirect_reroute },
    MatcherCase { name: "canonical-301", predicate: url_mismatch, action: r301_stored_url },
    MatcherCase { name: "terminal", predicate: always, action: serve_or_404 },
];

/// The routing answer `/public/resolve` serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveAnswer {
    pub action: &'static str, // "serve" | "redirect" | "not_found"
    pub status: u16,          // 200 | 301 | 302 | 308 | 404
    pub location: Option<String>,
    /// The matched row's name (observability; never client-facing).
    pub matched_row: &'static str,
}

/// Apply the table to one input, first-match-wins.
pub fn apply_matcher(i: &MatchInput<'_>) -> ResolveAnswer {
    for case in &MATCHER_TABLE {
        if (case.predicate)(i) {
            return match (case.action)(i) {
                MatcherAction::Serve => ResolveAnswer {
                    action: "serve",
                    status: 200,
                    location: None,
                    matched_row: case.name,
                },
                MatcherAction::Redirect { status, location } => ResolveAnswer {
                    action: "redirect",
                    status,
                    location: Some(location),
                    matched_row: case.name,
                },
                MatcherAction::NotFound => ResolveAnswer {
                    action: "not_found",
                    status: 404,
                    location: None,
                    matched_row: case.name,
                },
            };
        }
    }
    // The table's terminal row always matches; this is unreachable.
    ResolveAnswer {
        action: "not_found",
        status: 404,
        location: None,
        matched_row: "terminal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(path: &'a str, url: Option<&'a str>) -> MatchInput<'a> {
        MatchInput {
            path,
            method: "GET",
            bot: false,
            default_lang_code: "en",
            redirect: None,
            resolved_url: url,
        }
    }

    // Row 1: a declared bot is served or 404'd but NEVER redirected,
    // even when later rows would fire.
    #[test]
    fn row1_bot_never_redirect() {
        let mut i = input("/en/about/", None);
        i.bot = true;
        let a = apply_matcher(&i);
        assert_eq!((a.action, a.status), ("not_found", 404));
        assert_eq!(a.matched_row, "bot-never-redirect");
        // Same request with a serving page resolves to serve.
        let mut i2 = input("/en/about/", Some("/en/about/"));
        i2.bot = true;
        let a2 = apply_matcher(&i2);
        assert_eq!((a2.action, a2.status), ("serve", 200));
    }

    // Row 2: non-GET methods never bounce to 3xx.
    #[test]
    fn row2_post_never_bounce() {
        let mut i = input("/en/about/", None);
        i.method = "POST";
        let a = apply_matcher(&i);
        assert_eq!((a.action, a.status), ("not_found", 404));
        assert_eq!(a.matched_row, "post-never-bounce");
    }

    // Row 3: trailing slash (≠ "/") → 301 minus the slash.
    #[test]
    fn row3_trailing_slash_301() {
        let a = apply_matcher(&input("/about/", Some("/about/")));
        assert_eq!(a.action, "redirect");
        assert_eq!(a.status, 301);
        assert_eq!(a.location.as_deref(), Some("/about"));
        // Root is exempt.
        let root = apply_matcher(&input("/", Some("/")));
        assert_eq!(root.action, "serve");
    }

    // Row 4: "//" collapse.
    #[test]
    fn row4_double_slash_301() {
        let a = apply_matcher(&input("//about//team", Some("//about//team")));
        assert_eq!(a.status, 301);
        assert_eq!(a.location.as_deref(), Some("/about/team"));
    }

    // Row 5: default-lang prefix → 301 unprefixed.
    #[test]
    fn row5_default_lang_301() {
        let a = apply_matcher(&input("/en/about", Some("/en/about")));
        assert_eq!(a.status, 301);
        assert_eq!(a.location.as_deref(), Some("/about"));
        // Non-default prefixes do NOT fire this row.
        let b = apply_matcher(&input("/fr/about", Some("/fr/about")));
        assert_ne!(b.matched_row, "default-lang-301");
    }

    // Row 6: dormant — the alias registry is empty, so no path can
    // match it, and later rows take over.
    #[test]
    fn row6_lang_alias_dormant() {
        assert!(LANG_ALIASES.is_empty());
        let a = apply_matcher(&input("/fr/about", Some("/fr/about")));
        assert_ne!(a.matched_row, "lang-alias-301");
        // Direct predicate check: nothing is alias-prefixed.
        let mut probe = MatchInput {
            path: "/fr/about",
            method: "GET",
            bot: false,
            default_lang_code: "en",
            redirect: None,
            resolved_url: None,
        };
        assert!(!lang_alias_prefixed(&probe));
        probe.path = "/en/about";
        assert!(!lang_alias_prefixed(&probe));
    }

    // Row 7: redirect-table reroute, all four arms.
    #[test]
    fn row7_redirect_table() {
        let mut i = input("/old", None);
        i.redirect = Some(RedirectAnswer {
            redirect_type: "moved_301".into(),
            url_to: Some("/new".into()),
        });
        let a = apply_matcher(&i);
        assert_eq!((a.status, a.location.as_deref()), (301, Some("/new")));

        i.redirect = Some(RedirectAnswer {
            redirect_type: "found_302".into(),
            url_to: Some("/temp".into()),
        });
        assert_eq!(apply_matcher(&i).status, 302);

        i.redirect = Some(RedirectAnswer {
            redirect_type: "alias_308".into(),
            url_to: Some("/canonical".into()),
        });
        assert_eq!(apply_matcher(&i).status, 308);

        i.redirect = Some(RedirectAnswer { redirect_type: "gone_404".into(), url_to: None });
        let gone = apply_matcher(&i);
        assert_eq!((gone.action, gone.status), ("not_found", 404));
    }

    // Row 8: the page resolved at a stored url ≠ the requested one →
    // canonical 301 to the stored url.
    #[test]
    fn row8_canonical_301() {
        let a = apply_matcher(&input("/requested", Some("/stored")));
        assert_eq!(a.matched_row, "canonical-301");
        assert_eq!(a.status, 301);
        assert_eq!(a.location.as_deref(), Some("/stored"));
    }

    // Row 9: terminal — page at the exact url serves, else 404.
    #[test]
    fn row9_terminal() {
        let serve = apply_matcher(&input("/about", Some("/about")));
        assert_eq!((serve.action, serve.matched_row), ("serve", "terminal"));
        let gone = apply_matcher(&input("/missing", None));
        assert_eq!((gone.action, gone.status), ("not_found", 404));
        assert_eq!(gone.matched_row, "terminal");
    }

    // The order itself: row 3 outranks row 4 when both would fire.
    #[test]
    fn order_is_frozen_first_match_wins() {
        let a = apply_matcher(&input("/about//", None));
        assert_eq!(a.matched_row, "trailing-slash-301");
    }
}
