//! Text-preserve rules: regex hits become `[CTRL_n]` before the model sees them.
//!
//! RMMZ backslash codes stay the baseline (same pattern as `model::mask_controls`).
//! Built-in extras cover `{ident}` and printf `%s`/`%d`. Ren'Py adds `[ident]`.
//! A workspace `preserve.toml` appends more patterns; overlapping hits keep the
//! leftmost-longest span.

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const PRESERVE_FILE: &str = "preserve.toml";
pub const PRESERVE_VERSION: u32 = 1;

/// Shared with `model::mask_controls` so RMMZ codes have one pattern.
pub const RMMZ_CONTROL_PATTERN: &str = r"(?x)
        \\{2}                                   # escaped backslash
        | \\[VvNnCcGg]\[\d+\]                   # \V[n] \N[n] \C[n] \G[n] (case variants)
        | \\[VvNnCcGg]                          # bare
        | \\[!.>|{\}\\\$\^]                     # single-char controls
        | \\[A-Za-z]\[\d+\]                     # other letter[n]
        | \\[A-Za-z]                            # other letter
        ";

const BRACE_PLACEHOLDER: &str = r"\{[A-Za-z_][A-Za-z0-9_]*\}";
const PRINTF_PLACEHOLDER: &str = r"%\d*[sd]";
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

    /// RMMZ codes + `{ident}` + printf. No engine-specific extras.
    pub fn core() -> &'static Self {
        static SET: LazyLock<PreserveSet> = LazyLock::new(|| {
            let mut s = PreserveSet::empty();
            s.push_builtin(RMMZ_CONTROL_PATTERN, "rmmz control codes");
            s.push_builtin(BRACE_PLACEHOLDER, "brace placeholder");
            s.push_builtin(PRINTF_PLACEHOLDER, "printf placeholder");
            s
        });
        &SET
    }
    pub fn for_engine(engine: &str) -> Self {
        let mut s = Self::core().clone();
        if engine == "renpy" {
            s.push_builtin(RENPY_BRACKET, "renpy interpolation");
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

    /// Mask one line. Overlapping hits: leftmost, then longest.
    pub fn mask_line(&self, text: &str) -> (String, Vec<(String, String)>) {
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for rule in &self.rules {
            for m in rule.re.find_iter(text) {
                if m.start() == m.end() {
                    continue;
                }
                spans.push((m.start(), m.end()));
            }
        }
        spans.sort_by_key(|&(a, b)| (a, std::cmp::Reverse(b - a)));
        let mut kept: Vec<(usize, usize)> = Vec::new();
        let mut last_end = 0usize;
        for (start, end) in spans {
            if start < last_end {
                continue;
            }
            kept.push((start, end));
            last_end = end;
        }
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

    /// Unit-wide `[CTRL_n]` numbering. Same high-first rename as `model::mask_unit_lines`.
    pub fn mask_unit_lines(&self, lines: &[String]) -> (Vec<String>, Vec<(String, String)>) {
        let mut unit_map: Vec<(String, String)> = Vec::new();
        let mut masked_lines = Vec::with_capacity(lines.len());
        for line in lines {
            let (m, map) = self.mask_line(line);
            let base = unit_map.len();
            let mut line_out = m;
            let mut renamed = vec![(String::new(), String::new()); map.len()];
            for (j, (k, v)) in map.into_iter().enumerate().rev() {
                let nk = format!("[CTRL_{}]", base + j);
                line_out = line_out.replacen(&k, &nk, 1);
                renamed[j] = (nk, v);
            }
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
    let header = "# attx preserve — regexes whose hits become [CTRL_n] before translate.\n\
                  # Builtin RMMZ / {ident} / %s rules always apply; this file only adds more.\n";
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

/// How many original preserved spans are missing from the translation.
pub fn lost_token_count(translation_lines: &[String], map: &[(String, String)]) -> usize {
    if map.is_empty() {
        return 0;
    }
    let dst = translation_lines.join("\n");
    map.iter().filter(|(_, v)| !dst.contains(v.as_str())).count()
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
}
