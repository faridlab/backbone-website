//! The ONE slug utility (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! Exactly one `slug_from` serves every slug need in the module — page
//! keys from names, SEO aliases, any future slug-bearing surface in the
//! family. A second slug implementation anywhere downstream is a review
//! refusal. The signature is frozen by spec:
//!
//! - precedence: folded `seo_name`, else folded `fallback_name`, else
//!   the literal `"page"`;
//! - then `-{id}` appended — the FULL simple-form uuid for `Uuid`, the
//!   decimal `abs(id)` for `Int` (the upstream negative-id retry guard);
//! - output is lowercase kebab, capped at 120 characters.

/// The typed identity a slug derives from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugId {
    Uuid(uuid::Uuid),
    Int(i64),
}

/// Maximum slug length (spec-frozen).
pub const SLUG_MAX: usize = 120;

/// The literal base when neither seo_name nor fallback_name carries
/// slug-able bytes.
const BASE_FALLBACK: &str = "page";

/// Slug-able ASCII: lowercase letters, digits, `-`. Uppercase folds to
/// lowercase; everything else acts as a word boundary.
fn slug_char(c: char) -> Option<char> {
    match c {
        'a'..='z' | '0'..='9' => Some(c),
        'A'..='Z' => Some(c.to_ascii_lowercase()),
        '-' => Some('-'),
        _ => None,
    }
}

/// Fold one input string to kebab bytes: map slug-able chars, treat
/// every other char as a separator, collapse separator runs, trim
/// edges. Accents do not transliterate (they fold away — SEO aliases
/// are officer-supplied via `seo_name`).
fn kebab(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_dash = false;
    for c in input.chars() {
        if let Some(mapped) = slug_char(c) {
            if mapped == '-' {
                pending_dash = !out.is_empty();
            } else {
                if pending_dash {
                    out.push('-');
                    pending_dash = false;
                }
                out.push(mapped);
            }
        } else {
            pending_dash = !out.is_empty();
        }
    }
    out
}

/// Derive the slug per the frozen precedence, then append the id arm.
pub fn slug_from(id: SlugId, seo_name: Option<&str>, fallback_name: Option<&str>) -> String {
    let base = seo_name
        .and_then(|s| {
            let k = kebab(s);
            if k.is_empty() { None } else { Some(k) }
        })
        .or_else(|| {
            fallback_name.and_then(|s| {
                let k = kebab(s);
                if k.is_empty() { None } else { Some(k) }
            })
        })
        .unwrap_or_else(|| BASE_FALLBACK.to_string());

    let id_arm = match id {
        SlugId::Uuid(u) => u.to_string(),
        SlugId::Int(i) => i.unsigned_abs().to_string(),
    };

    // "{base}-{id}" capped at SLUG_MAX; never cut mid-separator run.
    let combined = format!("{base}-{id_arm}");
    let mut slug: String = combined.chars().take(SLUG_MAX).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str(BASE_FALLBACK);
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn seo_name_wins_when_slugable() {
        let id = SlugId::Uuid(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap());
        assert_eq!(
            slug_from(id, Some("About Us — Team"), Some("fallback")),
            "about-us-team-00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn fallback_name_second() {
        assert_eq!(
            slug_from(SlugId::Int(42), None, Some("Contact Page")),
            "contact-page-42"
        );
    }

    #[test]
    fn literal_page_when_both_empty() {
        assert_eq!(slug_from(SlugId::Int(7), None, None), "page-7");
        // Un-slug-able inputs count as empty.
        assert_eq!(slug_from(SlugId::Int(7), Some("🌟"), Some("✨")), "page-7");
    }

    #[test]
    fn int_arm_applies_abs_negative_id_guard() {
        // The upstream negative-id retry guard: a negative id slugs to
        // its absolute decimal, never a "-5" fragment.
        assert_eq!(slug_from(SlugId::Int(-5), Some("Home"), None), "home-5");
    }

    #[test]
    fn uuid_arm_is_full_simple_form() {
        let u = Uuid::parse_str("6f9619ff-8b86-d011-b42d-00c04fc964ff").unwrap();
        assert_eq!(
            slug_from(SlugId::Uuid(u), None, None),
            "page-6f9619ff-8b86-d011-b42d-00c04fc964ff"
        );
    }

    #[test]
    fn output_is_lowercase_kebab() {
        assert_eq!(slug_from(SlugId::Int(3), Some("  --Hello,,  World--  "), None), "hello-world-3");
    }

    #[test]
    fn capped_at_120_chars() {
        let long = "a".repeat(400);
        let out = slug_from(SlugId::Int(1), Some(&long), None);
        assert!(out.len() <= SLUG_MAX);
        assert!(!out.ends_with('-'));
        assert!(out.starts_with('a'));
    }

    #[test]
    fn canonical_301_arm_prefers_seo_name() {
        // The redirect-canonical slug (moved pages): seo_name wins over
        // the raw page title fallback.
        let u = Uuid::nil();
        assert_eq!(slug_from(SlugId::Uuid(u), Some("Old Prices"), Some("prices v2")),
            "old-prices-00000000-0000-0000-0000-000000000000");
    }
}
