//! Evidence gathering for the extraction knowledge layer.
//!
//! Turns what already happened in a workspace into *proposals*: candidate
//! rules with the evidence that produced them. Nothing here changes extraction
//! — proposals must be approved (`attx learn review`) before they become rules.
//!
//! Two evidence sources, per the design:
//! * objective signals, free, straight out of the workspace DB (see `scan`)
//! * optional LLM review that turns weak statistics into a reasoned proposal
//!
//! Statistics are aggregated **per field name**, never per unit. One passthrough
//! is noise; eight of eight passthroughs under the same field name is a signal
//! that the field is not text at all.

use crate::config::{LlmClient, Settings};
use crate::knowledge::{self, Rule, RuleSet, Scope, Verdict};
use crate::model::{TextUnit, Translation};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Minimum units under a field name before it may be proposed. Below this the
/// sample is too small to distinguish a pattern from a coincidence.
const MIN_SAMPLE: usize = 4;
/// Fraction of a field's units that must show the signal.
const MIN_RATIO: f64 = 0.75;
/// Samples embedded in a proposal so a human can judge it.
const MAX_SAMPLES: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub format: String,
    pub field: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub scope: Scope,
    pub confidence: f32,
    pub reason: String,
    pub evidence: Vec<String>,
    /// Example values, so approval is an informed decision rather than a
    /// confidence number taken on faith.
    pub samples: Vec<String>,
}

impl Proposal {
    fn into_rule(self, approved_at: String) -> Rule {
        Rule {
            field: self.field,
            verdict: self.verdict,
            scope: self.scope,
            confidence: self.confidence,
            reason: self.reason,
            evidence: self.evidence,
            approved_at,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProposalFile {
    #[serde(default, rename = "proposal")]
    pub proposals: Vec<Proposal>,
}

fn proposals_path() -> Result<PathBuf> {
    Ok(knowledge::knowledge_dir()?.join("proposals.toml"))
}

pub fn load_proposals() -> ProposalFile {
    let Ok(p) = proposals_path() else {
        return ProposalFile::default();
    };
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return ProposalFile::default();
    };
    match toml::from_str(&raw) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("learn: ignoring {}: {e}", p.display());
            ProposalFile::default()
        }
    }
}

pub fn save_proposals(f: &ProposalFile) -> Result<PathBuf> {
    let p = proposals_path()?;
    let body = toml::to_string_pretty(f).context("serialize proposals")?;
    std::fs::write(
        &p,
        format!(
            "# attx learning proposals — NOT active until approved.\n\
             # Review with `attx learn review`.\n{body}"
        ),
    )
    .with_context(|| format!("write {}", p.display()))?;
    Ok(p)
}

/// Per-field tallies collected from one workspace.
#[derive(Debug, Default, Clone)]
struct FieldStats {
    total: usize,
    nested: usize,
    /// Value is machine data despite having been extracted.
    machine: usize,
    /// Model handed the text back untouched.
    identical: usize,
    /// Model refused / failed and the original was kept.
    passthrough: usize,
    samples: Vec<String>,
}

impl FieldStats {
    fn note_sample(&mut self, s: &str) {
        if self.samples.len() < MAX_SAMPLES && !s.trim().is_empty() {
            self.samples.push(truncate(s, 40));
        }
    }
}

/// Collect per-field evidence from a workspace.
fn collect(units: &[TextUnit], translations: &BTreeMap<String, Translation>) -> BTreeMap<String, FieldStats> {
    let mut stats: BTreeMap<String, FieldStats> = BTreeMap::new();
    for u in units {
        let Some(field) = knowledge::field_of(u) else {
            continue;
        };
        let e = stats.entry(field).or_default();
        e.total += 1;
        if u.location.contains('#') {
            e.nested += 1;
        }
        let joined = u.original_lines.join("\n");
        e.note_sample(&joined);
        if u.original_lines
            .iter()
            .all(|l| knowledge::is_machine_literal(l))
        {
            e.machine += 1;
        }
        if let Some(tr) = translations.get(&u.id) {
            if tr.passthrough {
                e.passthrough += 1;
            } else if tr.translation_lines == u.original_lines {
                e.identical += 1;
            }
        }
    }
    stats
}

/// Build proposals from evidence, skipping fields an approved rule already covers.
fn propose(format: &str, stats: &BTreeMap<String, FieldStats>, existing: &RuleSet) -> Vec<Proposal> {
    let covered: Vec<&str> = existing.rules.iter().map(|r| r.field.as_str()).collect();
    let mut out = Vec::new();
    for (field, s) in stats {
        if s.total < MIN_SAMPLE || covered.contains(&field.as_str()) {
            continue;
        }
        // Signals that the field is not player-visible text at all.
        let bad = s.machine.max(s.passthrough).max(s.identical);
        let ratio = bad as f64 / s.total as f64;
        if ratio < MIN_RATIO {
            continue;
        }
        let (reason, detail) = if s.machine >= s.passthrough && s.machine >= s.identical {
            (
                "值为机器数据（数字 / 路径 / 脚本），却被提取",
                format!("machine={}/{}", s.machine, s.total),
            )
        } else if s.passthrough >= s.identical {
            (
                "模型反复拒绝翻译，疑似非文本字段",
                format!("passthrough={}/{}", s.passthrough, s.total),
            )
        } else {
            (
                "译文与原文完全一致，疑似标识符而非文本",
                format!("identical={}/{}", s.identical, s.total),
            )
        };
        // Only claim `nested` when the evidence is entirely nested; a mixed
        // field must not have its top-level occurrences silently dropped.
        let scope = if s.nested == s.total {
            Scope::Nested
        } else if s.nested == 0 {
            Scope::Top
        } else {
            Scope::Any
        };
        out.push(Proposal {
            format: format.to_string(),
            field: field.clone(),
            verdict: Verdict::Skip,
            scope,
            confidence: ratio as f32,
            reason: reason.to_string(),
            evidence: vec![detail],
            samples: s.samples.clone(),
        });
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub format: String,
    pub units_scanned: usize,
    pub fields_seen: usize,
    pub new_proposals: usize,
    pub total_pending: usize,
}

/// Scan a workspace for evidence and merge new proposals into the pending file.
pub fn scan(workspace: &Path, use_llm: bool, settings: &Settings) -> Result<ScanReport> {
    let store = crate::store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    let stats = collect(&units, &translations);
    let existing = knowledge::load_rules(&meta.engine);
    let mut fresh = propose(&meta.engine, &stats, &existing);

    if use_llm && !fresh.is_empty() {
        match settings.client(None) {
            Ok(client) => review_with_llm(client, &mut fresh),
            Err(e) => eprintln!("learn: skipping LLM review ({e})"),
        }
    }

    let mut file = load_proposals();
    let mut added = 0usize;
    for p in fresh {
        let dup = file
            .proposals
            .iter()
            .any(|q| q.format == p.format && q.field == p.field);
        if !dup {
            file.proposals.push(p);
            added += 1;
        }
    }
    if added > 0 {
        save_proposals(&file)?;
    }
    Ok(ScanReport {
        format: meta.engine,
        units_scanned: units.len(),
        fields_seen: stats.len(),
        new_proposals: added,
        total_pending: file.proposals.len(),
    })
}

/// Ask the model to sanity-check each proposal. It may only *lower* confidence
/// or improve the reason — it cannot promote a proposal or invent new ones, so
/// a hallucinating model degrades to "no learning" rather than a bad rule.
fn review_with_llm(client: &LlmClient, proposals: &mut [Proposal]) {
    for p in proposals.iter_mut() {
        let prompt = format!(
            "字段名: {}\n格式: {}\n统计证据: {}\n样本值:\n{}\n\n\
             这个字段是「游戏内部标识符/机器数据」还是「玩家可见文本」？\n\
             只回 JSON: {{\"identifier\": true|false, \"reason\": \"一句中文理由\"}}",
            p.field,
            p.format,
            p.evidence.join(", "),
            p.samples
                .iter()
                .map(|s| format!("  - {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        match ask_json(client, &prompt) {
            Ok(v) => {
                let is_id = v.get("identifier").and_then(|x| x.as_bool()).unwrap_or(false);
                if let Some(r) = v.get("reason").and_then(|x| x.as_str())
                    && !r.trim().is_empty()
                {
                    p.reason = format!("{} (LLM: {})", p.reason, truncate(r, 80));
                }
                if !is_id {
                    // Model disagrees with the statistics — keep the proposal but
                    // mark it doubtful so a human looks harder.
                    p.confidence *= 0.5;
                    p.reason.push_str("；LLM 认为可能是可见文本，需人工判断");
                }
            }
            Err(e) => eprintln!("learn: LLM review failed for {}: {e:#}", p.field),
        }
    }
}

fn ask_json(client: &LlmClient, prompt: &str) -> Result<serde_json::Value> {
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(client.timeout.max(30)))
        .build()?;
    let url = format!("{}/chat/completions", client.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": client.model,
        "temperature": 0.0,
        "messages": [
            {"role": "system", "content": "你是本地化工程助手。只输出 JSON，不要解释。"},
            {"role": "user", "content": prompt}
        ]
    });
    let resp = http
        .post(&url)
        .bearer_auth(&client.api_key)
        .json(&body)
        .send()
        .with_context(|| format!("POST {url}"))?;
    let text = resp.text()?;
    let parsed: serde_json::Value = serde_json::from_str(&text).context("decode chat")?;
    let content = parsed["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no content"))?;
    let start = content.find('{').unwrap_or(0);
    let end = content.rfind('}').map(|i| i + 1).unwrap_or(content.len());
    serde_json::from_str(&content[start..end]).context("parse review json")
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewReport {
    pub approved: usize,
    pub rejected: usize,
    pub remaining: usize,
    pub files_written: Vec<String>,
}

/// Approve / reject pending proposals by 1-based index.
pub fn review(approve: &[usize], reject: &[usize]) -> Result<ReviewReport> {
    let mut file = load_proposals();
    let now = unix_now();
    let mut by_format: BTreeMap<String, Vec<Rule>> = BTreeMap::new();
    let mut approved = 0usize;
    let mut rejected = 0usize;
    let mut keep = Vec::new();

    for (i, p) in file.proposals.drain(..).enumerate() {
        let n = i + 1;
        if approve.contains(&n) {
            by_format
                .entry(p.format.clone())
                .or_default()
                .push(p.into_rule(now.clone()));
            approved += 1;
        } else if reject.contains(&n) {
            rejected += 1;
        } else {
            keep.push(p);
        }
    }

    let mut written = Vec::new();
    for (format, rules) in by_format {
        let mut set = knowledge::load_rules(&format);
        if set.format.is_empty() {
            set = RuleSet::new(&format);
        }
        for r in rules {
            set.rules.retain(|x| x.field != r.field);
            set.rules.push(r);
        }
        let p = knowledge::save_rules(&set)?;
        written.push(p.display().to_string());
    }

    file.proposals = keep;
    save_proposals(&file)?;
    Ok(ReviewReport {
        approved,
        rejected,
        remaining: file.proposals.len(),
        files_written: written,
    })
}

/// Remove a rule by field name; returns how many were dropped.
pub fn forget(field: &str, format: Option<&str>) -> Result<usize> {
    let mut n = 0;
    for mut set in knowledge::all_rules() {
        if let Some(f) = format
            && set.format != f
        {
            continue;
        }
        let before = set.rules.len();
        set.rules.retain(|r| r.field != field);
        if set.rules.len() != before {
            n += before - set.rules.len();
            knowledge::save_rules(&set)?;
        }
    }
    Ok(n)
}

fn truncate(s: &str, n: usize) -> String {
    let one_line = s.replace('\n', " ");
    let mut t: String = one_line.chars().take(n).collect();
    if one_line.chars().count() > n {
        t.push('…');
    }
    t
}

fn unix_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
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

    fn tr(id: &str, lines: &[&str], passthrough: bool) -> Translation {
        Translation {
            unit_id: id.into(),
            translation_lines: lines.iter().map(|s| s.to_string()).collect(),
            source_hash: String::new(),
            passthrough,
        }
    }

    #[test]
    fn passthrough_cluster_becomes_a_skip_proposal() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..6 {
            let loc = format!("js/plugins.js/0/d#{i}/switchName");
            units.push(unit(&loc, "スイッチ"));
            trs.insert(loc.clone(), tr(&loc, &["スイッチ"], true));
        }
        let stats = collect(&units, &trs);
        let proposals = propose("rmmz", &stats, &RuleSet::new("rmmz"));
        assert_eq!(proposals.len(), 1, "one field, one proposal");
        let p = &proposals[0];
        assert_eq!(p.field, "switchname");
        assert_eq!(p.verdict, Verdict::Skip);
        assert_eq!(p.scope, Scope::Nested);
        assert!(!p.samples.is_empty(), "must carry samples for review");
        assert!(p.evidence[0].contains("passthrough"));
    }

    #[test]
    fn small_samples_are_not_proposed() {
        // Below MIN_SAMPLE the pattern cannot be told from coincidence.
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..2 {
            let loc = format!("a#{i}/rareField");
            units.push(unit(&loc, "12"));
            trs.insert(loc.clone(), tr(&loc, &["12"], true));
        }
        let stats = collect(&units, &trs);
        assert!(propose("rmmz", &stats, &RuleSet::new("rmmz")).is_empty());
    }

    #[test]
    fn mixed_evidence_below_ratio_is_not_proposed() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..8 {
            let loc = format!("a#{i}/title");
            units.push(unit(&loc, "タイトル"));
            // only 2 of 8 passthrough — well under MIN_RATIO
            trs.insert(loc.clone(), tr(&loc, &["标题"], i < 2));
        }
        let stats = collect(&units, &trs);
        assert!(propose("rmmz", &stats, &RuleSet::new("rmmz")).is_empty());
    }

    #[test]
    fn fields_already_covered_are_not_reproposed() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..6 {
            let loc = format!("a#{i}/key");
            units.push(unit(&loc, "実績_x"));
            trs.insert(loc.clone(), tr(&loc, &["実績_x"], false)); // identical
        }
        let stats = collect(&units, &trs);
        let mut existing = RuleSet::new("rmmz");
        existing.rules.push(Rule {
            field: "key".into(),
            verdict: Verdict::Skip,
            scope: Scope::Any,
            confidence: 1.0,
            reason: "already known".into(),
            evidence: vec![],
            approved_at: "0".into(),
        });
        assert!(
            propose("rmmz", &stats, &existing).is_empty(),
            "scan must not repeat known rules every run"
        );
    }

    #[test]
    fn mixed_scope_stays_any_so_top_level_is_not_dropped() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..3 {
            let loc = format!("a#{i}/code");
            units.push(unit(&loc, "123"));
            trs.insert(loc.clone(), tr(&loc, &["123"], true));
        }
        for i in 0..3 {
            let loc = format!("a/{i}/code");
            units.push(unit(&loc, "456"));
            trs.insert(loc.clone(), tr(&loc, &["456"], true));
        }
        let stats = collect(&units, &trs);
        let p = propose("rmmz", &stats, &RuleSet::new("rmmz"));
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].scope,
            Scope::Any,
            "mixed nested/top evidence must not claim a narrower scope"
        );
    }

    #[test]
    fn machine_literals_are_detected_without_translations() {
        // Evidence works even before anything is translated.
        let units: Vec<TextUnit> = (0..5)
            .map(|i| unit(&format!("a#{i}/iconIndex"), "42"))
            .collect();
        let stats = collect(&units, &BTreeMap::new());
        let p = propose("rmmz", &stats, &RuleSet::new("rmmz"));
        assert_eq!(p.len(), 1);
        assert!(p[0].evidence[0].contains("machine"));
    }

    #[test]
    fn proposal_toml_roundtrip() {
        let f = ProposalFile {
            proposals: vec![Proposal {
                format: "rmmz".into(),
                field: "key".into(),
                verdict: Verdict::Skip,
                scope: Scope::Nested,
                confidence: 0.9,
                reason: "身份句柄".into(),
                evidence: vec!["identical=6/6".into()],
                samples: vec!["実績_a".into()],
            }],
        };
        let s = toml::to_string_pretty(&f).unwrap();
        let back: ProposalFile = toml::from_str(&s).unwrap();
        assert_eq!(back.proposals.len(), 1);
        assert_eq!(back.proposals[0].field, "key");
        assert_eq!(back.proposals[0].verdict, Verdict::Skip);
        assert_eq!(back.proposals[0].samples, vec!["実績_a".to_string()]);
    }
}
