//! The one-resolver grep invariant (spec §2, probe class 1's filesystem
//! assertion): the generic-vs-specific fold is DEFINED in exactly one
//! file — `src/application/service/specificity.rs`.
//!
//! Two literal assertions, walked over the crate's `src/` tree at test
//! time (a source-tree probe: no database needed, and a violation fails
//! the suite regardless of which behavior test would have caught it):
//!
//! 1. the fold's predicate text `website_id IS NULL` occurs in NO file
//!    under `src/` except `specificity.rs` (the NULL arm of the fold is
//!    never re-written by hand anywhere else);
//! 2. the fold's entry points are never re-implemented: `fn resolve_specific`
//!    and `fn resolve_page_by_url` definitions exist ONLY in
//!    `specificity.rs`.
//!
//! Naming note (the spec's own §2 vs §9.4 seam): the exported trait's
//! frozen method `resolve_website_by_host` (§9.4 — the artifact name
//! downstream modules hold this crate to) is the ONE sanctioned
//! `fn resolve_` name outside `specificity.rs`, and only in the trait
//! file `website_surface.rs` (declaration + the impl that delegates).
//! The probe asserts exactly that shape — any OTHER `fn resolve_`
//! anywhere is a failure. The shared hostname helper therefore carries
//! a different name (`website_by_host`), keeping this invariant literal.

use std::path::{Path, PathBuf};

/// The one file the fold lives in.
const RESOLVER_FILE: &str = "specificity.rs";
/// The trait file carrying the sanctioned §9.4 method name.
const SURFACE_FILE: &str = "website_surface.rs";
/// The frozen trait artifact name sanctioned by §9.4.
const SANCTIONED_NAME: &str = "resolve_website_by_host";

fn src_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest).join("src")
}

/// Walk every `.rs` file under `src/` (sorted for deterministic
/// failure messages).
fn rust_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
            Err(e) => panic!("PROBE-FAIL: cannot read {}: {e}", dir.display()),
        };
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    files
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

/// The fold's NULL-arm predicate text lives ONLY in the resolver file.
#[test]
fn probe_fold_null_arm_one_file() {
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files(&src_root()) {
        if file_name(&path) == RESOLVER_FILE {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("PROBE-FAIL: cannot read {}: {e}", path.display()));
        for (no, line) in text.lines().enumerate() {
            if line.contains("website_id IS NULL") {
                offenders.push(format!("{}:{}: {}", path.display(), no + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the `website_id IS NULL` fold arm must exist only in {RESOLVER_FILE}; offenders:\n{}",
        offenders.join("\n")
    );
}

/// The fold's entry points are defined only in the resolver file.
#[test]
fn probe_fold_entry_points_not_reimplemented() {
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files(&src_root()) {
        if file_name(&path) == RESOLVER_FILE {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("PROBE-FAIL: cannot read {}: {e}", path.display()));
        for (no, line) in text.lines().enumerate() {
            if line.contains("fn resolve_specific") || line.contains("fn resolve_page_by_url") {
                offenders.push(format!("{}:{}: {}", path.display(), no + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "`fn resolve_specific` / `fn resolve_page_by_url` may be defined only in {RESOLVER_FILE}; offenders:\n{}",
        offenders.join("\n")
    );
}

/// Every other `fn resolve_` name is a failure: outside the resolver
/// file itself, the ONLY sanctioned occurrence is the §9.4 frozen trait
/// method, and only in the trait file (declaration + delegating impl).
#[test]
fn probe_no_other_resolve_fns_anywhere() {
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files(&src_root()) {
        // The resolver file legitimately defines the `resolve_*` family.
        if file_name(&path) == RESOLVER_FILE {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("PROBE-FAIL: cannot read {}: {e}", path.display()));
        for (no, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let is_resolve_fn = [
                "pub async fn resolve_",
                "async fn resolve_",
                "pub fn resolve_",
                "fn resolve_",
            ]
            .iter()
            .any(|p| trimmed.starts_with(p));
            if !is_resolve_fn {
                continue;
            }
            // Extract the FULL fn name (the `fn ` prefix goes, the
            // `resolve_` stem stays — the sanctioned comparison is
            // against the whole name).
            let after_fn = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("async ")
                .strip_prefix("fn ")
                .unwrap_or(trimmed);
            let name: String = after_fn
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let sanctioned = name == SANCTIONED_NAME && file_name(&path) == SURFACE_FILE;
            if !sanctioned {
                offenders.push(format!("{}:{}: {}", path.display(), no + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "no `fn resolve_*` outside {RESOLVER_FILE} except the frozen §9.4 trait method `{SANCTIONED_NAME}` in {SURFACE_FILE}; offenders:\n{}",
        offenders.join("\n")
    );
}

/// The sanctioned name really is the frozen trait shape: declared once
/// and implemented once in the trait file, nowhere else.
#[test]
fn probe_sanctioned_name_is_the_trait_pair_only() {
    let surface = src_root().join("application").join("service").join(SURFACE_FILE);
    let text = std::fs::read_to_string(&surface)
        .unwrap_or_else(|e| panic!("PROBE-FAIL: cannot read {}: {e}", surface.display()));
    let occurrences = text
        .lines()
        .filter(|l| l.trim_start().starts_with("async fn resolve_website_by_host"))
        .count();
    assert_eq!(
        occurrences, 2,
        "the trait file must carry exactly the declaration + the delegating impl of `{SANCTIONED_NAME}`"
    );
}
