//! Self-improving extraction knowledge.
//!
//! Format adapters decide what to extract using hardcoded heuristics (name
//! whitelists, machine-literal checks). Those tables are hand-written and
//! therefore wrong sometimes — and every fix stays in the source, so the next
//! game and the next user re-discover the same mistake.
//!
//! This module keeps that judgement as *data*: per-format rules that say a
//! field name is an identity handle (skip) or player-visible text (extract).
//! Rules are learned from evidence (see `learn.rs`), approved by a human, and
//! stored as readable TOML next to saved profiles.
//!
//! Design constraints that keep this safe:
//! * `apply` is a pure function over already-extracted units — no adapter
//!   changes, no I/O, and `--no-knowledge` restores the old behaviour exactly.
//! * The rule language is deliberately poor: field name, verdict, scope. No
//!   regex, no value predicates, no boolean composition.
//! * `Extract` rules cannot resurrect machine literals (see `apply`). Learning
//!   may override a *name* heuristic, never the evidence of a value.

use crate::model::TextUnit;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Current on-disk schema version for a rule file.
pub const RULES_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Field addresses game logic (ids, handles, filenames) — never translate.
    Skip,
    /// Field holds player-visible text the adapter's heuristics missed.
    Extract,
}

/// Where in a structure the rule applies. Plugin params, for instance, behave
/// differently at the top level than inside a nested struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Only inside a nested structure (unit location carries a `#json/path`).
    Nested,
    /// Only at the top level of a record.
    Top,
    #[default]
    Any,
}

impl Scope {
    fn matches(self, unit_is_nested: bool) -> bool {
        match self {
            Scope::Any => true,
            Scope::Nested => unit_is_nested,
            Scope::Top => !unit_is_nested,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Lowercase field name. A leading `*` matches by suffix (`*text`).
    pub field: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reason: String,
    /// Human-checkable provenance: which workspace, how many hits.
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub approved_at: String,
}

impl Rule {
    fn matches_field(&self, field: &str) -> bool {
        match self.field.strip_prefix('*') {
            Some(suffix) => !suffix.is_empty() && field.ends_with(suffix),
            None => self.field == field,
        }
    }

    /// Exact rules beat suffix rules, so a specific `key` overrides a broad
    /// `*key`. Ties are broken by the caller (skip wins).
    fn specificity(&self) -> u8 {
        if self.field.starts_with('*') { 0 } else { 1 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub format: String,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

fn default_version() -> u32 {
    RULES_VERSION
}

impl RuleSet {
    pub fn new(format: &str) -> Self {
        Self {
            format: format.to_string(),
            version: RULES_VERSION,
            rules: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Best rule for `field`, or None. Exact beats suffix; within the same
    /// specificity `Skip` beats `Extract` — a missed translation is visible in
    /// `attx status`, a translated identity handle silently breaks saves.
    fn lookup(&self, field: &str, nested: bool) -> Option<&Rule> {
        self.rules
            .iter()
            .filter(|r| r.scope.matches(nested) && r.matches_field(field))
            .max_by_key(|r| (r.specificity(), u8::from(r.verdict == Verdict::Skip)))
    }
}

/// What `apply` did, for `--verbose` reporting and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub skipped: usize,
    /// Extract rules that fired but were vetoed by the machine-literal check.
    pub extract_vetoed: usize,
}

/// Filter extracted units through the learned rules.
///
/// Only removal happens here: an `Extract` rule cannot invent a unit the
/// adapter never produced. Its job is to stop *other* layers from dropping the
/// field, which for the current adapters means it simply protects the unit from
/// a `Skip` rule — and it is refused outright when the value is a machine
/// literal, so a bad rule can never send switch ids or filenames to the model.
pub fn apply(units: Vec<TextUnit>, rules: &RuleSet) -> (Vec<TextUnit>, ApplyReport) {
    if rules.is_empty() {
        return (units, ApplyReport::default());
    }
    let mut report = ApplyReport::default();
    let mut out = Vec::with_capacity(units.len());
    for u in units {
        let Some(field) = field_of(&u) else {
            out.push(u);
            continue;
        };
        let nested = is_nested(&u);
        match rules.lookup(&field, nested).map(|r| r.verdict) {
            Some(Verdict::Skip) => {
                report.skipped += 1;
            }
            Some(Verdict::Extract) => {
                // Gate: a learned rule may override the *name* heuristic, never
                // the evidence that the value itself is machine data.
                if u.original_lines.iter().all(|l| is_machine_literal(l)) {
                    report.extract_vetoed += 1;
                }
                out.push(u);
            }
            None => out.push(u),
        }
    }
    (out, report)
}

/// True when the unit addresses something inside a nested structure.
fn is_nested(u: &TextUnit) -> bool {
    u.location.contains('#')
}

/// Field name a rule can match against, lowercased.
///
/// Locations look like `js/plugins.js/0/baseAchievementData#0/Rewards/0/Name`
/// or `Map003.json/2/0/5`. The field is the last non-numeric segment, so array
/// indices do not become field names.
pub fn field_of(u: &TextUnit) -> Option<String> {
    let tail = u.location.rsplit('#').next().unwrap_or(&u.location);
    let from_path = tail
        .split('/')
        .rfind(|s| !s.is_empty() && s.parse::<usize>().is_err())
        .map(|s| s.to_ascii_lowercase());
    if let Some(f) = &from_path
        && !f.contains('.')
    {
        return Some(f.clone());
    }
    // Fall back to an explicit payload field (jsonkv, csv, profile adapters).
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&u.payload)
        && let Some(k) = v
            .get("field")
            .or_else(|| v.get("key"))
            .or_else(|| v.get("param"))
            .and_then(|x| x.as_str())
        && !k.is_empty()
    {
        return Some(k.to_ascii_lowercase());
    }
    from_path
}

/// Shared machine-data predicate. Mirrors the adapters' own checks so a learned
/// `Extract` rule is judged by the same standard the extractor uses.
pub fn is_machine_literal(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if matches!(
        t.to_ascii_lowercase().as_str(),
        "true" | "false" | "null" | "undefined"
    ) {
        return true;
    }
    if t.parse::<f64>().is_ok() {
        return true;
    }
    // Paths / asset filenames / script fragments.
    let asset = t.rsplit('.').next().is_some_and(|ext| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "ogg" | "m4a" | "wav" | "json" | "js" | "css" | "webm"
        )
    });
    if asset || t.contains("$game") || t.starts_with("function") || t.contains("return ") {
        return true;
    }
    false
}

// ---- storage ----

/// Directories that may hold knowledge files, most specific first. Mirrors
/// `profile::config_dirs` so profiles and knowledge live side by side.
pub fn knowledge_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("ATTX_HOME") {
        out.push(PathBuf::from(home).join("knowledge"));
    }
    if let Some(cfg) = dirs::config_dir() {
        out.push(cfg.join("attx/knowledge"));
    }
    out
}

/// Writable knowledge dir, created on demand.
pub fn knowledge_dir() -> Result<PathBuf> {
    let dir = knowledge_dirs()
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no config dir available; set $ATTX_HOME"))?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create knowledge dir {}", dir.display()))?;
    Ok(dir)
}

fn rules_path_in(dir: &Path, format: &str) -> PathBuf {
    dir.join(format!("{format}.toml"))
}

/// Load approved rules for a format. Missing file → empty set (not an error).
/// A malformed file is reported to stderr and treated as empty so a bad edit
/// degrades to "no learning" rather than breaking extraction.
pub fn load_rules(format: &str) -> RuleSet {
    for dir in knowledge_dirs() {
        let p = rules_path_in(&dir, format);
        if !p.is_file() {
            continue;
        }
        match std::fs::read_to_string(&p)
            .map_err(anyhow::Error::from)
            .and_then(|raw| toml::from_str::<RuleSet>(&raw).map_err(anyhow::Error::from))
        {
            Ok(rs) => return rs,
            Err(e) => eprintln!("knowledge: ignoring {}: {e:#}", p.display()),
        }
    }
    RuleSet::new(format)
}

pub fn save_rules(rules: &RuleSet) -> Result<PathBuf> {
    let dir = knowledge_dir()?;
    let path = rules_path_in(&dir, &rules.format);
    let body = toml::to_string_pretty(rules).context("serialize rules")?;
    let header = format!(
        "# attx learned extraction rules for format `{}`.\n\
         # Edit or delete freely: `attx learn list` shows what is active,\n\
         # `attx extract --no-knowledge` ignores this file entirely.\n",
        rules.format
    );
    std::fs::write(&path, format!("{header}{body}"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// All formats that currently have a rule file, with their rules.
pub fn all_rules() -> Vec<RuleSet> {
    let mut seen: BTreeMap<String, RuleSet> = BTreeMap::new();
    for dir in knowledge_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem == "proposals" || seen.contains_key(stem) {
                continue;
            }
            let rs = load_rules(stem);
            if !rs.is_empty() {
                seen.insert(stem.to_string(), rs);
            }
        }
    }
    seen.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemType;

    fn unit(location: &str, text: &str) -> TextUnit {
        TextUnit {
            id: location.to_string(),
            engine: "rmmz".into(),
            domain: "plugins".into(),
            location: location.to_string(),
            item_type: ItemType::ShortText,
            role: String::new(),
            original_lines: vec![text.to_string()],
            source_line_paths: vec![],
            context: String::new(),
            payload: String::new(),
        }
    }

    fn rule(field: &str, verdict: Verdict, scope: Scope) -> Rule {
        Rule {
            field: field.into(),
            verdict,
            scope,
            confidence: 1.0,
            reason: "test".into(),
            evidence: vec![],
            approved_at: String::new(),
        }
    }

    fn ruleset(rules: Vec<Rule>) -> RuleSet {
        RuleSet {
            format: "rmmz".into(),
            version: RULES_VERSION,
            rules,
        }
    }

    #[test]
    fn field_of_uses_last_non_numeric_segment() {
        let u = unit("js/plugins.js/0/baseAchievementData#0/description", "x");
        assert_eq!(field_of(&u).as_deref(), Some("description"));
    }

    #[test]
    fn field_of_skips_array_indices() {
        // `.../Rewards/0/Name` -> Name; `.../Rewards/0` -> Rewards
        let u = unit("js/plugins.js/2/QuestDatas#0/Rewards/0/Name", "x");
        assert_eq!(field_of(&u).as_deref(), Some("name"));
        let u = unit("js/plugins.js/2/QuestDatas#0/Rewards/0", "x");
        assert_eq!(field_of(&u).as_deref(), Some("rewards"));
    }

    #[test]
    fn field_of_falls_back_to_payload() {
        let mut u = unit("Map003.json/2/0/5", "x");
        u.payload = r#"{"kind":"dialogue","field":"displayName"}"#.into();
        assert_eq!(field_of(&u).as_deref(), Some("displayname"));
    }

    #[test]
    fn empty_ruleset_is_identity() {
        let units = vec![unit("a#0/key", "x"), unit("b#0/title", "y")];
        let (out, rep) = apply(units.clone(), &RuleSet::new("rmmz"));
        assert_eq!(out.len(), units.len());
        assert_eq!(rep, ApplyReport::default());
    }

    #[test]
    fn skip_rule_removes_matching_units() {
        let units = vec![
            unit("js/plugins.js/0/data#0/key", "実績_a"),
            unit("js/plugins.js/0/data#0/title", "称号"),
        ];
        let rules = ruleset(vec![rule("key", Verdict::Skip, Scope::Any)]);
        let (out, rep) = apply(units, &rules);
        assert_eq!(rep.skipped, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(field_of(&out[0]).as_deref(), Some("title"));
    }

    #[test]
    fn scope_nested_does_not_touch_top_level() {
        let nested = unit("js/plugins.js/0/data#0/key", "実績_a");
        let top = unit("js/plugins.js/0/key", "実績_b");
        let rules = ruleset(vec![rule("key", Verdict::Skip, Scope::Nested)]);
        let (out, rep) = apply(vec![nested, top], &rules);
        assert_eq!(rep.skipped, 1, "only the nested unit is skipped");
        assert_eq!(out.len(), 1);
        assert!(!is_nested(&out[0]));
    }

    #[test]
    fn suffix_rule_matches_by_ending() {
        let units = vec![
            unit("a#0/commandText", "コマンド"),
            unit("a#0/other", "その他"),
        ];
        let rules = ruleset(vec![rule("*text", Verdict::Skip, Scope::Any)]);
        let (out, rep) = apply(units, &rules);
        assert_eq!(rep.skipped, 1);
        assert_eq!(field_of(&out[0]).as_deref(), Some("other"));
    }

    #[test]
    fn exact_rule_beats_suffix_rule() {
        // `*name` says skip, but the exact `displayname` says extract.
        let rules = ruleset(vec![
            rule("*name", Verdict::Skip, Scope::Any),
            rule("displayname", Verdict::Extract, Scope::Any),
        ]);
        let (out, rep) = apply(vec![unit("a#0/displayName", "名前")], &rules);
        assert_eq!(rep.skipped, 0, "exact extract rule wins");
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn skip_wins_ties_against_extract() {
        // Same specificity, opposite verdicts: the safe one must win.
        let rules = ruleset(vec![
            rule("key", Verdict::Extract, Scope::Any),
            rule("key", Verdict::Skip, Scope::Any),
        ]);
        let (out, rep) = apply(vec![unit("a#0/key", "実績_a")], &rules);
        assert_eq!(rep.skipped, 1);
        assert!(out.is_empty());
    }

    #[test]
    fn extract_rule_is_vetoed_for_machine_literals() {
        // A bad learned rule must never send switch ids / filenames to the LLM.
        let rules = ruleset(vec![rule("switchid", Verdict::Extract, Scope::Any)]);
        let (_out, rep) = apply(vec![unit("a#0/switchId", "12")], &rules);
        assert_eq!(rep.extract_vetoed, 1, "numeric value must trip the gate");

        let (_out, rep) = apply(vec![unit("a#0/switchId", "本当のテキスト")], &rules);
        assert_eq!(rep.extract_vetoed, 0, "real text passes");
    }

    #[test]
    fn machine_literal_covers_numbers_paths_and_scripts() {
        for s in ["12", "-3.5", "true", "", "img/pictures/a.png", "x.ogg"] {
            assert!(is_machine_literal(s), "{s:?} should be machine data");
        }
        for s in ["実績_a", "所持金", "Hello there"] {
            assert!(!is_machine_literal(s), "{s:?} should be text");
        }
    }

    #[test]
    fn ruleset_toml_roundtrip_preserves_semantics() {
        let rules = ruleset(vec![
            rule("key", Verdict::Skip, Scope::Nested),
            rule("*text", Verdict::Extract, Scope::Top),
        ]);
        let toml_str = toml::to_string_pretty(&rules).unwrap();
        let back: RuleSet = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.format, rules.format);
        assert_eq!(back.version, RULES_VERSION);
        assert_eq!(back.rules.len(), 2);
        assert_eq!(back.rules[0].verdict, Verdict::Skip);
        assert_eq!(back.rules[0].scope, Scope::Nested);
        assert_eq!(back.rules[1].field, "*text");
        assert_eq!(back.rules[1].scope, Scope::Top);
    }

    #[test]
    fn rule_file_missing_or_broken_degrades_to_empty() {
        // Point ATTX_HOME at a dir with a malformed file; extraction must not break.
        let dir = std::env::temp_dir().join(format!("attx-kn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("knowledge")).unwrap();
        std::fs::write(dir.join("knowledge/brokenfmt.toml"), "not = [valid").unwrap();
        // SAFETY: single-threaded test process section; no other thread reads env here.
        unsafe { std::env::set_var("ATTX_HOME", &dir) };
        let rs = load_rules("brokenfmt");
        assert!(rs.is_empty(), "malformed rules degrade to no learning");
        let rs = load_rules("nosuchfmt");
        assert!(rs.is_empty(), "missing file is not an error");
        unsafe { std::env::remove_var("ATTX_HOME") };
    }
}
