//! Glossary: one agreed translation per proper noun, for a whole work.
//!
//! Without one, a model translating a long novel or game will render the same
//! character name differently across batches — `アレイ` becomes 艾蕾 in chapter
//! one and 埃雷 in chapter nine, and the reader cannot tell they are the same
//! person. Batching makes this structural, not accidental: no single request
//! ever sees enough of the work to be consistent with the rest of it.
//!
//! Extraction is LLM-based throughout (the LinguaGacha strategy):
//!
//! ```text
//! source batches → model emits {src,dst,info} → substring gate
//!   → min_occurrences gate (real source hits) → vote/cap → inject
//! ```
//!
//! No regex mining: heuristics only see katakana runs and capitalised words,
//! so organisations, items, skills and world concepts never surface. The model
//! reads the raw source and decides what is a term; the mechanical gates are
//! only anti-hallucination (src must be a real substring) and cost control
//! (a term must actually occur at least `min_occurrences` times).
//!
//! One failure mode shapes the design: a wrong `dst` is applied everywhere,
//! which makes it *harder* to spot than a one-off mistranslation. Hence
//! `check`, and hence entries stay editable.

use crate::config::Settings;
use crate::knowledge;
use crate::llm;
use crate::model::{TextUnit, Translation};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const GLOSSARY_VERSION: u32 = 1;
pub const GLOSSARY_FILE: &str = "glossary.toml";

/// Source characters per extract request.
const EXTRACT_BATCH_CHARS: usize = 3500;
/// Hard cap on extract batches so a huge workspace cannot open an unbounded bill.
/// ponytail: raise or make configurable if novels routinely need deeper coverage.
const EXTRACT_MAX_BATCHES: usize = 40;

/// Allowed `info` / type labels from llm extract (LinguaGacha-style whitelist).
const LLM_INFO_OK: &[&str] = &[
    "男性角色",
    "女性角色",
    "未知性别角色",
    "地名",
    "家族",
    "组织",
    "特殊物品",
    "特殊技能",
    "特殊生物",
    "特殊概念",
    "其他",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TermStatus {
    #[default]
    Active,
    /// The model judged this candidate not to be a proper noun. Kept rather
    /// than deleted so the next build does not pay to ask again.
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub src: String,
    #[serde(default)]
    pub dst: String,
    /// Disambiguating description ("女性名字", "地点"). Not decoration: without
    /// it the model cannot tell `アレイさん` should be 艾蕾小姐 and not 埃雷先生.
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub status: TermStatus,
    #[serde(default)]
    pub source: String,
    /// English `May` vs `may`. CJK is unaffected. Default off.
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Glossary {
    // Declaration order is emission order: scalars before the term array.
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default, rename = "term")]
    pub terms: Vec<GlossaryTerm>,
}

fn default_version() -> u32 {
    GLOSSARY_VERSION
}

impl Default for Glossary {
    fn default() -> Self {
        Self {
            version: GLOSSARY_VERSION,
            source_lang: String::new(),
            target_lang: String::new(),
            terms: Vec::new(),
        }
    }
}

impl Glossary {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn active(&self) -> Vec<GlossaryTerm> {
        self.terms
            .iter()
            .filter(|t| t.status == TermStatus::Active && !t.dst.is_empty())
            .cloned()
            .collect()
    }

    fn find(&self, src: &str) -> Option<&GlossaryTerm> {
        self.terms.iter().find(|t| t.src == src)
    }

    /// Replace a term with the same `src`, else append.
    pub fn upsert(&mut self, term: GlossaryTerm) {
        self.terms.retain(|t| t.src != term.src);
        self.terms.push(term);
    }

    pub fn remove(&mut self, src: &str) -> bool {
        let before = self.terms.len();
        self.terms.retain(|t| t.src != src);
        self.terms.len() != before
    }
}

// ---- storage ----

pub fn path(workspace: &Path) -> PathBuf {
    workspace.join(GLOSSARY_FILE)
}

/// Load a workspace glossary. Missing file → empty (not an error); a malformed
/// one is reported and treated as empty so a bad hand-edit degrades to "no
/// glossary" rather than blocking translation.
pub fn load(workspace: &Path) -> Glossary {
    let p = path(workspace);
    if !p.is_file() {
        return Glossary::default();
    }
    match std::fs::read_to_string(&p)
        .map_err(anyhow::Error::from)
        .and_then(|raw| toml::from_str::<Glossary>(&raw).map_err(anyhow::Error::from))
    {
        Ok(g) => g,
        Err(e) => {
            eprintln!("glossary: ignoring {}: {e:#}", p.display());
            Glossary::default()
        }
    }
}

pub fn save(workspace: &Path, g: &Glossary) -> Result<PathBuf> {
    let p = path(workspace);
    let body = toml::to_string_pretty(g).context("serialize glossary")?;
    let header = "# attx glossary — one agreed translation per proper noun.\n\
                  # Edit freely: `attx glossary list` shows what is active,\n\
                  # `attx glossary check` reports terms the translation ignored.\n";
    std::fs::write(&p, format!("{header}{body}"))
        .with_context(|| format!("write {}", p.display()))?;
    Ok(p)
}

/// Fill in the language pair from the workspace when it is not set yet.
///
/// `build` knows the languages because it reads the DB anyway; a glossary
/// created by `add` or `import` would otherwise carry no record of which
/// direction it is for, which matters once the file is shared or re-imported.
/// A workspace that cannot be opened is not an error here — the terms are
/// still worth saving.
pub fn ensure_langs(workspace: &Path, g: &mut Glossary) {
    if !g.source_lang.is_empty() && !g.target_lang.is_empty() {
        return;
    }
    if let Ok(store) = crate::store::workspace_db(workspace)
        && let Ok(meta) = store.meta()
    {
        if g.source_lang.is_empty() {
            g.source_lang = meta.source_lang;
        }
        if g.target_lang.is_empty() {
            g.target_lang = meta.target_lang;
        }
    }
}

// ---- occurrence counting ----

/// Count how many lines of source text contain `term`.
///
/// A line-level count rather than a substring-count: overlapping matches are
/// ambiguous, and one occurrence per dialogue line is what "this name keeps
/// coming back" means in a game or novel. Used to gate and rank LLM-extracted
/// terms — votes only say *the model saw it in a sample batch*; this says
/// *the work itself keeps using it*.
pub fn count_occurrences(units: &[TextUnit], term: &str) -> usize {
    units
        .iter()
        .filter(|u| u.original_lines.iter().any(|l| l.contains(term)))
        .count()
}

// ---- build ----

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub candidates: usize,
    /// Terms whose real occurrence count met `min_occurrences`.
    pub above_threshold: usize,
    /// Candidates dropped by `max_terms`. Reported rather than silently cut:
    /// a truncated run that looks complete is worse than a noisy one.
    pub truncated: usize,
    pub asked: usize,
    pub added: usize,
    pub rejected: usize,
    pub total_active: usize,
    pub min_occurrences: usize,
    pub dry_run: bool,
    pub file: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sample: Vec<String>,
}

/// Build a glossary for a workspace: LLM extraction over source batches.
///
/// `min_occurrences` overrides `[glossary]` when set (CLI flag).
pub fn build(
    workspace: &Path,
    settings: &Settings,
    min_occurrences: Option<usize>,
    dry_run: bool,
) -> Result<BuildReport> {
    let store = crate::store::workspace_db(workspace)?;
    let meta = store.meta()?;
    let units = store.all_units()?;
    if units.is_empty() {
        bail!(
            "no extracted units in {}; run `attx extract` first",
            workspace.display()
        );
    }
    build_llm(workspace, settings, &meta, &units, min_occurrences, dry_run)
}

fn build_llm(
    workspace: &Path,
    settings: &Settings,
    meta: &crate::model::WorkspaceMeta,
    units: &[TextUnit],
    min_occurrences: Option<usize>,
    dry_run: bool,
) -> Result<BuildReport> {
    let max_terms = settings.glossary.max_terms.max(1);
    // A floor of 1 keeps the flag a *threshold* on recurrence: an LLM-extracted
    // term that never re-occurs in the source is noise or a hallucination, and
    // the substring gate already guarantees the one hit.
    let min_occ = min_occurrences.unwrap_or(settings.glossary.min_occurrences).max(1);
    let batches = source_batches(units, EXTRACT_BATCH_CHARS, EXTRACT_MAX_BATCHES);
    let asked = batches.len();

    let mut glossary = load(workspace);
    glossary.source_lang = meta.source_lang.clone();
    glossary.target_lang = meta.target_lang.clone();

    let sample: Vec<String> = batches
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, b)| format!("batch{}:{}c", i + 1, b.chars().count()))
        .collect();

    if dry_run {
        return Ok(BuildReport {
            candidates: 0,
            above_threshold: 0,
            truncated: 0,
            asked,
            added: 0,
            rejected: 0,
            total_active: glossary.active().len(),
            min_occurrences: min_occ,
            dry_run: true,
            file: path(workspace).display().to_string(),
            sample,
        });
    }

    if batches.is_empty() {
        let file = path(workspace);
        return Ok(BuildReport {
            candidates: 0,
            above_threshold: 0,
            truncated: 0,
            asked: 0,
            added: 0,
            rejected: 0,
            total_active: glossary.active().len(),
            min_occurrences: min_occ,
            dry_run: false,
            file: file.display().to_string(),
            sample: vec![],
        });
    }

    let client = crate::config::require_llm(settings)?;
    // Aggregate votes across batches: same src may appear many times with
    // slightly different dst/info; majority wins, ties keep first-seen order.
    let mut dst_votes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut info_votes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut raw_hits = 0usize;

    for (i, batch) in batches.iter().enumerate() {
        match extract_terms_llm(client, batch, &meta.source_lang, &meta.target_lang) {
            Ok(rows) => {
                for row in rows {
                    if !accept_llm_term(units, &row) {
                        continue;
                    }
                    if glossary.find(&row.src).is_some() {
                        continue; // already decided (incl. rejected)
                    }
                    raw_hits += 1;
                    *dst_votes
                        .entry(row.src.clone())
                        .or_default()
                        .entry(row.dst)
                        .or_insert(0) += 1;
                    *info_votes
                        .entry(row.src)
                        .or_default()
                        .entry(row.info)
                        .or_insert(0) += 1;
                }
            }
            Err(e) => eprintln!("glossary: extract batch {}/{} failed: {e:#}", i + 1, asked),
        }
    }

    // Rank by real occurrences in the source (not batch votes — those only say
    // the sampled batches happened to contain the term). Namebox speaker plates
    // bypass the floor and stay at the front so max_terms cannot drop them.
    let namebox_set: BTreeSet<String> = namebox_terms(units)
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    let mut ranked: Vec<(String, usize)> = dst_votes
        .keys()
        .map(|src| (src.clone(), count_occurrences(units, src)))
        .filter(|(src, n)| *n >= min_occ || namebox_set.contains(src))
        .collect();
    let candidates = dst_votes.len();
    let above_threshold = ranked.len();
    ranked.sort_by(|a, b| {
        let an = namebox_set.contains(&a.0);
        let bn = namebox_set.contains(&b.0);
        bn.cmp(&an)
            .then(b.1.cmp(&a.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let truncated = above_threshold.saturating_sub(max_terms);
    if truncated > 0 {
        eprintln!(
            "glossary: {truncated} term(s) dropped by max_terms={max_terms}; \
             raise it or lower min_occurrences to change what is covered"
        );
    }

    let mut added = 0usize;
    let mut sample = Vec::new();
    for (src, count) in ranked.into_iter().take(max_terms) {
        let dst = winner(dst_votes.get(&src).unwrap());
        let info = info_votes
            .get(&src)
            .map(winner)
            .unwrap_or_else(|| "其他".into());
        let info = normalize_info(&info).unwrap_or_else(|| "其他".into());
        if dst.is_empty() || dst == src {
            continue;
        }
        if sample.len() < 10 {
            sample.push(format!("{src}→{dst} ({count})"));
        }
        glossary.upsert(GlossaryTerm {
            src,
            dst,
            info,
            count,
            status: TermStatus::Active,
            source: "auto:llm".into(),
            case_sensitive: false,
        });
        added += 1;
    }

    let file = save(workspace, &glossary)?;
    Ok(BuildReport {
        candidates,
        above_threshold,
        truncated,
        asked,
        added,
        rejected: raw_hits.saturating_sub(added), // rough: filtered/duped/capped
        total_active: glossary.active().len(),
        min_occurrences: min_occ,
        dry_run: false,
        file: file.display().to_string(),
        sample,
    })
}

/// Pack unit lines into ~`budget` char batches, oldest units first, hard-capped.
fn source_batches(units: &[TextUnit], budget: usize, max_batches: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for u in units {
        for line in &u.original_lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let add = if cur.is_empty() {
                line.to_string()
            } else {
                format!("\n{line}")
            };
            if !cur.is_empty() && cur.chars().count() + add.chars().count() > budget {
                out.push(std::mem::take(&mut cur));
                if out.len() >= max_batches {
                    return out;
                }
            }
            cur.push_str(&add);
        }
    }
    if !cur.is_empty() && out.len() < max_batches {
        out.push(cur);
    }
    out
}

#[derive(Debug, Clone)]
struct ExtractedTerm {
    src: String,
    dst: String,
    info: String,
}

fn extract_terms_llm(
    client: &crate::config::LlmClient,
    batch: &str,
    src_lang: &str,
    dst_lang: &str,
) -> Result<Vec<ExtractedTerm>> {
    let system = "你是本地化术语工程师。只输出 JSON 数组，不要解释、不要 Markdown。";
    let user = format!(
        "下面是一部作品的 {src_lang} 原文片段。请提取**应当在全作品统一译名的专有名词或作品特有概念**，\
         并给出 {dst_lang} 译名。\n\n\
         规则：\n\
         - 术语必须是原文中的连续子字符串（子字符串原则）\n\
         - 精准边界：只截取必要的连续字符，去掉修饰称谓（如「骑士艾琳」→「艾琳」，\
           「黑木家族的族长」→「黑木家族」）\n\
         - info 必须且只能是：男性角色、女性角色、未知性别角色、地名、家族、组织、\
           特殊物品、特殊技能、特殊生物、特殊概念、其他\n\
         - 应当提取：独创专有词汇（人名、地名、家族、组织、专有物品/技能/生物/概念），\
           例如「临冬城」（地名）、「凤凰社」（组织）、「魔力回路」（特殊概念）\n\
         - 禁止：泛用词（剑/魔法/城堡/公会）、泛用称谓职业（先生/战士/商人）、整句、变量名\n\
         - 合并重复；同一概念只留一条\n\
         - 若本段没有专有名词，输出 []\n\n\
         只输出 JSON 数组：[{{\"src\":\"...\",\"dst\":\"...\",\"info\":\"...\"}}]\n\n\
         原文：\n{batch}"
    );
    let v = llm::ask_json(client, system, &user)?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("expected a JSON array of terms"))?;
    Ok(arr
        .iter()
        .filter_map(|item| {
            let src = item.get("src")?.as_str()?.trim();
            let dst = item
                .get("dst")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            // accept type as alias for info (LinguaGacha JSONL uses type)
            let info = item
                .get("info")
                .or_else(|| item.get("type"))
                .and_then(|x| x.as_str())
                .unwrap_or("其他")
                .trim();
            if src.is_empty() || dst.is_empty() {
                return None;
            }
            Some(ExtractedTerm {
                src: src.to_string(),
                dst: dst.to_string(),
                info: info.to_string(),
            })
        })
        .collect())
}

fn accept_llm_term(units: &[TextUnit], row: &ExtractedTerm) -> bool {
    let src = row.src.trim();
    let dst = row.dst.trim();
    if src.chars().count() < 2 || dst.is_empty() {
        return false;
    }
    if knowledge::is_machine_literal(src) {
        return false;
    }
    if !units
        .iter()
        .any(|u| u.original_lines.iter().any(|l| l.contains(src)))
    {
        return false;
    }
    normalize_info(&row.info).is_some()
}

/// Map a model/human info label onto the LinguaGacha-style whitelist.
pub fn normalize_info(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return Some("其他".into());
    }
    if LLM_INFO_OK.contains(&t) {
        return Some(t.to_string());
    }
    let lower = t.to_ascii_lowercase();
    if lower == "other" || lower == "others" {
        return Some("其他".into());
    }
    let mapped = match (t, lower.as_str()) {
        ("男" | "男性", _) | (_, "male") => "男性角色",
        ("女" | "女性", _) | (_, "female") => "女性角色",
        (_, "place" | "location") => "地名",
        (_, "org" | "organization") => "组织",
        (_, "family" | "clan") => "家族",
        (_, "item") => "特殊物品",
        (_, "skill") => "特殊技能",
        (_, "creature" | "monster") => "特殊生物",
        _ => return None,
    };
    Some(mapped.into())
}

pub fn is_namebox(u: &TextUnit) -> bool {
    u.domain == "namebox" || u.role == "namebox"
}

/// Nameplate strings that must enter the glossary even if the miner missed them.
pub fn namebox_terms(units: &[TextUnit]) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for u in units {
        if !is_namebox(u) {
            continue;
        }
        let src = u.joined_text().trim().to_string();
        if src.chars().count() < 2 || knowledge::is_machine_literal(&src) {
            continue;
        }
        *counts.entry(src).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

pub fn term_hits_text(term: &GlossaryTerm, text: &str) -> bool {
    if term.src.is_empty() {
        return false;
    }
    if term.case_sensitive {
        text.contains(&term.src)
    } else if text.contains(&term.src) {
        true
    } else {
        text.to_lowercase().contains(&term.src.to_lowercase())
    }
}

fn winner(votes: &BTreeMap<String, usize>) -> String {
    votes
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())
        .unwrap_or_default()
}

// ---- per-batch injection ----

/// Terms a batch of source text actually contains, highest count first.
///
/// `terms` is expected sorted by count descending (see `Translator::with_glossary`),
/// so `take(limit)` keeps the most load-bearing names when a batch has more
/// matches than the budget allows.
pub fn select_for_batch<'a>(
    terms: &'a [GlossaryTerm],
    text: &str,
    limit: usize,
) -> Vec<&'a GlossaryTerm> {
    terms
        .iter()
        .filter(|t| {
            t.status == TermStatus::Active
                && !t.dst.is_empty()
                && !t.src.is_empty()
                && term_hits_text(t, text)
        })
        .take(limit)
        .collect()
}

// ---- post-translation check ----

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub src: String,
    pub dst: String,
    /// Translated units whose source contained `src`.
    pub occurrences: usize,
    /// …of those, how many actually used `dst`.
    pub applied: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub active_terms: usize,
    pub terms_seen: usize,
    pub terms_fully_applied: usize,
    pub violations: Vec<Violation>,
}

/// Report glossary terms the translation did not actually use.
///
/// A substring test, so an inflected target language can produce false
/// positives — it is a review aid, not a gate.
pub fn check(workspace: &Path) -> Result<CheckReport> {
    let store = crate::store::workspace_db(workspace)?;
    let units = store.all_units()?;
    let translations = store.all_translations()?;
    let glossary = load(workspace);
    Ok(check_units(&units, &translations, &glossary))
}

pub fn check_units(
    units: &[TextUnit],
    translations: &BTreeMap<String, Translation>,
    glossary: &Glossary,
) -> CheckReport {
    let active = glossary.active();
    let mut violations = Vec::new();
    let mut seen = 0usize;
    let mut ok = 0usize;
    for t in &active {
        let mut occurrences = 0usize;
        let mut applied = 0usize;
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            if tr.passthrough {
                continue;
            }
            let src_text = u.original_lines.join("\n");
            if !term_hits_text(t, &src_text) {
                continue;
            }
            occurrences += 1;
            if tr.translation_lines.iter().any(|l| l.contains(&t.dst)) {
                applied += 1;
            }
        }
        if occurrences == 0 {
            continue;
        }
        seen += 1;
        if applied == occurrences {
            ok += 1;
        } else {
            violations.push(Violation {
                src: t.src.clone(),
                dst: t.dst.clone(),
                occurrences,
                applied,
            });
        }
    }
    violations.sort_by_key(|v| std::cmp::Reverse(v.occurrences - v.applied));
    CheckReport {
        active_terms: active.len(),
        terms_seen: seen,
        terms_fully_applied: ok,
        violations,
    }
}

// ---- import / export ----

/// Import terms from JSON. Two shapes are accepted:
/// * `[{"src": "...", "dst": "...", "info": "..."}]`
/// * `{"src": "dst"}`
pub fn import_json(workspace: &Path, file: &Path) -> Result<usize> {
    let raw = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", file.display()))?;
    let mut incoming: Vec<GlossaryTerm> = Vec::new();
    match &v {
        serde_json::Value::Array(items) => {
            for it in items {
                let src = it
                    .get("src")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string();
                if src.is_empty() {
                    continue;
                }
                incoming.push(GlossaryTerm {
                    src,
                    dst: it
                        .get("dst")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    info: it
                        .get("info")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    count: 0,
                    status: TermStatus::Active,
                    source: "import".into(),
                    case_sensitive: it
                        .get("case_sensitive")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                });
            }
        }
        serde_json::Value::Object(map) => {
            for (src, dst) in map {
                let Some(dst) = dst.as_str() else { continue };
                if src.is_empty() {
                    continue;
                }
                incoming.push(GlossaryTerm {
                    src: src.clone(),
                    dst: dst.to_string(),
                    info: String::new(),
                    count: 0,
                    status: TermStatus::Active,
                    source: "import".into(),
                    case_sensitive: false,
                });
            }
        }
        _ => bail!("unsupported glossary JSON: expected an array or an object"),
    }

    let mut g = load(workspace);
    let n = incoming.len();
    for t in incoming {
        g.upsert(t);
    }
    ensure_langs(workspace, &mut g);
    save(workspace, &g)?;
    Ok(n)
}

pub fn export_json(workspace: &Path, file: &Path) -> Result<usize> {
    let g = load(workspace);
    let items: Vec<serde_json::Value> = g
        .terms
        .iter()
        .filter(|t| t.status == TermStatus::Active)
        .map(|t| {
            serde_json::json!({
                "src": t.src,
                "dst": t.dst,
                "info": t.info,
                "case_sensitive": t.case_sensitive,
            })
        })
        .collect();
    let n = items.len();
    std::fs::write(file, serde_json::to_string_pretty(&items)?)
        .with_context(|| format!("write {}", file.display()))?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemType;

    fn unit(text: &str) -> TextUnit {
        TextUnit {
            id: text.to_string(),
            engine: "rmmz".into(),
            domain: "dialogue".into(),
            location: text.to_string(),
            item_type: ItemType::ShortText,
            role: String::new(),
            original_lines: vec![text.to_string()],
            source_line_paths: vec![],
            context: String::new(),
            payload: String::new(),
        }
    }

    fn repeat(text: &str, n: usize) -> Vec<TextUnit> {
        (0..n)
            .map(|i| {
                let mut u = unit(text);
                u.id = format!("{text}#{i}");
                u.location = u.id.clone();
                u
            })
            .collect()
    }

    fn count_of(v: &[(String, usize)], term: &str) -> usize {
        v.iter()
            .find(|(t, _)| t == term)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    // ---- occurrence counting ----

    #[test]
    fn counts_lines_containing_the_term() {
        let units = repeat("アレイは村を出た", 12);
        assert_eq!(count_occurrences(&units, "アレイ"), 12);
    }

    #[test]
    fn counts_a_line_once_even_with_two_hits() {
        let units = repeat("アレイとアレイの影", 5);
        assert_eq!(count_occurrences(&units, "アレイ"), 5, "line-level count");
    }

    #[test]
    fn absent_terms_count_zero() {
        let units = repeat("アレイは村を出た", 3);
        assert_eq!(count_occurrences(&units, "ベルナ"), 0);
    }

    #[test]
    fn counts_work_across_scripts() {
        let units = repeat("Alice went to Silver Harbor today", 6);
        assert_eq!(count_occurrences(&units, "Alice"), 6);
        assert_eq!(count_occurrences(&units, "Silver Harbor"), 6);
    }

    // ---- injection ----

    fn term(src: &str, dst: &str, count: usize) -> GlossaryTerm {
        GlossaryTerm {
            src: src.into(),
            dst: dst.into(),
            info: String::new(),
            count,
            status: TermStatus::Active,
            source: "auto".into(),
            case_sensitive: false,
        }
    }

    #[test]
    fn selects_only_terms_present_in_the_batch() {
        let terms = vec![term("アレイ", "艾蕾", 40), term("ベルナ", "贝尔娜", 10)];
        let got = select_for_batch(&terms, "アレイは剣を取った", 30);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].dst, "艾蕾");
    }

    #[test]
    fn respects_the_inject_limit() {
        let terms: Vec<GlossaryTerm> = (0..10)
            .map(|i| term(&format!("名{i}"), &format!("N{i}"), 100 - i))
            .collect();
        let text: String = (0..10).map(|i| format!("名{i} ")).collect();
        assert_eq!(select_for_batch(&terms, &text, 3).len(), 3);
    }

    #[test]
    fn rejected_and_unnamed_terms_are_never_injected() {
        let mut rejected = term("春", "", 99);
        rejected.status = TermStatus::Rejected;
        let unnamed = term("ベルナ", "", 50);
        let terms = vec![rejected, unnamed, term("アレイ", "艾蕾", 10)];
        let got = select_for_batch(&terms, "春にアレイとベルナが会った", 30);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].src, "アレイ");
    }

    #[test]
    fn case_insensitive_match_unless_flagged() {
        let mut sensitive = term("May", "梅", 3);
        sensitive.case_sensitive = true;
        let terms = vec![sensitive, term("Alice", "爱丽丝", 3)];
        assert_eq!(select_for_batch(&terms, "may and Alice", 10).len(), 1);
        assert_eq!(select_for_batch(&terms, "May and Alice", 10).len(), 2);
    }

    #[test]
    fn namebox_terms_are_counted() {
        let mut u = unit("アレイ");
        u.domain = "namebox".into();
        u.role = "namebox".into();
        assert_eq!(namebox_terms(&[u])[0].0, "アレイ");
    }

    #[test]
    fn normalize_info_whitelist_and_aliases() {
        assert_eq!(normalize_info("女性角色").as_deref(), Some("女性角色"));
        assert_eq!(normalize_info("female").as_deref(), Some("女性角色"));
        assert_eq!(normalize_info("").as_deref(), Some("其他"));
        assert!(normalize_info("女主光环").is_none());
    }

    // ---- storage ----

    #[test]
    fn toml_roundtrip_preserves_terms() {
        let g = Glossary {
            version: GLOSSARY_VERSION,
            source_lang: "ja".into(),
            target_lang: "zh".into(),
            terms: vec![
                term("アレイ", "艾蕾", 47),
                GlossaryTerm {
                    src: "春".into(),
                    dst: String::new(),
                    info: "太常见，非专有名词".into(),
                    count: 120,
                    status: TermStatus::Rejected,
                    source: "auto".into(),
                    case_sensitive: false,
                },
            ],
        };
        let back: Glossary = toml::from_str(&toml::to_string_pretty(&g).unwrap()).unwrap();
        assert_eq!(back.terms.len(), 2);
        assert_eq!(back.source_lang, "ja");
        assert_eq!(back.terms[0].count, 47);
        assert_eq!(back.terms[1].status, TermStatus::Rejected);
        assert_eq!(back.active().len(), 1, "rejected terms stay out of use");
    }

    #[test]
    fn upsert_replaces_the_same_source_term() {
        let mut g = Glossary::default();
        g.upsert(term("アレイ", "埃雷", 1));
        g.upsert(term("アレイ", "艾蕾", 47));
        assert_eq!(g.terms.len(), 1);
        assert_eq!(g.terms[0].dst, "艾蕾");
    }

    #[test]
    fn import_accepts_both_json_shapes() {
        let dir = std::env::temp_dir().join(format!("attx-gl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let list = dir.join("list.json");
        std::fs::write(
            &list,
            r#"[{"src":"アレイ","dst":"艾蕾","info":"女性名字"}]"#,
        )
        .unwrap();
        assert_eq!(import_json(&dir, &list).unwrap(), 1);

        let map = dir.join("map.json");
        std::fs::write(&map, r#"{"ベルナ":"贝尔娜"}"#).unwrap();
        assert_eq!(import_json(&dir, &map).unwrap(), 1);

        let g = load(&dir);
        assert_eq!(g.active().len(), 2);
        assert_eq!(g.find("アレイ").unwrap().info, "女性名字");

        let out = dir.join("out.json");
        assert_eq!(export_json(&dir, &out).unwrap(), 2);
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(back.as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_glossary_is_empty_not_an_error() {
        let dir = std::env::temp_dir().join(format!("attx-gl-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_batches_respect_char_budget() {
        let units = repeat("あいうえお", 20); // 5 chars each
        let batches = source_batches(&units, 12, 10);
        assert!(batches.len() > 1);
        for b in &batches {
            assert!(b.chars().count() <= 12 + 5, "batch too big: {b}");
        }
    }

    #[test]
    fn source_batches_hard_cap() {
        let units = repeat("名前", 100);
        let batches = source_batches(&units, 3, 5);
        assert_eq!(batches.len(), 5);
    }

    #[test]
    fn accept_llm_term_requires_substring() {
        let units = repeat("アレイは剣を取った", 1);
        assert!(accept_llm_term(
            &units,
            &ExtractedTerm {
                src: "アレイ".into(),
                dst: "艾蕾".into(),
                info: "女性角色".into(),
            }
        ));
        assert!(!accept_llm_term(
            &units,
            &ExtractedTerm {
                src: "ベルナ".into(),
                dst: "贝尔娜".into(),
                info: "女性角色".into(),
            }
        ));
        assert!(
            !accept_llm_term(
                &units,
                &ExtractedTerm {
                    src: "アレイ".into(),
                    dst: "艾蕾".into(),
                    info: "女主光环".into(),
                }
            ),
            "unknown info labels are rejected"
        );
    }

    #[test]
    fn normalize_info_accepts_concept_label() {
        // 特殊概念 is the LinguaGacha label for world-specific concepts like
        // magic systems; it must pass the whitelist now that the prompt asks
        // for it explicitly.
        assert_eq!(normalize_info("特殊概念").as_deref(), Some("特殊概念"));
    }

}
