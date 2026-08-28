//! Evidence gathering for the experience layer.
//!
//! Turns what already happened in a workspace into *entries*: field-level
//! judgements plus free-form notes, each carrying the evidence that produced
//! it. This runs automatically after a successful writeback, so experience
//! accumulates without anyone remembering to ask for it.
//!
//! Two evidence sources:
//! * objective signals, free, straight out of the workspace DB (see `summarize`)
//! * optional LLM review that turns weak statistics into a reasoned entry
//!
//! Statistics are aggregated **per field name**, never per unit. One passthrough
//! is noise; eight of eight passthroughs under the same field name is a signal
//! that the field is not text at all.
//!
//! The approval asymmetry lives here: an entry that would *delete* text is
//! written `pending` and does nothing until a human approves it, while notes
//! and additive entries take effect on their own. A missed translation is
//! visible in `attx status`; a silently dropped line is not.

use crate::config::{LlmClient, Settings};
use crate::knowledge::{self, Entry, FieldEntry, NoteEntry, Scope, Status, Verdict};
use crate::model::{TextUnit, Translation, mask_controls};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Minimum units under a field name before it may be proposed. Below this the
/// sample is too small to distinguish a pattern from a coincidence.
const MIN_SAMPLE: usize = 4;
/// Fraction of a field's units that must show the signal.
const MIN_RATIO: f64 = 0.75;
/// Samples embedded in an entry's evidence so a human can judge it.
const MAX_SAMPLES: usize = 3;
/// Fraction of control-code-bearing units that must lose codes before it is
/// worth telling the model about.
const CTRL_LOSS_RATIO: f64 = 0.2;

/// Provenance marker for everything this module writes.
pub const SOURCE_AUTO: &str = "learn:auto";

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
    domains: BTreeSet<String>,
}

impl FieldStats {
    fn note_sample(&mut self, s: &str) {
        if self.samples.len() < MAX_SAMPLES && !s.trim().is_empty() {
            self.samples.push(truncate(s, 40));
        }
    }
}

/// Run-level tallies that become notes rather than field judgements.
#[derive(Debug, Default, Clone)]
struct RunStats {
    units: usize,
    translated: usize,
    passthrough: usize,
    /// Units whose source carried control codes and had a translation.
    ctrl_units: usize,
    /// …of those, how many lost at least one code.
    ctrl_lost: usize,
}

fn count_controls(lines: &[String]) -> usize {
    lines.iter().map(|l| mask_controls(l).1.len()).sum()
}

/// Collect per-field and run-level evidence from a workspace.
fn collect(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
) -> (BTreeMap<String, FieldStats>, RunStats) {
    let mut stats: BTreeMap<String, FieldStats> = BTreeMap::new();
    let mut run = RunStats {
        units: units.len(),
        ..Default::default()
    };
    for u in units {
        let tr = translations.get(&u.id);
        if let Some(t) = tr {
            if t.passthrough {
                run.passthrough += 1;
            } else {
                run.translated += 1;
                let before = count_controls(&u.original_lines);
                if before > 0 {
                    run.ctrl_units += 1;
                    if count_controls(&t.translation_lines) < before {
                        run.ctrl_lost += 1;
                    }
                }
            }
        }

        let Some(field) = knowledge::field_of(u) else {
            continue;
        };
        let e = stats.entry(field).or_default();
        e.total += 1;
        if u.location.contains('#') {
            e.nested += 1;
        }
        e.domains.insert(u.domain.clone());
        e.note_sample(&u.original_lines.join("\n"));
        if u.original_lines
            .iter()
            .all(|l| knowledge::is_machine_literal(l))
        {
            e.machine += 1;
        }
        if let Some(t) = tr {
            if t.passthrough {
                e.passthrough += 1;
            } else if t.translation_lines == u.original_lines {
                e.identical += 1;
            }
        }
    }
    (stats, run)
}

/// A field judgement plus the sample values that justify it. Kept as a struct
/// (rather than going straight to `Entry`) so `review_with_llm` has somewhere
/// to hang samples that never reach disk.
#[derive(Debug, Clone)]
struct Candidate {
    field: String,
    verdict: Verdict,
    scope: Scope,
    domain: String,
    confidence: f32,
    reason: String,
    evidence: Vec<String>,
    samples: Vec<String>,
}

impl Candidate {
    fn into_entry(self, now: &str) -> Entry {
        let mut fe = FieldEntry::new(&self.field, self.verdict, self.scope);
        // The asymmetry: deletions wait for a human, additions do not.
        fe.status = match self.verdict {
            Verdict::Skip => Status::Pending,
            Verdict::Extract => Status::Approved,
        };
        fe.domain = self.domain;
        fe.confidence = self.confidence;
        fe.reason = self.reason;
        fe.evidence = self
            .evidence
            .into_iter()
            .chain(self.samples.into_iter().map(|s| format!("sample: {s}")))
            .collect();
        fe.source = SOURCE_AUTO.into();
        fe.updated_at = now.to_string();
        Entry::Field(fe)
    }
}

/// Build candidates from evidence, skipping fields the merged experience
/// already decides.
fn propose(stats: &BTreeMap<String, FieldStats>, covered: &BTreeSet<String>) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (field, s) in stats {
        if s.total < MIN_SAMPLE || covered.contains(field) {
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
        // Same caution for the domain guard: only claim one when every unit
        // agreed, otherwise the entry would miss half its evidence.
        let domain = if s.domains.len() == 1 {
            s.domains.iter().next().cloned().unwrap_or_default()
        } else {
            String::new()
        };
        out.push(Candidate {
            field: field.clone(),
            verdict: Verdict::Skip,
            scope,
            domain,
            confidence: ratio as f32,
            reason: reason.to_string(),
            evidence: vec![detail],
            samples: s.samples.clone(),
        });
    }
    out
}

/// Run-level notes. Only signals that are objective *and* actionable next time
/// become notes — a per-run statistic nobody can act on is noise in a file
/// meant to be read.
fn notes_for(engine: &str, run: &RunStats, now: &str) -> Vec<Entry> {
    let mut out = Vec::new();

    let mut summary = NoteEntry::new(
        "run",
        &format!(
            "{engine}: 上次运行 units={}, translated={}, passthrough={}",
            run.units, run.translated, run.passthrough
        ),
    );
    summary.source = SOURCE_AUTO.into();
    summary.updated_at = now.to_string();
    out.push(Entry::Note(summary));

    // Control-code loss is the one prompt-worthy signal available for free:
    // the codes are countable before and after, and losing them breaks colour,
    // name substitution and message flow in-game.
    if run.ctrl_units > 0 {
        let ratio = run.ctrl_lost as f64 / run.ctrl_units as f64;
        if ratio >= CTRL_LOSS_RATIO {
            let mut n = NoteEntry::new(
                "prompt",
                &format!(
                    "该格式的译文容易丢失控制码（上次 {}/{} 条含控制码的文本丢了至少一个）。\
                     务必原样保留 [CTRL_n] 标记，数量与原文一致。",
                    run.ctrl_lost, run.ctrl_units
                ),
            );
            n.source = SOURCE_AUTO.into();
            n.updated_at = now.to_string();
            out.push(Entry::Note(n));
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryReport {
    pub format: String,
    pub units_scanned: usize,
    pub fields_seen: usize,
    /// Entries added or refreshed in the global experience file.
    pub entries_written: usize,
    /// Of those, how many need `attx learn review --approve`.
    pub pending: usize,
    pub notes: usize,
    pub file: String,
}

/// Scan a workspace for evidence and merge entries into the global experience
/// file for its format. Zero API cost unless `use_llm` is set.
pub fn summarize(workspace: &Path, use_llm: bool, settings: &Settings) -> Result<SummaryReport> {
    let store = crate::store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    let (stats, run) = collect(&units, &translations);

    // Coverage is checked against the *merged* view: the builtin defaults
    // already decide most field names, and re-proposing them every run would
    // bury the genuinely new signal.
    let merged = knowledge::load_experience(&meta.engine, Some(workspace));
    let covered = covered_fields(&merged);
    let mut candidates = propose(&stats, &covered);

    if use_llm && !candidates.is_empty() {
        match settings.client(None) {
            Ok(client) => review_with_llm(client, &mut candidates),
            Err(e) => eprintln!("learn: skipping LLM review ({e})"),
        }
    }

    let now = unix_now();
    let mut file = knowledge::load_file(&meta.engine);
    if file.format.is_empty() {
        file.format = meta.engine.clone();
    }
    let mut pending = 0usize;
    let mut written = 0usize;
    for c in candidates {
        let entry = c.into_entry(&now);
        if let Entry::Field(fe) = &entry
            && fe.status == Status::Pending
        {
            pending += 1;
        }
        file.upsert(entry);
        written += 1;
    }
    let notes = notes_for(&meta.engine, &run, &now);
    let note_count = notes.len();
    for n in notes {
        file.upsert(n);
    }
    let path = knowledge::save_file(&file)?;

    Ok(SummaryReport {
        format: meta.engine,
        units_scanned: units.len(),
        fields_seen: stats.len(),
        entries_written: written + note_count,
        pending,
        notes: note_count,
        file: path.display().to_string(),
    })
}

/// Field names the merged experience already has an entry for, in any status.
/// Pending entries count as covered so a re-run does not queue a duplicate the
/// human has not yet looked at.
fn covered_fields(exp: &knowledge::Experience) -> BTreeSet<String> {
    exp.entries
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Field(f) => Some(f.field.trim_start_matches('*').to_string()),
            _ => None,
        })
        .collect()
}

/// Ask the model to sanity-check each candidate. It may only *lower* confidence
/// or improve the reason — it cannot promote a candidate or invent new ones, so
/// a hallucinating model degrades to "no learning" rather than a bad entry.
fn review_with_llm(client: &LlmClient, candidates: &mut [Candidate]) {
    for p in candidates.iter_mut() {
        let prompt = format!(
            "字段名: {}\n统计证据: {}\n样本值:\n{}\n\n\
             这个字段是「游戏内部标识符/机器数据」还是「玩家可见文本」？\n\
             只回 JSON: {{\"identifier\": true|false, \"reason\": \"一句中文理由\"}}",
            p.field,
            p.evidence.join(", "),
            p.samples
                .iter()
                .map(|s| format!("  - {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        match crate::llm::ask_json(
            client,
            "你是本地化工程助手。只输出 JSON，不要解释。",
            &prompt,
        ) {
            Ok(v) => {
                let is_id = v
                    .get("identifier")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if let Some(r) = v.get("reason").and_then(|x| x.as_str())
                    && !r.trim().is_empty()
                {
                    p.reason = format!("{} (LLM: {})", p.reason, truncate(r, 80));
                }
                if !is_id {
                    // Model disagrees with the statistics — keep the candidate but
                    // mark it doubtful so a human looks harder.
                    p.confidence *= 0.5;
                    p.reason.push_str("；LLM 认为可能是可见文本，需人工判断");
                }
            }
            Err(e) => eprintln!("learn: LLM review failed for {}: {e:#}", p.field),
        }
    }
}


/// One pending entry, addressed by the same 1-based index `review` accepts.
pub struct PendingItem {
    pub format: String,
    pub index: usize,
    entry_index: usize,
    pub entry: Entry,
}

/// Every pending entry across all formats, in a stable order.
pub fn pending_items() -> Vec<PendingItem> {
    let mut out = Vec::new();
    let mut n = 0usize;
    for f in knowledge::all_files() {
        for (i, e) in f.entries.iter().enumerate() {
            let Entry::Field(fe) = e else { continue };
            if fe.status != Status::Pending {
                continue;
            }
            n += 1;
            out.push(PendingItem {
                format: f.format.clone(),
                index: n,
                entry_index: i,
                entry: e.clone(),
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewReport {
    pub approved: usize,
    pub rejected: usize,
    pub remaining: usize,
    pub files_written: Vec<String>,
}

/// Approve (flip to active) or reject (delete) pending entries by 1-based index.
pub fn review(approve: &[usize], reject: &[usize]) -> Result<ReviewReport> {
    let items = pending_items();
    // (entry_index, approve?) per format.
    let mut by_format: BTreeMap<String, Vec<(usize, bool)>> = BTreeMap::new();
    for it in &items {
        if approve.contains(&it.index) {
            by_format
                .entry(it.format.clone())
                .or_default()
                .push((it.entry_index, true));
        } else if reject.contains(&it.index) {
            by_format
                .entry(it.format.clone())
                .or_default()
                .push((it.entry_index, false));
        }
    }

    let now = unix_now();
    let mut approved = 0usize;
    let mut rejected = 0usize;
    let mut written = Vec::new();
    for (format, mut ops) in by_format {
        let mut file = knowledge::load_file(&format);
        // Highest index first: removing a low index would shift the rest.
        ops.sort_by_key(|op| std::cmp::Reverse(op.0));
        for (idx, ok) in ops {
            if idx >= file.entries.len() {
                continue;
            }
            if ok {
                if let Entry::Field(fe) = &mut file.entries[idx] {
                    fe.status = Status::Approved;
                    fe.updated_at = now.clone();
                }
                approved += 1;
            } else {
                file.entries.remove(idx);
                rejected += 1;
            }
        }
        written.push(knowledge::save_file(&file)?.display().to_string());
    }

    Ok(ReviewReport {
        approved,
        rejected,
        remaining: pending_items().len(),
        files_written: written,
    })
}

/// Remove entries by field name; returns how many were dropped.
pub fn forget(field: &str, format: Option<&str>) -> Result<usize> {
    let mut n = 0;
    for mut file in knowledge::all_files() {
        if let Some(f) = format
            && file.format != f
        {
            continue;
        }
        let before = file.entries.len();
        file.entries.retain(|e| match e {
            Entry::Field(fe) => fe.field != field,
            _ => true,
        });
        if file.entries.len() != before {
            n += before - file.entries.len();
            knowledge::save_file(&file)?;
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

    fn no_coverage() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn passthrough_cluster_becomes_a_pending_skip_entry() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..6 {
            let loc = format!("js/plugins.js/0/d#{i}/switchName");
            units.push(unit(&loc, "スイッチ"));
            trs.insert(loc.clone(), tr(&loc, &["スイッチ"], true));
        }
        let (stats, _) = collect(&units, &trs);
        let c = propose(&stats, &no_coverage());
        assert_eq!(c.len(), 1, "one field, one candidate");
        assert_eq!(c[0].field, "switchname");
        assert_eq!(c[0].verdict, Verdict::Skip);
        assert_eq!(c[0].scope, Scope::Nested);
        assert_eq!(c[0].domain, "plugins");

        // A deletion must never arrive pre-approved.
        let Entry::Field(fe) = c[0].clone().into_entry("0") else {
            panic!("expected field entry")
        };
        assert_eq!(fe.status, Status::Pending);
        assert!(
            fe.evidence.iter().any(|e| e.starts_with("sample: ")),
            "review needs sample values, not just a confidence number"
        );
    }

    #[test]
    fn small_samples_are_not_proposed() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..2 {
            let loc = format!("a#{i}/rareField");
            units.push(unit(&loc, "12"));
            trs.insert(loc.clone(), tr(&loc, &["12"], true));
        }
        let (stats, _) = collect(&units, &trs);
        assert!(propose(&stats, &no_coverage()).is_empty());
    }

    #[test]
    fn mixed_evidence_below_ratio_is_not_proposed() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..8 {
            let loc = format!("a#{i}/title");
            units.push(unit(&loc, "タイトル"));
            trs.insert(loc.clone(), tr(&loc, &["标题"], i < 2));
        }
        let (stats, _) = collect(&units, &trs);
        assert!(propose(&stats, &no_coverage()).is_empty());
    }

    #[test]
    fn covered_fields_are_not_reproposed() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..6 {
            let loc = format!("a#{i}/key");
            units.push(unit(&loc, "実績_x"));
            trs.insert(loc.clone(), tr(&loc, &["実績_x"], false));
        }
        let (stats, _) = collect(&units, &trs);
        let covered: BTreeSet<String> = ["key".to_string()].into_iter().collect();
        assert!(
            propose(&stats, &covered).is_empty(),
            "a re-run must not queue duplicates of what is already decided"
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
        let (stats, _) = collect(&units, &trs);
        let c = propose(&stats, &no_coverage());
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].scope,
            Scope::Any,
            "mixed nested/top evidence must not claim a narrower scope"
        );
    }

    #[test]
    fn mixed_domains_claim_no_domain_guard() {
        let mut units = Vec::new();
        for i in 0..3 {
            units.push(unit(&format!("a#{i}/code"), "123"));
        }
        for i in 0..3 {
            let mut u = unit(&format!("b#{i}/code"), "456");
            u.domain = "dialogue".into();
            units.push(u);
        }
        let (stats, _) = collect(&units, &BTreeMap::new());
        let c = propose(&stats, &no_coverage());
        assert_eq!(c.len(), 1);
        assert!(
            c[0].domain.is_empty(),
            "an entry must not claim a domain it only half covers"
        );
    }

    #[test]
    fn machine_literals_are_detected_without_translations() {
        let units: Vec<TextUnit> = (0..5)
            .map(|i| unit(&format!("a#{i}/iconIndex"), "42"))
            .collect();
        let (stats, _) = collect(&units, &BTreeMap::new());
        let c = propose(&stats, &no_coverage());
        assert_eq!(c.len(), 1);
        assert!(c[0].evidence[0].contains("machine"));
    }

    // ---- run-level notes ----

    #[test]
    fn control_code_loss_becomes_a_prompt_note() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..5 {
            let loc = format!("a#{i}/text");
            units.push(unit(&loc, r"\C[1]こんにちは"));
            // Translation dropped the colour code.
            trs.insert(loc.clone(), tr(&loc, &["你好"], false));
        }
        let (_, run) = collect(&units, &trs);
        assert_eq!(run.ctrl_units, 5);
        assert_eq!(run.ctrl_lost, 5);

        let notes = notes_for("rmmz", &run, "0");
        let prompt: Vec<_> = notes
            .iter()
            .filter_map(|e| match e {
                Entry::Note(n) if n.topic == "prompt" => Some(n),
                _ => None,
            })
            .collect();
        assert_eq!(prompt.len(), 1);
        assert!(prompt[0].text.contains("CTRL_"));
        assert_eq!(
            prompt[0].status,
            Status::Approved,
            "notes apply on their own"
        );
    }

    #[test]
    fn intact_control_codes_produce_no_prompt_note() {
        let mut units = Vec::new();
        let mut trs = BTreeMap::new();
        for i in 0..5 {
            let loc = format!("a#{i}/text");
            units.push(unit(&loc, r"\C[1]こんにちは"));
            trs.insert(loc.clone(), tr(&loc, &[r"\C[1]你好"], false));
        }
        let (_, run) = collect(&units, &trs);
        assert_eq!(run.ctrl_lost, 0);
        let notes = notes_for("rmmz", &run, "0");
        assert!(
            !notes
                .iter()
                .any(|e| matches!(e, Entry::Note(n) if n.topic == "prompt")),
            "no advice is better than advice about a problem that did not happen"
        );
    }

    #[test]
    fn run_note_replaces_itself_instead_of_piling_up() {
        let mut file = knowledge::ExperienceFile::new("rmmz");
        for i in 0..3 {
            let run = RunStats {
                units: i,
                ..Default::default()
            };
            for n in notes_for("rmmz", &run, "0") {
                file.upsert(n);
            }
        }
        let run_notes = file
            .entries
            .iter()
            .filter(|e| matches!(e, Entry::Note(n) if n.topic == "run"))
            .count();
        assert_eq!(run_notes, 1, "three runs must leave one run note");
    }

    #[test]
    fn a_human_note_is_not_overwritten_by_the_automatic_one() {
        let mut file = knowledge::ExperienceFile::new("rmmz");
        let mut mine = NoteEntry::new("run", "手写：这个游戏的存档在 www/save");
        mine.source = "human".into();
        file.upsert(Entry::Note(mine));
        for n in notes_for("rmmz", &RunStats::default(), "0") {
            file.upsert(n);
        }
        let kept = file
            .entries
            .iter()
            .filter(|e| matches!(e, Entry::Note(n) if n.source == "human"))
            .count();
        assert_eq!(
            kept, 1,
            "automatic summaries must not eat hand-written notes"
        );
    }
}
