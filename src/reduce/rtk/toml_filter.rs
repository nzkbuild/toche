//! Applies TOML-defined filter rules to command output.
//! Vendored and adapted from RTK (Apache-2.0) for Toche 0.5.0.
//!
//! Provides a declarative pipeline of 8 stages that can be configured
//! via TOML files. Only the built-in filter set (63 filters from
//! vendor_reuse/rtk/src/filters/*.toml, concatenated by build.rs) is
//! loaded — disk-based project/user filters are not supported in Toche.
//!
//! Pipeline stages (applied in order):
//!   1. strip_ansi           — remove ANSI escape codes
//!   2. replace              — regex substitutions, line-by-line, chainable
//!   3. match_output         — short-circuit: if blob matches a pattern, return message immediately
//!   4. strip/keep_lines     — filter lines by regex
//!   5. truncate_lines_at    — truncate each line to N chars
//!   6. head/tail_lines      — keep first/last N lines
//!   7. max_lines            — absolute line cap
//!   8. on_empty             — message if result is empty

use super::constants::RTK_META_COMMANDS;
use lazy_static::lazy_static;
use regex::{Regex, RegexSet};
use serde::Deserialize;
use std::collections::BTreeMap;

// Built-in filters: concatenated from vendor_reuse/rtk/src/filters/*.toml by build.rs.
const BUILTIN_TOML: &str = include_str!(concat!(env!("OUT_DIR"), "/builtin_filters.toml"));

// ---------------------------------------------------------------------------
// Deserialization types (TOML schema)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchOutputRule {
    pattern: String,
    message: String,
    #[serde(default)]
    unless: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceRule {
    pattern: String,
    replacement: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TomlFilterTestDef {
    pub name: String,
    pub input: String,
    pub expected: String,
}

#[derive(Deserialize)]
struct TomlFilterFile {
    schema_version: u32,
    #[serde(default)]
    filters: BTreeMap<String, TomlFilterDef>,
    #[serde(default)]
    tests: BTreeMap<String, Vec<TomlFilterTestDef>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlFilterDef {
    description: Option<String>,
    match_command: String,
    #[serde(default)]
    strip_ansi: bool,
    #[serde(default)]
    replace: Vec<ReplaceRule>,
    #[serde(default)]
    match_output: Vec<MatchOutputRule>,
    #[serde(default)]
    strip_lines_matching: Vec<String>,
    #[serde(default)]
    keep_lines_matching: Vec<String>,
    truncate_lines_at: Option<usize>,
    head_lines: Option<usize>,
    tail_lines: Option<usize>,
    max_lines: Option<usize>,
    on_empty: Option<String>,
    #[serde(default)]
    filter_stderr: bool,
}

// ---------------------------------------------------------------------------
// Compiled types (post-validation, ready to use)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct CompiledMatchOutputRule {
    pattern: Regex,
    message: String,
    unless: Option<Regex>,
}

#[derive(Debug)]
struct CompiledReplaceRule {
    pattern: Regex,
    replacement: String,
}

#[derive(Debug)]
enum LineFilter {
    None,
    Strip(RegexSet),
    Keep(RegexSet),
}

/// A filter that has been parsed and compiled — all regexes are ready.
#[derive(Debug)]
pub struct CompiledFilter {
    pub name: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    match_regex: Regex,
    strip_ansi: bool,
    replace: Vec<CompiledReplaceRule>,
    match_output: Vec<CompiledMatchOutputRule>,
    line_filter: LineFilter,
    truncate_lines_at: Option<usize>,
    head_lines: Option<usize>,
    tail_lines: Option<usize>,
    pub max_lines: Option<usize>,
    on_empty: Option<String>,
    /// When true, stderr should be captured and merged with stdout.
    pub filter_stderr: bool,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct TomlFilterRegistry {
    pub filters: Vec<CompiledFilter>,
}

impl TomlFilterRegistry {
    /// Load registry from built-in filters only (no disk paths in Toche).
    fn load() -> Self {
        let mut filters = Vec::new();

        match Self::parse_and_compile(BUILTIN_TOML, "builtin") {
            Ok(f) => filters.extend(f),
            Err(e) => eprintln!("[toche:reduce] warning: builtin filters: {}", e),
        }

        TomlFilterRegistry { filters }
    }

    pub fn parse_and_compile(content: &str, source: &str) -> Result<Vec<CompiledFilter>, String> {
        let file: TomlFilterFile = toml::from_str(content)
            .map_err(|e| format!("TOML parse error in {}: {}", source, e))?;

        if file.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {} in {} (expected 1)",
                file.schema_version, source
            ));
        }

        let mut compiled = Vec::new();
        for (name, def) in file.filters {
            match compile_filter(name.clone(), def) {
                Ok(f) => compiled.push(f),
                Err(e) => eprintln!(
                    "[toche:reduce] warning: filter '{}' in {}: {}",
                    name, source, e
                ),
            }
        }
        Ok(compiled)
    }
}

/// Commands already handled by dedicated Rust modules (not relevant for Toche
/// since we only use the TOML engine, but kept for filter validation warnings).
const RUST_HANDLED_COMMANDS: &[&str] = &[
    "ls",
    "tree",
    "read",
    "smart",
    "git",
    "gh",
    "aws",
    "psql",
    "pnpm",
    "err",
    "test",
    "json",
    "deps",
    "env",
    "find",
    "diff",
    "log",
    "docker",
    "kubectl",
    "summary",
    "grep",
    "init",
    "wget",
    "wc",
    "gain",
    "config",
    "vitest",
    "prisma",
    "tsc",
    "next",
    "lint",
    "prettier",
    "format",
    "playwright",
    "cargo",
    "npm",
    "npx",
    "curl",
    "discover",
    "ruff",
    "pytest",
    "mypy",
    "pip",
    "go",
    "golangci-lint",
    "rewrite",
    "proxy",
    "verify",
    "learn",
];

pub fn is_rtk_reserved_command(name: &str) -> bool {
    RUST_HANDLED_COMMANDS.contains(&name) || RTK_META_COMMANDS.contains(&name)
}

fn compile_filter(name: String, def: TomlFilterDef) -> Result<CompiledFilter, String> {
    if !def.strip_lines_matching.is_empty() && !def.keep_lines_matching.is_empty() {
        return Err("strip_lines_matching and keep_lines_matching are mutually exclusive".into());
    }

    let match_regex = Regex::new(&def.match_command)
        .map_err(|e| format!("invalid match_command regex: {}", e))?;

    // Warn if match_command matches a Rust-handled command (informational)
    for cmd in RUST_HANDLED_COMMANDS {
        if match_regex.is_match(cmd) {
            eprintln!(
                "[toche:reduce] warning: filter '{}' match_command matches '{}' which is \
                 already handled by a Rust module — this filter will never activate for that command",
                name, cmd
            );
            break;
        }
    }

    let replace = def
        .replace
        .into_iter()
        .map(|r| {
            let pat = r.pattern.clone();
            Regex::new(&r.pattern)
                .map(|pattern| CompiledReplaceRule {
                    pattern,
                    replacement: r.replacement,
                })
                .map_err(|e| format!("invalid replace pattern '{}': {}", pat, e))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let match_output = def
        .match_output
        .into_iter()
        .map(|r| -> Result<CompiledMatchOutputRule, String> {
            let pat = r.pattern.clone();
            let pattern = Regex::new(&r.pattern)
                .map_err(|e| format!("invalid match_output pattern '{}': {}", pat, e))?;
            let unless = r
                .unless
                .as_deref()
                .map(|u| {
                    Regex::new(u)
                        .map_err(|e| format!("invalid match_output unless pattern '{}': {}", u, e))
                })
                .transpose()?;
            Ok(CompiledMatchOutputRule {
                pattern,
                message: r.message,
                unless,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let line_filter = if !def.strip_lines_matching.is_empty() {
        let set = RegexSet::new(&def.strip_lines_matching)
            .map_err(|e| format!("invalid strip_lines_matching regex: {}", e))?;
        LineFilter::Strip(set)
    } else if !def.keep_lines_matching.is_empty() {
        let set = RegexSet::new(&def.keep_lines_matching)
            .map_err(|e| format!("invalid keep_lines_matching regex: {}", e))?;
        LineFilter::Keep(set)
    } else {
        LineFilter::None
    };

    Ok(CompiledFilter {
        name,
        description: def.description,
        match_regex,
        strip_ansi: def.strip_ansi,
        replace,
        match_output,
        line_filter,
        truncate_lines_at: def.truncate_lines_at,
        head_lines: def.head_lines,
        tail_lines: def.tail_lines,
        max_lines: def.max_lines,
        on_empty: def.on_empty,
        filter_stderr: def.filter_stderr,
    })
}

// ---------------------------------------------------------------------------
// Singleton (lazy-loaded, one-time cost)
// ---------------------------------------------------------------------------

lazy_static! {
    static ref REGISTRY: TomlFilterRegistry = TomlFilterRegistry::load();
}

lazy_static! {
    static ref MATCH_SET: RegexSet = build_match_set();
}

pub fn command_matches_filter(command: &str) -> bool {
    MATCH_SET.is_match(command)
}

fn build_match_set() -> RegexSet {
    let patterns = match_patterns_in(BUILTIN_TOML);
    RegexSet::new(&patterns).unwrap_or_else(|_| {
        let valid: Vec<String> = patterns
            .into_iter()
            .filter(|p| Regex::new(p).is_ok())
            .collect();
        RegexSet::new(&valid).unwrap_or_else(|_| RegexSet::empty())
    })
}

fn match_patterns_in(content: &str) -> Vec<String> {
    match toml::from_str::<TomlFilterFile>(content) {
        Ok(file) if file.schema_version == 1 => file
            .filters
            .into_values()
            .map(|def| def.match_command)
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Public API — pure functions (testable without global state)
// ---------------------------------------------------------------------------

/// Find the first matching filter in a slice. O(N) on the number of filters.
pub fn find_filter_in<'a>(
    command: &str,
    filters: &'a [CompiledFilter],
) -> Option<&'a CompiledFilter> {
    filters.iter().find(|f| f.match_regex.is_match(command))
}

/// Apply a compiled filter pipeline to raw stdout. Pure String -> String.
pub fn apply_filter(filter: &CompiledFilter, stdout: &str) -> String {
    apply_filter_with_info(filter, stdout).0
}

#[derive(Debug, PartialEq)]
pub enum Lossiness {
    None,
    /// `tail -n +{tail_offset}` over `tee_payload` reproduces the dropped lines,
    /// up to the tee `max_file_size` cap.
    Tail {
        tee_payload: String,
        tail_offset: usize,
    },
    Whole,
}

pub fn apply_filter_with_info(filter: &CompiledFilter, stdout: &str) -> (String, Lossiness) {
    let mut lines: Vec<String> = stdout.lines().map(String::from).collect();

    // 1. strip_ansi
    if filter.strip_ansi {
        lines = lines
            .into_iter()
            .map(|l| super::utils::strip_ansi(&l))
            .collect();
    }

    // 2. replace — line-by-line, rules chained sequentially
    if !filter.replace.is_empty() {
        lines = lines
            .into_iter()
            .map(|mut line| {
                for rule in &filter.replace {
                    line = rule
                        .pattern
                        .replace_all(&line, rule.replacement.as_str())
                        .into_owned();
                }
                line
            })
            .collect();
    }

    // 3. match_output — short-circuit on full blob match (first rule wins)
    if !filter.match_output.is_empty() {
        let blob = lines.join("\n");
        for rule in &filter.match_output {
            if rule.pattern.is_match(&blob) {
                if let Some(ref unless_re) = rule.unless {
                    if unless_re.is_match(&blob) {
                        continue;
                    }
                }
                return (rule.message.clone(), Lossiness::Whole);
            }
        }
    }

    // 4. strip OR keep (mutually exclusive)
    match &filter.line_filter {
        LineFilter::Strip(set) => lines.retain(|l| !set.is_match(l)),
        LineFilter::Keep(set) => lines.retain(|l| set.is_match(l)),
        LineFilter::None => {}
    }

    // 5. truncate_lines_at — uses utils::truncate (unicode-safe)
    let mut intra_line_loss = false;
    if let Some(max_chars) = filter.truncate_lines_at {
        lines = lines
            .into_iter()
            .map(|line| {
                let truncated = super::utils::truncate(&line, max_chars);
                if truncated != line {
                    intra_line_loss = true;
                }
                truncated
            })
            .collect();
    }

    let snapshot_for_tail = !intra_line_loss
        && filter.tail_lines.is_none()
        && (filter.head_lines.is_some() || filter.max_lines.is_some());
    let pre_cut = snapshot_for_tail.then(|| lines.clone());

    // 6. head + tail
    let total = lines.len();
    let mut noncontiguous_drop = false;
    let mut head_cut: Option<usize> = None;
    if let (Some(head), Some(tail)) = (filter.head_lines, filter.tail_lines) {
        if total > head + tail {
            let mut result = lines[..head].to_vec();
            result.push(format!("... ({} lines omitted)", total - head - tail));
            result.extend_from_slice(&lines[total - tail..]);
            lines = result;
            noncontiguous_drop = true;
        }
    } else if let Some(head) = filter.head_lines {
        if total > head {
            lines.truncate(head);
            lines.push(format!("... ({} lines omitted)", total - head));
            head_cut = Some(head);
        }
    } else if let Some(tail) = filter.tail_lines {
        if total > tail {
            let omitted = total - tail;
            lines = lines[omitted..].to_vec();
            lines.insert(0, format!("... ({} lines omitted)", omitted));
            noncontiguous_drop = true;
        }
    }

    // 7. max_lines — absolute cap applied after head/tail (includes omit messages)
    let mut max_cut: Option<usize> = None;
    if let Some(max) = filter.max_lines {
        if lines.len() > max {
            let dropped = lines.len() - max;
            lines.truncate(max);
            lines.push(format!("... ({} lines truncated)", dropped));
            max_cut = Some(max);
        }
    }

    // 8. on_empty
    let result = lines.join("\n");
    if result.trim().is_empty() {
        if let Some(ref msg) = filter.on_empty {
            return (msg.clone(), Lossiness::None);
        }
    }

    let loss = if let Some(snapshot) = pre_cut {
        match (head_cut, max_cut) {
            (Some(_), Some(_)) => Lossiness::Whole,
            (Some(head), None) => Lossiness::Tail {
                tee_payload: snapshot.join("\n"),
                tail_offset: head + 1,
            },
            (None, Some(max)) => Lossiness::Tail {
                tee_payload: snapshot.join("\n"),
                tail_offset: max + 1,
            },
            (None, None) => Lossiness::None,
        }
    } else if noncontiguous_drop || intra_line_loss || head_cut.is_some() || max_cut.is_some() {
        Lossiness::Whole
    } else {
        Lossiness::None
    };

    (result, loss)
}

// ---------------------------------------------------------------------------
// Convenience wrapper (uses singleton)
// ---------------------------------------------------------------------------

/// Find a matching filter from the global registry.
pub fn find_matching_filter(command: &str) -> Option<&'static CompiledFilter> {
    find_filter_in(command, &REGISTRY.filters)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_filters(toml: &str) -> Vec<CompiledFilter> {
        TomlFilterRegistry::parse_and_compile(toml, "test").expect("test TOML should be valid")
    }

    fn first_filter(toml: &str) -> CompiledFilter {
        make_filters(toml)
            .into_iter()
            .next()
            .expect("expected at least one filter")
    }

    #[test]
    fn command_matches_filter_agrees_with_find_matching_filter() {
        for cmd in ["jj log", "jq .", "frobnicate xyz", "cd /tmp"] {
            assert_eq!(
                command_matches_filter(cmd),
                find_matching_filter(cmd).is_some(),
                "match-set disagreed with registry for {cmd:?}"
            );
        }
    }

    #[test]
    fn test_builtin_filters_compile() {
        let builtin = BUILTIN_TOML;
        let result = TomlFilterRegistry::parse_and_compile(builtin, "builtin");
        assert!(
            result.is_ok(),
            "builtin filters failed to compile: {:?}",
            result
        );
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_builtin_filter_count() {
        let filters = make_filters(BUILTIN_TOML);
        assert_eq!(
            filters.len(),
            63,
            "Expected exactly 63 built-in filters, got {}.",
            filters.len()
        );
    }

    #[test]
    fn test_builtin_toml_has_schema_version() {
        assert!(BUILTIN_TOML.contains("schema_version = 1"));
    }

    #[test]
    fn test_find_filter_matches_terraform() {
        let filters = make_filters(
            r#"
schema_version = 1
[filters.terraform-plan]
match_command = "^terraform\\s+plan"
strip_ansi = true
"#,
        );
        let found = find_filter_in("terraform plan -out=tfplan", &filters);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "terraform-plan");
    }

    #[test]
    fn test_find_filter_no_match_returns_none() {
        let filters = make_filters(
            r#"
schema_version = 1
[filters.f]
match_command = "^terraform"
"#,
        );
        let found = find_filter_in("kubectl get pods", &filters);
        assert!(found.is_none());
    }

    #[test]
    fn test_strip_ansi_removes_codes() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
strip_ansi = true
"#,
        );
        let out = apply_filter(&f, "\x1b[31mError\x1b[0m\nnormal");
        assert_eq!(out, "Error\nnormal");
    }

    #[test]
    fn test_head_lines() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
head_lines = 2
"#,
        );
        let input = "a\nb\nc\nd\ne";
        let out = apply_filter(&f, input);
        assert!(out.starts_with("a\nb\n"));
        assert!(out.contains("3 lines omitted"));
    }

    #[test]
    fn test_empty_filter_passthrough() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
"#,
        );
        let input = "line1\nline2\nline3";
        let out = apply_filter(&f, input);
        assert_eq!(out, input);
    }

    #[test]
    fn test_full_pipeline_order() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
strip_ansi = true
strip_lines_matching = ["^noise"]
truncate_lines_at = 10
head_lines = 3
max_lines = 4
on_empty = "empty"
"#,
        );
        let input =
            "\x1b[31mred line\x1b[0m\nnoise skip\nkeep one\nkeep two\nkeep three\nkeep four";
        let out = apply_filter(&f, input);
        assert!(out.contains("red line"));
        assert!(!out.contains("noise skip"));
        assert!(out.contains("lines omitted") || out.contains("lines truncated"));
    }

    #[test]
    fn test_keep_lines_matching_basic() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
keep_lines_matching = ["^PASS", "^FAIL"]
"#,
        );
        let input = "PASS test_a\nsome noise\nFAIL test_b\nmore noise";
        let out = apply_filter(&f, input);
        assert_eq!(out, "PASS test_a\nFAIL test_b");
    }

    #[test]
    fn test_on_empty_when_all_filtered() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
strip_lines_matching = [".*"]
on_empty = "nothing left"
"#,
        );
        let out = apply_filter(&f, "line1\nline2");
        assert_eq!(out, "nothing left");
    }

    #[test]
    fn test_empty_input() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
strip_lines_matching = [".*"]
"#,
        );
        let out = apply_filter(&f, "");
        assert_eq!(out, "");
    }

    #[test]
    fn test_unicode_preserved() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
strip_lines_matching = ["^noise"]
"#,
        );
        let out = apply_filter(&f, "日本語テスト\nnoise\n中文内容");
        assert_eq!(out, "日本語テスト\n中文内容");
    }

    #[test]
    fn test_loss_head_lines_is_tail() {
        let toml = "schema_version = 1\n[filters.f]\nmatch_command = \"^cmd\"\nhead_lines = 2\n";
        let (out, loss) = apply_filter_with_info(&first_filter(toml), "a\nb\nc\nd\ne");
        assert!(out.starts_with("a\nb\n"));
        match loss {
            Lossiness::Tail {
                tee_payload,
                tail_offset,
            } => {
                assert_eq!(tail_offset, 3);
                let recovered: Vec<&str> = tee_payload.lines().skip(tail_offset - 1).collect();
                assert_eq!(recovered, vec!["c", "d", "e"]);
            }
            other => panic!("expected Tail, got {:?}", other),
        }
    }

    #[test]
    fn test_match_output_basic_short_circuit() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
match_output = [
  { pattern = "Switched to branch", message = "ok" },
]
"#,
        );
        let out = apply_filter(&f, "Switched to branch 'main'");
        assert_eq!(out, "ok");
    }

    #[test]
    fn test_replace_basic_all_occurrences() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^cmd"
replace = [
  { pattern = "foo", replacement = "bar" },
]
"#,
        );
        let out = apply_filter(&f, "foo baz foo\nfoo");
        assert_eq!(out, "bar baz bar\nbar");
    }

    #[test]
    fn test_match_output_unless_blocks_short_circuit_when_errors_present() {
        let f = first_filter(
            r#"
schema_version = 1
[filters.f]
match_command = "^rsync"
match_output = [
  { pattern = "total size is", message = "ok (synced)", unless = "error|failed" },
]
"#,
        );
        let input = "rsync: [sender] error\ntotal size is 1000  speedup is 3.33\n";
        let out = apply_filter(&f, input);
        assert_ne!(out.trim(), "ok (synced)");
        assert!(out.contains("error"));
    }
}
