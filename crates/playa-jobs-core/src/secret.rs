//! Provider-credential lookup helper.
//!
//! Every [`crate::JobProvider`] tends to need an API key. Each provider crate
//! re-implementing env-or-`.env` lookup is the kind of duplication the audit
//! flagged elsewhere; this module is the single source.
//!
//! # Order
//!
//! 1. Process environment, in the order of `env_keys`. First non-empty wins.
//! 2. Each path in `file_paths`, scanned in order. First file that exists and
//!    contains any of `env_keys` wins; values are returned **without** mutating
//!    the process environment (Rust 2024's `std::env::set_var` is `unsafe`,
//!    and quietly poisoning a parent process's env is a footgun anyway).
//!
//! # `.env` parser shape
//!
//! Tolerates:
//! - Trailing newline or none (matches a vanilla "echo KEY=VAL > .env").
//! - CRLF (Windows) line endings — `str::lines` handles them.
//! - `# comment` lines and blank lines.
//! - Surrounding `"..."` or `'...'` quotes around the value.
//!
//! Does **not** support:
//! - `export FOO=bar` shell prefix — strip manually if present.
//! - Inline comments after the value (e.g. `KEY=val # note`) — the `#` is
//!   treated as part of the value.
//! - Multi-line values (HEREDOC).

use std::path::Path;

/// Walk env vars then files, return the first non-empty match.
pub fn lookup<P: AsRef<Path>>(env_keys: &[&str], file_paths: &[P]) -> Option<String> {
    for key in env_keys {
        if let Ok(v) = std::env::var(key)
            && !v.is_empty()
        {
            return Some(v);
        }
    }
    for path in file_paths {
        if let Some(v) = parse_env_file(path.as_ref(), env_keys) {
            return Some(v);
        }
    }
    None
}

/// Read `path` and return the first value whose key appears in `wanted_keys`.
/// Does not mutate the process environment. Returns `None` for any read error
/// (file missing, unreadable, no matching key).
pub fn parse_env_file(path: &Path, wanted_keys: &[&str]) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        if !wanted_keys.iter().any(|w| *w == key) {
            // Keep scanning — a later line might match.
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(value)
            .to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn tmp_env(tag: &str, body: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("playa-jobs-secret-{tag}-{nanos}.env"));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        path
    }

    #[test]
    fn finds_canonical_key() {
        let p = tmp_env("canon", "FAL_KEY=abc123\n");
        let v = parse_env_file(&p, &["FAL_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("abc123".to_string()));
    }

    #[test]
    fn finds_synonym_when_canonical_absent() {
        let p = tmp_env("syn", "FAL_API_KEY=zzz\n");
        let v = parse_env_file(&p, &["FAL_KEY", "FAL_API_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("zzz".to_string()));
    }

    #[test]
    fn handles_no_trailing_newline() {
        // Real-world: `echo -n KEY=VAL > .env` from PowerShell or notepad
        // without a final newline produces this exact shape.
        let p = tmp_env("nonl", "FAL_API_KEY=xyz");
        let v = parse_env_file(&p, &["FAL_API_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("xyz".to_string()));
    }

    #[test]
    fn handles_crlf_line_endings() {
        let p = tmp_env("crlf", "FAL_KEY=one\r\nFAL_API_KEY=two\r\n");
        let v = parse_env_file(&p, &["FAL_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("one".to_string()));
    }

    #[test]
    fn strips_double_quotes() {
        let p = tmp_env("dq", "FAL_KEY=\"quoted\"\n");
        let v = parse_env_file(&p, &["FAL_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("quoted".to_string()));
    }

    #[test]
    fn strips_single_quotes() {
        let p = tmp_env("sq", "FAL_KEY='quoted'\n");
        let v = parse_env_file(&p, &["FAL_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("quoted".to_string()));
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let p = tmp_env(
            "cmt",
            "# header\n\nUNRELATED=x\nFAL_KEY=value\n# trailer\n",
        );
        let v = parse_env_file(&p, &["FAL_KEY"]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("value".to_string()));
    }

    #[test]
    fn returns_none_when_no_matching_key() {
        let p = tmp_env("none", "OTHER_KEY=irrelevant\n");
        let v = parse_env_file(&p, &["FAL_KEY", "FAL_API_KEY"]);
        std::fs::remove_file(&p).ok();
        assert!(v.is_none());
    }

    #[test]
    fn empty_value_treated_as_missing() {
        let p = tmp_env("empty", "FAL_KEY=\n");
        let v = parse_env_file(&p, &["FAL_KEY"]);
        std::fs::remove_file(&p).ok();
        assert!(v.is_none());
    }

    #[test]
    fn missing_file_returns_none() {
        let v = parse_env_file(Path::new("/nonexistent/.env"), &["FAL_KEY"]);
        assert!(v.is_none());
    }

    #[test]
    fn lookup_reads_env_file_when_no_env_var_set() {
        // Use a key name nobody else sets to avoid contaminating concurrent
        // tests.
        let key = "PLAYA_TEST_FAL_KEY_XYZ_UNIQUE";
        // Sanity: ensure it really is unset.
        assert!(std::env::var(key).is_err());
        let p = tmp_env("lookup", &format!("{key}=from-file\n"));
        let v = lookup(&[key], &[p.as_path()]);
        std::fs::remove_file(&p).ok();
        assert_eq!(v, Some("from-file".to_string()));
    }

    #[test]
    fn lookup_returns_none_when_neither_env_nor_file_has_key() {
        let key = "PLAYA_TEST_NEVER_SET_KEY";
        assert!(std::env::var(key).is_err());
        let p = tmp_env("lookup_none", "OTHER=x\n");
        let v = lookup(&[key], &[p.as_path()]);
        std::fs::remove_file(&p).ok();
        assert!(v.is_none());
    }
}
