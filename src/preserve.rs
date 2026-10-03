//! Text-preserve rules: regex hits become `[CTRL_n]` before the model sees them.
//!
//! Built-ins cover named RMMZ controls, brace and printf placeholders, and
//! literal control-mask tokens. Ren'Py adds interpolation; markup adapters add
//! HTML/XML tags. Workspace rules append patterns. Overlapping hits keep the
//! leftmost-longest span.

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const PRESERVE_FILE: &str = "preserve.toml";
pub const PRESERVE_VERSION: u32 = 1;

/// RMMZ controls, including named plugin controls with single-line arguments.
pub const RMMZ_CONTROL_PATTERN: &str = r"(?x)
        \\{2}                                   # escaped backslash
        | \\[A-Za-z]+\[[^\]\r\n]*\]            # named controls, including plugin arguments
        | \\[!.>|{\}\\\$\^]                   # single-character controls
        | \\[A-Za-z]                            # bare letter control
        ";

const BRACE_PLACEHOLDER: &str = r"\{\{\s*[A-Za-z_][A-Za-z0-9_.]*\s*\}\}|\{[A-Za-z_][A-Za-z0-9_]*\}|\{\d+(?:,-?\d+)?(?::[^{}\r\n]+)?\}";
const PRINTF_PLACEHOLDER: &str = r"%(?:\d+\$)?[-+#0]*(?:\d+|\*(?:\d+\$)?)?(?:\.(?:\d+|\*(?:\d+\$)?))?(?:hh|ll|[hlLjzt])?[diuoxXfFeEgGaAcspn]|%%";
const MASK_TOKEN: &str = r"\[CTRL_\d+\]";
const INLINE_TAG: &str = r#"</?[A-Za-z][A-Za-z0-9:_-]*(?:\s+[A-Za-z_:][A-Za-z0-9_:.-]*(?:\s*=\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s<>"'=]+))?)*\s*/?>"#;
const RENPY_BRACKET: &str = r"\[[A-Za-z_][A-Za-z0-9_]*(?:![a-z]+)?\]";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRule {
    pub pattern: String,
    #[serde(default)]
    pub info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreserveFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default, rename = "rule")]
    rules: Vec<FileRule>,
}

fn default_version() -> u32 {
    PRESERVE_VERSION
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleInfo {
    pub pattern: String,
    pub info: String,
    pub source: String,
}

#[derive(Debug, Clone)]
struct Compiled {
    pattern: String,
    info: String,
    source: String,
    re: Regex,
}

#[derive(Debug, Clone)]
pub struct PreserveSet {
    rules: Vec<Compiled>,
}

impl PreserveSet {
    fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Engine controls, brace/printf placeholders, and literal mask tokens.
    pub fn core() -> &'static Self {
        static SET: LazyLock<PreserveSet> = LazyLock::new(|| {
            let mut s = PreserveSet::empty();
            s.push_builtin(RMMZ_CONTROL_PATTERN, "rmmz control codes");
            s.push_builtin(BRACE_PLACEHOLDER, "brace placeholder");
            s.push_builtin(PRINTF_PLACEHOLDER, "printf placeholder");
            s.push_builtin(MASK_TOKEN, "literal control-mask token");
            s
        });
        &SET
    }
    pub fn for_engine(engine: &str) -> Self {
        let mut s = Self::core().clone();
        if engine == "renpy" {
            s.push_builtin(RENPY_BRACKET, "renpy interpolation");
        }
        if matches!(engine, "auto" | "epub" | "html" | "xml" | "md" | "docx" | "srt" | "vtt") {
            s.push_builtin(INLINE_TAG, "inline HTML/XML tag");
        }
        s
    }

    fn push_builtin(&mut self, pattern: &str, info: &str) {
        match compile_rule(pattern) {
            Ok(re) => self.rules.push(Compiled {
                pattern: pattern.to_string(),
                info: info.into(),
                source: "builtin".into(),
                re,
            }),
            Err(e) => panic!("builtin preserve pattern must compile: {e:#}"),
        }
    }

    fn merge_file(&mut self, path: &Path) {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return;
        };
        let parsed: PreserveFile = match toml::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("preserve: ignoring malformed {}: {e:#}", path.display());
                return;
            }
        };
        for r in parsed.rules {
            match compile_rule(&r.pattern) {
                Ok(re) => self.rules.push(Compiled {
                    pattern: r.pattern,
                    info: r.info,
                    source: "workspace".into(),
                    re,
                }),
                Err(e) => eprintln!(
                    "preserve: skip rule `{}`: {e:#}",
                    r.pattern
                ),
            }
        }
    }

    pub fn rules(&self) -> Vec<RuleInfo> {
        self.rules
            .iter()
            .map(|r| RuleInfo {
                pattern: r.pattern.clone(),
                info: r.info.clone(),
                source: r.source.clone(),
            })
            .collect()
    }

    fn literal_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        for rule in &self.rules {
            spans.extend(rule.re.find_iter(text).filter(|hit| !hit.is_empty()).map(|hit| (hit.start(), hit.end())));
        }
        spans.sort_unstable_by_key(|&(start, end)| (start, std::cmp::Reverse(end - start)));
        let mut end = 0;
        spans.retain(|&(start, next)| {
            if start < end { return false }
            end = next;
            true
        });
        spans
    }

    fn literals<'a>(&self, lines: &'a [String]) -> Vec<&'a str> {
        lines.iter().flat_map(|line| self.literal_spans(line).into_iter().map(|(start, end)| &line[start..end])).collect()
    }

    /// Mask one line. Overlapping hits: leftmost, then longest.
    pub fn mask_line(&self, text: &str) -> (String, Vec<(String, String)>) {
        let kept = self.literal_spans(text);
        let mut map = Vec::new();
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for (i, (start, end)) in kept.into_iter().enumerate() {
            out.push_str(&text[last..start]);
            let key = format!("[CTRL_{i}]");
            map.push((key.clone(), text[start..end].to_string()));
            out.push_str(&key);
            last = end;
        }
        out.push_str(&text[last..]);
        (out, map)
    }

    /// Unit-wide `[CTRL_n]` numbering; replacement never rescans inserted tokens.
    pub fn mask_unit_lines(&self, lines: &[String]) -> (Vec<String>, Vec<(String, String)>) {
        let mut unit_map: Vec<(String, String)> = Vec::new();
        let mut masked_lines = Vec::with_capacity(lines.len());
        for line in lines {
            let (m, map) = self.mask_line(line);
            let base = unit_map.len();
            let mut line_out = String::with_capacity(m.len());
            let mut cursor = 0;
            let token_re = mask_token_regex();
            let mut renamed = Vec::with_capacity(map.len());
            for (j, (k, v)) in map.into_iter().enumerate() {
                renamed.push((format!("[CTRL_{}]", base + j), v));
                debug_assert_eq!(k, format!("[CTRL_{j}]"));
            }
            for (j, hit) in token_re.find_iter(&m).enumerate() {
                line_out.push_str(&m[cursor..hit.start()]);
                line_out.push_str(&renamed[j].0);
                cursor = hit.end();
            }
            line_out.push_str(&m[cursor..]);
            unit_map.extend(renamed);
            masked_lines.push(line_out);
        }
        (masked_lines, unit_map)
    }
}

pub fn path(workspace: &Path) -> PathBuf {
    workspace.join(PRESERVE_FILE)
}

pub fn load(workspace: &Path, engine: &str) -> PreserveSet {
    let mut s = PreserveSet::for_engine(engine);
    s.merge_file(&path(workspace));
    s
}

pub fn list(workspace: &Path, engine: &str) -> Vec<RuleInfo> {
    load(workspace, engine).rules()
}

pub fn add(workspace: &Path, pattern: &str, info: &str) -> Result<PathBuf> {
    let re = compile_rule(pattern)?;
    drop(re);
    let p = path(workspace);
    let mut file = load_file(&p);
    if file.rules.iter().any(|r| r.pattern == pattern) {
        file.rules.retain(|r| r.pattern != pattern);
    }
    file.rules.push(FileRule {
        pattern: pattern.to_string(),
        info: info.to_string(),
    });
    save_file(&p, &file)?;
    Ok(p)
}

pub fn remove(workspace: &Path, pattern: &str) -> Result<bool> {
    let p = path(workspace);
    let mut file = load_file(&p);
    let before = file.rules.len();
    file.rules.retain(|r| r.pattern != pattern);
    let removed = file.rules.len() != before;
    if removed {
        save_file(&p, &file)?;
    }
    Ok(removed)
}

fn load_file(path: &Path) -> PreserveFile {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return PreserveFile {
            version: PRESERVE_VERSION,
            rules: Vec::new(),
        };
    };
    toml::from_str(&raw).unwrap_or(PreserveFile {
        version: PRESERVE_VERSION,
        rules: Vec::new(),
    })
}

fn save_file(path: &Path, file: &PreserveFile) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let body = toml::to_string_pretty(file).context("serialize preserve.toml")?;
    let header = "# attx preserve: regex hits become [CTRL_n] before translation.\n\
                  # Built-in controls and placeholders apply; this file adds rules.\n";
    std::fs::write(path, format!("{header}{body}"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn compile_rule(pattern: &str) -> Result<Regex> {
    if pattern.trim().is_empty() {
        bail!("empty preserve pattern");
    }
    let re = Regex::new(pattern).with_context(|| format!("compile preserve pattern `{pattern}`"))?;
    if re.is_match("") {
        bail!("preserve pattern matches the empty string (refused): `{pattern}`");
    }
    Ok(re)
}

fn mask_token_regex() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(MASK_TOKEN).expect("mask token pattern"));
    &RE
}

/// Named/indexed placeholders may move; engine controls and markup may not.
fn reorderable_literal(literal: &str) -> bool {
    static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("^(?:{BRACE_PLACEHOLDER}|{RENPY_BRACKET})$")).expect("named placeholder pattern"));
    if !literal.starts_with("[CTRL_") && NAMED.is_match(literal) { return true }
    literal.strip_prefix('%').and_then(|tail| tail.split_once('$'))
        .is_some_and(|(index, _)| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
}

/// Require exact token multiplicity and the relative order of positional controls.
pub fn check_masked_tokens(lines: &[String], map: &[(String, String)]) -> Result<()> {
    let mut seen = vec![false; map.len()];
    let mut ordered = map.iter().filter(|(_, literal)| !reorderable_literal(literal));
    for hit in lines.iter().flat_map(|line| mask_token_regex().find_iter(line)) {
        let index = hit.as_str()[6..hit.as_str().len() - 1].parse::<usize>().ok();
        let Some(index) = index.filter(|&index| map.get(index).is_some_and(|(token, _)| token == hit.as_str())) else {
            bail!("unknown control-mask token {}", hit.as_str());
        };
        if seen[index] { bail!("duplicate control-mask token {}", hit.as_str()) }
        seen[index] = true;
        if !reorderable_literal(&map[index].1) && ordered.next().is_none_or(|(token, _)| token != hit.as_str()) {
            bail!("engine control-mask order mismatch at {}", hit.as_str());
        }
    }
    if seen.contains(&false) { bail!("missing control-mask token(s)") }
    Ok(())
}

/// Validate restored literals too, covering imports and existing cache records.
pub fn check_preserved_literals(lines: &[String], map: &[(String, String)], set: &PreserveSet) -> Result<()> {
    let actual = set.literals(lines);
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for (_, literal) in map { *counts.entry(literal).or_default() += 1; }
    let mut ordered = map.iter().filter(|(_, literal)| !reorderable_literal(literal));
    for literal in actual {
        let Some(count) = counts.get_mut(literal).filter(|count| **count > 0) else {
            bail!("unexpected or duplicate preserved literal {literal:?}");
        };
        *count -= 1;
        if !reorderable_literal(literal) && ordered.next().is_none_or(|(_, expected)| expected != literal) {
            bail!("engine control or markup order mismatch");
        }
    }
    if counts.values().any(|&count| count != 0) { bail!("missing preserved literal(s)") }
    Ok(())
}

/// Restore tokens in one pass so literal source tokens cannot be rescanned.
pub fn unmask_line(text: &str, map: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for hit in mask_token_regex().find_iter(text) {
        out.push_str(&text[cursor..hit.start()]);
        let literal = map.iter().find(|(token, _)| token == hit.as_str())
            .map(|(_, literal)| literal.as_str()).unwrap_or(hit.as_str());
        out.push_str(literal);
        cursor = hit.end();
    }
    out.push_str(&text[cursor..]);
    out
}

/// Count missing preserved literal occurrences, not just distinct strings.
pub fn lost_token_count(translation_lines: &[String], map: &[(String, String)]) -> usize {
    let dst = translation_lines.join("\n");
    let mut expected = std::collections::BTreeMap::<&str, usize>::new();
    for (_, literal) in map {
        *expected.entry(literal.as_str()).or_default() += 1;
    }
    let mut spans = Vec::new();
    for literal in expected.keys().copied().filter(|literal| !literal.is_empty()) {
        for (start, _) in dst.match_indices(literal) {
            spans.push((start, start + literal.len(), literal));
        }
    }
    spans.sort_by_key(|&(start, end, _)| (start, std::cmp::Reverse(end - start)));
    let mut last_end = 0;
    for (start, end, literal) in spans {
        if start < last_end { continue; }
        last_end = end;
        let missing = expected.get_mut(literal).expect("known preserved literal");
        *missing = missing.saturating_sub(1);
    }
    expected.into_values().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rmmz_codes_still_mask() {
        let (m, map) = PreserveSet::core().mask_line(r"\C[1]こんにちは\n[1]");
        assert!(m.contains("[CTRL_"));
        assert_eq!(map.len(), 2);
        let restored = {
            let mut s = m;
            for (k, v) in &map {
                s = s.replace(k, v);
            }
            s
        };
        assert_eq!(restored, r"\C[1]こんにちは\n[1]");
    }

    #[test]
    fn mask_unit_no_renumber_collision() {
        let lines = vec![r"\C[1]おはよう".to_string(), r"\C[2]やあ\V[7]".to_string()];
        let (masked, map) = PreserveSet::core().mask_unit_lines(&lines);
        assert_eq!(masked[0], "[CTRL_0]おはよう");
        assert_eq!(masked[1], "[CTRL_1]やあ[CTRL_2]");
        let restored0 = {
            let mut s = masked[0].clone();
            for (k, v) in &map {
                s = s.replace(k, v);
            }
            s
        };
        let restored1 = {
            let mut s = masked[1].clone();
            for (k, v) in &map {
                s = s.replace(k, v);
            }
            s
        };
        assert_eq!(restored0, lines[0]);
        assert_eq!(restored1, lines[1]);
        let m: std::collections::BTreeMap<_, _> = map.into_iter().collect();
        assert_eq!(m["[CTRL_1]"], r"\C[2]");
        assert_eq!(m["[CTRL_2]"], r"\V[7]");
    }

    #[test]
    fn brace_and_printf_are_builtin() {
        let (m, map) = PreserveSet::core().mask_line("got {item} x%s");
        assert_eq!(map.len(), 2);
        assert!(map.iter().any(|(_, v)| v == "{item}"));
        assert!(map.iter().any(|(_, v)| v == "%s"));
        assert!(!m.contains("{item}"));
        assert!(!m.contains("%s"));
    }

    #[test]
    fn renpy_brackets_only_for_renpy_engine() {
        let core = PreserveSet::core().mask_line("hi [player] there");
        assert!(
            core.1.is_empty(),
            "core must not eat [player]: {:?}",
            core.1
        );
        let (m, map) = PreserveSet::for_engine("renpy").mask_line("hi [player] there");
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].1, "[player]");
        assert!(!m.contains("[player]"));
    }

    #[test]
    fn overlapping_spans_keep_leftmost_longest() {
        let mut s = PreserveSet::empty();
        s.push_builtin(r"abc", "short");
        s.push_builtin(r"abcd", "long");
        let (_, map) = s.mask_line("abcd");
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].1, "abcd");
    }

    #[test]
    fn empty_match_pattern_is_refused() {
        assert!(compile_rule(".*").is_err());
        assert!(compile_rule("").is_err());
    }

    #[test]
    fn workspace_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("attx-pv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        add(&dir, r"\{player_name\}", "player slot").unwrap();
        let set = load(&dir, "txt");
        let (_, map) = set.mask_line("hello {player_name}!");
        assert!(map.iter().any(|(_, v)| v == "{player_name}"));
        assert!(remove(&dir, r"\{player_name\}").unwrap());
        assert!(!remove(&dir, r"\{player_name\}").unwrap());
    }

    #[test]
    fn lost_token_count_detects_drop() {
        let map = vec![("[CTRL_0]".into(), "{item}".into())];
        assert_eq!(lost_token_count(&["拿到了{item}".into()], &map), 0);
        assert_eq!(lost_token_count(&["拿到了东西".into()], &map), 1);
    }
    #[test]
    fn repeated_literals_are_counted_and_nested_spans_are_not_double_counted() {
        let (_, map) = PreserveSet::core().mask_line("{name} {name} {name}");
        assert_eq!(lost_token_count(&["{name}".into()], &map), 2);
        assert_eq!(lost_token_count(&["{name} {name} {name}".into()], &map), 0);
        let (_, map) = PreserveSet::core().mask_line(r"\n \n[1]");
        assert_eq!(lost_token_count(&[r"\n[1]".into()], &map), 1);
    }

    #[test]
    fn control_tokens_require_exact_multiplicity() {
        let (_, map) = PreserveSet::core().mask_line(r"\SE[カナ]\V[1]");
        assert!(check_masked_tokens(&["[CTRL_0]你好[CTRL_1]".into()], &map).is_ok());
        for bad in ["[CTRL_0]你好", "[CTRL_0][CTRL_0][CTRL_1]", "[CTRL_0][CTRL_1][CTRL_8]", "[CTRL_00][CTRL_1]"] {
            assert!(check_masked_tokens(&[bad.into()], &map).is_err());
        }
        assert!(check_masked_tokens(&["[CTRL_0]".into()], &[]).is_err());
    }

    #[test]
    fn reordered_masked_and_restored_controls_are_rejected() {
        let set = PreserveSet::core();
        let (_, map) = set.mask_line(r"\C[1]こんにちは\C[0]");
        assert!(check_masked_tokens(&["[CTRL_1]你好[CTRL_0]".into()], &map).is_err());
        assert!(check_preserved_literals(&[r"\C[0]你好\C[1]".into()], &map, set).is_err());
        assert!(check_preserved_literals(&[r"\C[1]你好\C[0]".into()], &map, set).is_ok());
    }

    #[test]
    fn literal_mask_tokens_restore_without_recursive_replacement() {
        let lines = vec!["literal [CTRL_1] {name}".into(), "[CTRL_0]".into()];
        let (masked, map) = PreserveSet::core().mask_unit_lines(&lines);
        assert!(check_masked_tokens(&masked, &map).is_ok());
        let restored: Vec<String> = masked.iter().map(|line| unmask_line(line, &map)).collect();
        assert_eq!(restored, lines);
    }

    #[test]
    fn common_placeholder_boundaries_preserve_only_declared_syntax() {
        let input = r"\SE[カナ] %2$s %1$04d %.2f %% {{ player.name }} {0} {1,-8:N2} {item} [普通的人类文本] [word] {ordinary prose} 100% ready";
        let (_, map) = PreserveSet::core().mask_line(input);
        let protected: Vec<&str> = map.iter().map(|(_, value)| value.as_str()).collect();
        assert_eq!(protected, [r"\SE[カナ]", "%2$s", "%1$04d", "%.2f", "%%", "{{ player.name }}", "{0}", "{1,-8:N2}", "{item}"]);
        assert!(!protected.contains(&"[word]"));
        let markup = r#"<span class="name">人类文本</span><br/> 3 < 5 > 2"#;
        assert!(PreserveSet::core().mask_line(markup).1.is_empty());
        let (_, tags) = PreserveSet::for_engine("auto").mask_line(markup);
        assert_eq!(tags.iter().map(|(_, value)| value.as_str()).collect::<Vec<_>>(), [r#"<span class="name">"#, "</span>", "<br/>"]);
    }
}
