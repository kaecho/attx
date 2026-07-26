//! Custom format profiles — agent-teachable format support.
//!
//! When no built-in adapter matches an input, an agent (or human) can describe
//! the format as a small TOML profile: which lines/JSON fields hold
//! translatable text and how to write translations back. The profile becomes a
//! full adapter (engine id `custom:<name>`): it powers `detect`, `extract`,
//! and `writeback` exactly like built-in formats.
//!
//! Lifecycle:
//!   attx analyze  --input f            # recon report for the agent
//!   attx profile new --output p.toml   # documented template
//!   attx profile test --profile p.toml --input f   # iterate until units look right
//!   attx init --input f --profile p.toml           # profile copied into workspace
//!   attx profile save --profile p.toml             # "remember this format"
//!
//! Saved profiles live in `$ATTX_HOME/profiles/` or the platform config dir
//! (`~/.config/attx/profiles/`) and participate in `attx detect` fallback.
//!
//! Rules (a profile may mix kinds; JSON kinds apply when the file parses as JSON):
//! * `line_regex`  — per-line regex; named group `text` (required), `role` (optional)
//! * `json_keys`   — recursive: string values whose object key matches
//! * `json_paths`  — slash path globs, `*` = one level, `**` = any depth

use crate::adapter::{DetectHit, FormatAdapter, OutputFile, output_sibling, set_json_path};
use crate::model::{ItemType, TextUnit, Translation, needs_translation};
use crate::textio;
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const WORKSPACE_PROFILE: &str = "profile.toml";
pub const ENGINE_PREFIX: &str = "custom:";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatProfile {
    /// Stable id — engine becomes `custom:<name>`.
    pub name: String,
    #[serde(default)]
    pub label: String,
    /// Lowercase extensions this profile claims (required for directory input).
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Detect: every regex must match somewhere in the first 64 KiB of text.
    #[serde(default)]
    pub detect_regex: Vec<String>,
    /// Detect: trial extraction must yield at least this many units.
    #[serde(default = "default_min_units")]
    pub min_units: usize,
    /// Write translations back in place (pipeline backs up `*.attxbak`).
    /// Default false → translated sibling copy `<stem>.<dst>.<ext>`.
    #[serde(default)]
    pub overwrite: bool,
    /// line_regex only: skip lines matching any of these before rule matching.
    #[serde(default)]
    pub skip_lines: Vec<String>,
    /// Free-form notes (why the rules look like this) — for future readers.
    #[serde(default)]
    pub notes: String,
    pub rules: Vec<Rule>,
}

fn default_min_units() -> usize {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    /// Named groups: `text` (required) — the span replaced on writeback;
    /// `role` (optional) — speaker/context label.
    LineRegex { pattern: String },
    /// String values (or arrays of strings) under any object key in `keys`.
    JsonKeys { keys: Vec<String> },
    /// Slash-separated path globs over the JSON tree.
    JsonPaths { paths: Vec<String> },
}

/// A profile compiled into a usable adapter. Engine id/label/extensions are
/// leaked once per process — bounded, and required by the `'static` adapter
/// trait surface.
pub struct CustomAdapter {
    profile: FormatProfile,
    engine_id: &'static str,
    label: &'static str,
    extensions: &'static [&'static str],
    line_rules: Vec<Regex>,
    skip_rules: Vec<Regex>,
    json_keys: Vec<String>,
    json_paths: Vec<String>,
}

impl CustomAdapter {
    pub fn compile(profile: FormatProfile) -> Result<Self> {
        if profile.name.is_empty()
            || !profile
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!(
                "profile name must be non-empty [a-zA-Z0-9_-]: {:?}",
                profile.name
            );
        }
        if profile.rules.is_empty() {
            bail!("profile {} has no rules", profile.name);
        }
        let mut line_rules = Vec::new();
        let mut json_keys = Vec::new();
        let mut json_paths = Vec::new();
        for rule in &profile.rules {
            match rule {
                Rule::LineRegex { pattern } => {
                    let re = Regex::new(pattern)
                        .with_context(|| format!("bad line_regex pattern: {pattern}"))?;
                    if !re.capture_names().any(|n| n == Some("text")) {
                        bail!("line_regex pattern needs a (?P<text>…) group: {pattern}");
                    }
                    line_rules.push(re);
                }
                Rule::JsonKeys { keys } => json_keys.extend(keys.iter().cloned()),
                Rule::JsonPaths { paths } => json_paths.extend(paths.iter().cloned()),
            }
        }
        let skip_rules = profile
            .skip_lines
            .iter()
            .map(|p| Regex::new(p).with_context(|| format!("bad skip_lines pattern: {p}")))
            .collect::<Result<Vec<_>>>()?;
        for re in &profile.detect_regex {
            Regex::new(re).with_context(|| format!("bad detect_regex: {re}"))?;
        }

        let engine_id: &'static str =
            Box::leak(format!("{ENGINE_PREFIX}{}", profile.name).into_boxed_str());
        let label: &'static str = Box::leak(
            if profile.label.is_empty() {
                format!("custom profile {}", profile.name)
            } else {
                profile.label.clone()
            }
            .into_boxed_str(),
        );
        let extensions: &'static [&'static str] = Box::leak(
            profile
                .extensions
                .iter()
                .map(|e| -> &'static str { Box::leak(e.to_ascii_lowercase().into_boxed_str()) })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        Ok(Self {
            profile,
            engine_id,
            label,
            extensions,
            line_rules,
            skip_rules,
            json_keys,
            json_paths,
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw = textio::read_text(path)?;
        let profile: FormatProfile =
            toml::from_str(&raw).with_context(|| format!("parse profile {}", path.display()))?;
        Self::compile(profile)
    }

    pub fn profile(&self) -> &FormatProfile {
        &self.profile
    }

    fn claims_extension(&self, path: &Path) -> bool {
        if self.extensions.is_empty() {
            return path.is_file();
        }
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .is_some_and(|e| self.extensions.contains(&e.as_str()))
    }

    /// Candidate files: the input itself, or matching files under a directory.
    fn candidate_files(&self, input: &Path) -> Result<Vec<PathBuf>> {
        if input.is_file() {
            return Ok(vec![input.to_path_buf()]);
        }
        if !input.is_dir() {
            bail!("input not found: {}", input.display());
        }
        if self.extensions.is_empty() {
            bail!(
                "profile {} needs `extensions` to scan a directory",
                self.profile.name
            );
        }
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(input)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !(name.starts_with(".attx") || name == ".git" || name == "node_modules")
            })
        {
            let entry = entry?;
            if entry.file_type().is_file() && self.claims_extension(entry.path()) {
                files.push(entry.path().to_path_buf());
            }
        }
        Ok(files)
    }

    fn extract_file(
        &self,
        file: &Path,
        rel: &str,
        source_lang: &str,
        units: &mut Vec<TextUnit>,
    ) -> Result<()> {
        let body = textio::read_text(file)?;
        let json = parse_json_if_rules(&body, &self.json_keys, &self.json_paths);
        if let Some(root) = json {
            let mut hits: Vec<(String, String)> = Vec::new(); // (path, text)
            collect_json_units(&root, &self.json_keys, &self.json_paths, &mut hits);
            for (path, text) in hits {
                push_json_unit(units, self.engine_id, rel, &path, &text, source_lang);
            }
            return Ok(());
        }
        if self.line_rules.is_empty() {
            return Ok(()); // json-only profile on a non-json file
        }
        for (i, raw_line) in body.lines().enumerate() {
            let line = raw_line.trim_end_matches('\r');
            if self.skip_rules.iter().any(|re| re.is_match(line)) {
                continue;
            }
            let Some((text, role)) = self.match_line(line) else {
                continue;
            };
            if text.trim().is_empty() || !needs_translation(&text, source_lang) {
                continue;
            }
            let location = format!("{rel}#L{:06}", i + 1);
            let lines = vec![text];
            units.push(TextUnit {
                id: TextUnit::compute_id(self.engine_id, &location, &lines),
                engine: self.engine_id.to_string(),
                domain: "custom".into(),
                location,
                item_type: ItemType::ShortText,
                role,
                original_lines: lines,
                source_line_paths: vec![],
                context: format!("{rel}/s{:04}", i / 50),
                payload: String::new(),
            });
        }
        Ok(())
    }

    /// First rule whose `text` group matches wins.
    fn match_line(&self, line: &str) -> Option<(String, String)> {
        for re in &self.line_rules {
            if let Some(caps) = re.captures(line)
                && let Some(m) = caps.name("text")
            {
                let role = caps
                    .name("role")
                    .map(|r| r.as_str().to_string())
                    .unwrap_or_default();
                return Some((m.as_str().to_string(), role));
            }
        }
        None
    }

    fn writeback_file(
        &self,
        file: &Path,
        rel: &str,
        target_lang: &str,
        by_location: &BTreeMap<&str, &Translation>,
    ) -> Result<Option<OutputFile>> {
        let body = textio::read_text(file)?;
        let json = parse_json_if_rules(&body, &self.json_keys, &self.json_paths);
        let rendered = if let Some(mut root) = json {
            let prefix = format!("{rel}#");
            let mut touched = false;
            for (loc, tr) in by_location {
                let Some(path) = loc.strip_prefix(&prefix) else {
                    continue;
                };
                if path.starts_with('L') && path[1..].chars().all(|c| c.is_ascii_digit()) {
                    continue; // line unit, not a json path
                }
                let joined = tr.translation_lines.join("\n");
                if let Err(e) = set_json_path(&mut root, path, Value::String(joined)) {
                    eprintln!("custom: skip {loc}: {e}");
                    continue;
                }
                touched = true;
            }
            if !touched {
                return Ok(None);
            }
            serde_json::to_string_pretty(&root)? + "\n"
        } else {
            let mut lines: Vec<String> = body
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect();
            let prefix = format!("{rel}#L");
            let mut touched = false;
            for (loc, tr) in by_location {
                let Some(no) = loc
                    .strip_prefix(&prefix)
                    .and_then(|s| s.parse::<usize>().ok())
                else {
                    continue;
                };
                let Some(slot) = lines.get_mut(no - 1) else {
                    continue;
                };
                // Re-run the rules on the original line to find the text span.
                let Some(span) = self.line_rules.iter().find_map(|re| {
                    re.captures(slot.as_str())
                        .and_then(|c| c.name("text"))
                        .map(|m| (m.start(), m.end()))
                }) else {
                    continue;
                };
                let joined = tr.translation_lines.join(" ");
                *slot = format!("{}{}{}", &slot[..span.0], joined, &slot[span.1..]);
                touched = true;
            }
            if !touched {
                return Ok(None);
            }
            let mut out = lines.join("\n");
            if body.ends_with('\n') {
                out.push('\n');
            }
            out
        };
        let dest = if self.profile.overwrite {
            file.to_path_buf()
        } else {
            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("txt");
            output_sibling(file, target_lang, ext)
        };
        Ok(Some(OutputFile::text(dest, rendered)))
    }
}

impl FormatAdapter for CustomAdapter {
    fn id(&self) -> &'static str {
        self.engine_id
    }
    fn label(&self) -> &'static str {
        self.label
    }
    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }
    fn input_kind(&self) -> &'static str {
        "file|directory"
    }

    fn detect(&self, input: &Path) -> Option<DetectHit> {
        if input.is_file() {
            if !self.claims_extension(input) {
                return None;
            }
            let head = textio::read_text_detected(input).ok()?;
            let head: String = head.text.chars().take(65536).collect();
            if !self
                .profile
                .detect_regex
                .iter()
                .all(|p| Regex::new(p).map(|re| re.is_match(&head)).unwrap_or(false))
            {
                return None;
            }
        } else if !input.is_dir()
            || self.extensions.is_empty()
            || self
                .candidate_files(input)
                .map(|f| f.is_empty())
                .unwrap_or(true)
        {
            return None;
        }
        // Trial extraction is the ground truth.
        let units = self.extract(input, "ja").ok()?;
        if units.len() < self.profile.min_units.max(1) {
            return None;
        }
        Some(DetectHit {
            engine_id: self.engine_id,
            label: self.label,
            content_root: input.canonicalize().unwrap_or_else(|_| input.to_path_buf()),
        })
    }

    fn extract(&self, input: &Path, source_lang: &str) -> Result<Vec<TextUnit>> {
        let files = self.candidate_files(input)?;
        let mut units = Vec::new();
        for file in &files {
            let rel = rel_name(input, file);
            self.extract_file(file, &rel, source_lang, &mut units)?;
        }
        Ok(units)
    }

    fn writeback(
        &self,
        input: &Path,
        target_lang: &str,
        units: &[TextUnit],
        translations: &BTreeMap<String, Translation>,
    ) -> Result<Vec<OutputFile>> {
        // location = "<rel>#<anchor>"; group units per file.
        let mut per_file: BTreeMap<String, BTreeMap<&str, &Translation>> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let Some((rel, _)) = u.location.split_once('#') else {
                continue;
            };
            per_file
                .entry(rel.to_string())
                .or_default()
                .insert(u.location.as_str(), tr);
        }
        let mut out = Vec::new();
        for (rel, by_location) in &per_file {
            let file = if input.is_file() {
                input.to_path_buf()
            } else {
                input.join(rel)
            };
            if !file.is_file() {
                eprintln!("custom: missing source file {}", file.display());
                continue;
            }
            if let Some(o) = self.writeback_file(&file, rel, target_lang, by_location)? {
                out.push(o);
            }
        }
        Ok(out)
    }
}

fn rel_name(input: &Path, file: &Path) -> String {
    if input.is_file() {
        return input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "input".into());
    }
    file.strip_prefix(input)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Parse as JSON only when json rules exist and the body parses.
fn parse_json_if_rules(body: &str, keys: &[String], paths: &[String]) -> Option<Value> {
    if keys.is_empty() && paths.is_empty() {
        return None;
    }
    serde_json::from_str(body).ok()
}

fn push_json_unit(
    units: &mut Vec<TextUnit>,
    engine: &str,
    rel: &str,
    path: &str,
    text: &str,
    source_lang: &str,
) {
    if text.trim().is_empty() || !needs_translation(text, source_lang) {
        return;
    }
    let location = format!("{rel}#{path}");
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let item_type = if lines.len() > 1 {
        ItemType::LongText
    } else {
        ItemType::ShortText
    };
    units.push(TextUnit {
        id: TextUnit::compute_id(engine, &location, &lines),
        engine: engine.to_string(),
        domain: "custom".into(),
        location,
        item_type,
        role: String::new(),
        original_lines: lines,
        source_line_paths: vec![],
        context: rel.to_string(),
        payload: String::new(),
    });
}

/// Walk the JSON tree once, collecting (path, text) for both rule kinds.
/// Deduped by path (a value may match a key rule and a path rule).
fn collect_json_units(
    root: &Value,
    keys: &[String],
    paths: &[String],
    out: &mut Vec<(String, String)>,
) {
    let mut seen = std::collections::BTreeSet::new();
    walk_json(root, &mut String::new(), &mut |path, key, text| {
        let by_key = key.is_some_and(|k| keys.iter().any(|want| want == k));
        let by_path = paths.iter().any(|glob| path_matches(glob, path));
        if (by_key || by_path) && seen.insert(path.to_string()) {
            out.push((path.to_string(), text.to_string()));
        }
    });
}

/// DFS over string leaves. `key` is the nearest object key (array indices keep
/// the parent key, so `"lines": ["a","b"]` matches key rule "lines").
fn walk_json(v: &Value, path: &mut String, f: &mut impl FnMut(&str, Option<&str>, &str)) {
    fn inner(
        v: &Value,
        path: &mut String,
        key: Option<&str>,
        f: &mut impl FnMut(&str, Option<&str>, &str),
    ) {
        match v {
            Value::String(s) => f(path, key, s),
            Value::Object(o) => {
                for (k, child) in o {
                    if k.contains('/') {
                        continue; // would collide with path syntax
                    }
                    let len = path.len();
                    if !path.is_empty() {
                        path.push('/');
                    }
                    path.push_str(k);
                    inner(child, path, Some(k), f);
                    path.truncate(len);
                }
            }
            Value::Array(a) => {
                for (i, child) in a.iter().enumerate() {
                    let len = path.len();
                    if !path.is_empty() {
                        path.push('/');
                    }
                    path.push_str(&i.to_string());
                    inner(child, path, key, f);
                    path.truncate(len);
                }
            }
            _ => {}
        }
    }
    inner(v, path, None, f);
}

/// `events/*/name` — `*` one segment, `**` any number (including zero).
fn path_matches(glob: &str, path: &str) -> bool {
    fn rec(gs: &[&str], ps: &[&str]) -> bool {
        match (gs.first(), ps.first()) {
            (None, None) => true,
            (Some(&"**"), _) => rec(&gs[1..], ps) || (!ps.is_empty() && rec(gs, &ps[1..])),
            (Some(&g), Some(&p)) if g == "*" || g == p => rec(&gs[1..], &ps[1..]),
            _ => false,
        }
    }
    let gs: Vec<&str> = glob.split('/').filter(|s| !s.is_empty()).collect();
    let ps: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    rec(&gs, &ps)
}

// ---------------------------------------------------------------- storage

/// Directories searched for saved profiles, in priority order.
pub fn profile_dirs() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if let Ok(home) = std::env::var("ATTX_HOME") {
        dirs_out.push(PathBuf::from(home).join("profiles"));
    }
    if let Some(cfg) = dirs::config_dir() {
        dirs_out.push(cfg.join("attx/profiles"));
    }
    dirs_out
}

/// All loadable saved profiles (bad files are reported to stderr, not fatal).
pub fn saved_profiles() -> Vec<(PathBuf, CustomAdapter)> {
    let mut out = Vec::new();
    let mut seen_names = std::collections::BTreeSet::new();
    for dir in profile_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            match CustomAdapter::load(&path) {
                Ok(a) => {
                    if seen_names.insert(a.profile().name.clone()) {
                        out.push((path, a));
                    }
                }
                Err(e) => eprintln!("warning: skip profile {}: {e:#}", path.display()),
            }
        }
    }
    out
}

pub fn find_saved(name: &str) -> Result<(PathBuf, CustomAdapter)> {
    let want = name.strip_prefix(ENGINE_PREFIX).unwrap_or(name);
    for (path, a) in saved_profiles() {
        if a.profile().name == want {
            return Ok((path, a));
        }
    }
    bail!(
        "no saved profile named {want:?}. `attx profile list` shows saved ones; \
         `attx profile save --profile <file>` remembers a new one."
    )
}

/// Save (remember) a profile file into the user profile dir.
pub fn save(profile_path: &Path, force: bool) -> Result<PathBuf> {
    let adapter = CustomAdapter::load(profile_path)?; // validates
    let dir = profile_dirs()
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no config dir available; set $ATTX_HOME"))?;
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("{}.toml", adapter.profile().name));
    if dest.exists() && !force {
        bail!(
            "profile already saved at {}; pass --force to overwrite",
            dest.display()
        );
    }
    std::fs::copy(profile_path, &dest)?;
    Ok(dest)
}

pub fn template(name: &str) -> String {
    format!(
        r#"# attx custom format profile — teach attx a new file format.
# Docs: skills/attx/references/custom-format-discovery.md
name = "{name}"                    # id → engine "custom:{name}"
label = ""                         # human-readable description
extensions = []                    # e.g. ["ks", "scn"]; required for directory input
detect_regex = []                  # ALL must match in the first 64 KiB (auto-detect aid)
min_units = 1                      # auto-detect needs ≥ this many extracted units
overwrite = false                  # true = write back in place (backup *.attxbak kept)
skip_lines = []                    # line_regex mode: skip lines matching any regex
notes = ""                         # why these rules — for the next reader

# --- pick / combine rule kinds ---

# Per-line regex. Named groups: (?P<text>...) required, (?P<role>...) optional.
# [[rules]]
# kind = "line_regex"
# pattern = '^(?P<role>[^\s@;]*)\s*「(?P<text>.+)」$'

# JSON: translate string values under these object keys (any depth).
# [[rules]]
# kind = "json_keys"
# keys = ["message", "name", "description"]

# JSON: translate string leaves at path globs (* = one level, ** = any depth).
# [[rules]]
# kind = "json_paths"
# paths = ["events/*/text", "**/choices/*"]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TextUnit;

    fn tr_map(units: &[TextUnit], text: &str) -> BTreeMap<String, Translation> {
        units
            .iter()
            .map(|u| {
                (
                    u.id.clone(),
                    Translation {
                        unit_id: u.id.clone(),
                        translation_lines: vec![text.to_string()],
                        source_hash: TextUnit::source_hash(&u.original_lines),
                        passthrough: false,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn line_regex_roundtrip() {
        let dir = crate::adapter::test_dir("profile-line");
        let input = dir.join("scene.ks");
        std::fs::write(
            &input,
            "; コメント\n@bg storage=room\n【直哉】「おはよう」\nナレーション行です。\n",
        )
        .unwrap();
        let profile: FormatProfile = toml::from_str(
            r#"
name = "kag-test"
extensions = ["ks"]
skip_lines = ['^\s*;', '^\s*@']
[[rules]]
kind = "line_regex"
pattern = '^【(?P<role>[^】]+)】「(?P<text>.+)」$'
[[rules]]
kind = "line_regex"
pattern = '^(?P<text>[^;@【].*)$'
"#,
        )
        .unwrap();
        let a = CustomAdapter::compile(profile).unwrap();
        assert!(a.detect(&input).is_some());
        let units = a.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2, "{units:#?}");
        assert_eq!(units[0].role, "直哉");
        assert_eq!(units[0].original_lines, ["おはよう"]);
        let outs = a
            .writeback(&input, "zh", &units, &tr_map(&units, "译"))
            .unwrap();
        let body = String::from_utf8(outs[0].bytes.clone()).unwrap();
        assert!(body.contains("【直哉】「译」"), "{body}");
        assert!(body.contains("; コメント"), "{body}");
        assert!(body.contains("@bg storage=room"), "{body}");
        assert!(outs[0].path.to_string_lossy().ends_with("scene.zh.ks"));
    }

    #[test]
    fn json_keys_and_paths_roundtrip() {
        let dir = crate::adapter::test_dir("profile-json");
        let input = dir.join("data.dat");
        std::fs::write(
            &input,
            r#"{"scenes":[{"message":"こんにちは","speaker":"直哉","flag":1}],"meta":{"title":"物語"}}"#,
        )
        .unwrap();
        let profile: FormatProfile = toml::from_str(
            r#"
name = "json-test"
extensions = ["dat"]
[[rules]]
kind = "json_keys"
keys = ["message"]
[[rules]]
kind = "json_paths"
paths = ["meta/title"]
"#,
        )
        .unwrap();
        let a = CustomAdapter::compile(profile).unwrap();
        let units = a.extract(&input, "ja").unwrap();
        assert_eq!(units.len(), 2, "{units:#?}");
        let outs = a
            .writeback(&input, "zh", &units, &tr_map(&units, "译"))
            .unwrap();
        let v: Value = serde_json::from_slice(&outs[0].bytes).unwrap();
        assert_eq!(v["scenes"][0]["message"], "译");
        assert_eq!(v["scenes"][0]["speaker"], "直哉");
        assert_eq!(v["meta"]["title"], "译");
    }

    #[test]
    fn directory_mode_and_overwrite() {
        let dir = crate::adapter::test_dir("profile-dir");
        let game = dir.join("game");
        std::fs::create_dir_all(game.join("scenario")).unwrap();
        std::fs::write(game.join("scenario/a.ks"), "台詞その一\n").unwrap();
        std::fs::write(game.join("scenario/b.ks"), "台詞その二\n").unwrap();
        std::fs::write(game.join("readme.txt"), "無関係\n").unwrap();
        let profile: FormatProfile = toml::from_str(
            r#"
name = "dir-test"
extensions = ["ks"]
overwrite = true
[[rules]]
kind = "line_regex"
pattern = '^(?P<text>.+)$'
"#,
        )
        .unwrap();
        let a = CustomAdapter::compile(profile).unwrap();
        let units = a.extract(&game, "ja").unwrap();
        assert_eq!(units.len(), 2);
        assert!(units[0].location.starts_with("scenario/a.ks#L"));
        let outs = a
            .writeback(&game, "zh", &units, &tr_map(&units, "译"))
            .unwrap();
        assert_eq!(outs.len(), 2);
        assert_eq!(
            outs[0].path,
            game.join("scenario/a.ks"),
            "overwrite in place"
        );
    }

    #[test]
    fn path_glob_matching() {
        assert!(path_matches("events/*/name", "events/3/name"));
        assert!(!path_matches("events/*/name", "events/3/x/name"));
        assert!(path_matches("**/name", "a/b/name"));
        assert!(path_matches("**/name", "name"));
        assert!(path_matches("a/**", "a/b/c"));
        assert!(!path_matches("a/*", "b/c"));
    }

    #[test]
    fn template_parses() {
        // Rules are commented out in the template, so it is valid TOML but not
        // yet a valid profile — syntax is what we guarantee here.
        let parsed: std::result::Result<toml::Value, _> = toml::from_str(&template("demo"));
        assert!(parsed.is_ok(), "template must be valid TOML");
    }

    #[test]
    fn repo_example_profiles_compile() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("profiles/examples");
        let mut n = 0;
        for entry in std::fs::read_dir(&dir).expect("profiles/examples") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                CustomAdapter::load(&path)
                    .unwrap_or_else(|e| panic!("example {} invalid: {e:#}", path.display()));
                n += 1;
            }
        }
        assert!(n >= 3, "expected ≥3 example profiles, found {n}");
    }
}
