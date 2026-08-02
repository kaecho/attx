//! Extensible experience layer.
//!
//! Format adapters decide what to extract using hardcoded heuristics (name
//! whitelists, machine-literal checks). Those tables are hand-written and
//! therefore wrong sometimes — and every fix stays in the source, so the next
//! game and the next user re-discover the same mistake.
//!
//! This module keeps that judgement as *data*: per-format entries that say a
//! field name is an identity handle (skip) or player-visible text (extract),
//! plus free-form notes. Entries are learned from evidence (see `learn.rs`)
//! and stored as readable TOML next to saved profiles.
//!
//! Design constraints that keep this safe:
//! * `apply` is a pure function over already-extracted units — no adapter
//!   changes, no I/O, and `--no-knowledge` restores the old behaviour exactly.
//! * The entry language is deliberately poor: field name, verdict, scope,
//!   domain. No regex, no value predicates, no boolean composition.
//! * `Extract` entries cannot resurrect machine literals (see `apply`).
//!   Learning may override a *name* heuristic, never the evidence of a value.
//! * Entry kinds this build does not understand are preserved verbatim on
//!   write, so an agent can add its own vocabulary without attx dropping it.

use crate::model::TextUnit;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Current on-disk schema version. v1 files (`[[rule]]`, written by 0.5.0) are
/// still read; see `parse_file`.
pub const EXPERIENCE_VERSION: u32 = 2;

/// Per-project override file inside a workspace.
pub const WORKSPACE_EXPERIENCE: &str = "experience.toml";

// Merge layers, lowest first. A later layer overrides an earlier one.
pub const LAYER_BUILTIN: u8 = 0;
pub const LAYER_GLOBAL: u8 = 1;
pub const LAYER_WORKSPACE: u8 = 2;

/// Default experience shipped with the binary. Embedded rather than installed
/// as loose files: these tables are derived from the adapters' own key lists,
/// so shipping them separately would only add "file missing from the release"
/// and "file out of sync with the code" as failure modes.
const EMBEDDED: &[(&str, &str)] = &[("rmmz", include_str!("defaults/rmmz.toml"))];

pub fn embedded_defaults(format: &str) -> Option<&'static str> {
    EMBEDDED.iter().find(|(f, _)| *f == format).map(|(_, s)| *s)
}

pub fn embedded_formats() -> Vec<&'static str> {
    EMBEDDED.iter().map(|(f, _)| *f).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Field addresses game logic (ids, handles, filenames) — never translate.
    Skip,
    /// Field holds player-visible text the adapter's heuristics missed.
    Extract,
}

impl Verdict {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "skip" => Some(Self::Skip),
            "extract" => Some(Self::Extract),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Extract => "extract",
        }
    }
}

/// Where in a structure the entry applies. Plugin params, for instance, behave
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
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "nested" => Some(Self::Nested),
            "top" => Some(Self::Top),
            "any" => Some(Self::Any),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nested => "nested",
            Self::Top => "top",
            Self::Any => "any",
        }
    }

    fn matches(self, unit_is_nested: bool) -> bool {
        match self {
            Scope::Any => true,
            Scope::Nested => unit_is_nested,
            Scope::Top => !unit_is_nested,
        }
    }
}

/// Approval state. The asymmetry between the two is the whole safety story of
/// automatic learning: additive entries take effect on their own, entries that
/// *delete* text wait for a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Approved,
    Pending,
}

impl Status {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "approved" => Some(Self::Approved),
            "pending" => Some(Self::Pending),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Pending => "pending",
        }
    }
}

/// `kind = "field"` — a field-name level extraction judgement.
#[derive(Debug, Clone)]
pub struct FieldEntry {
    /// Lowercase field name. A leading `*` matches by suffix (`*text`).
    pub field: String,
    pub verdict: Verdict,
    pub scope: Scope,
    pub status: Status,
    /// Restrict to one unit domain (`plugins`, `dialogue`, …). Empty = any.
    /// Without this guard an rmmz plugin-param rule would also fire on units
    /// from `Map*.json`, where the same field name means something else.
    pub domain: String,
    pub confidence: f32,
    pub reason: String,
    /// Human-checkable provenance: which workspace, how many hits.
    pub evidence: Vec<String>,
    pub source: String,
    pub updated_at: String,
    /// Everything the parser did not claim, kept so a write preserves it.
    raw: toml::value::Table,
}

impl FieldEntry {
    pub fn new(field: &str, verdict: Verdict, scope: Scope) -> Self {
        Self {
            field: field.to_string(),
            verdict,
            scope,
            status: Status::Approved,
            domain: String::new(),
            confidence: 1.0,
            reason: String::new(),
            evidence: Vec::new(),
            source: String::new(),
            updated_at: String::new(),
            raw: toml::value::Table::new(),
        }
    }

    fn from_table(t: toml::value::Table, default_status: Status) -> Self {
        Self {
            field: str_of(&t, "field"),
            verdict: t
                .get("verdict")
                .and_then(|v| v.as_str())
                .and_then(Verdict::parse)
                .unwrap_or(Verdict::Skip),
            scope: t
                .get("scope")
                .and_then(|v| v.as_str())
                .and_then(Scope::parse)
                .unwrap_or_default(),
            status: t
                .get("status")
                .and_then(|v| v.as_str())
                .and_then(Status::parse)
                .unwrap_or(default_status),
            domain: str_of(&t, "domain"),
            confidence: num_of(&t, "confidence") as f32,
            reason: str_of(&t, "reason"),
            evidence: strings_of(&t, "evidence"),
            source: str_of(&t, "source"),
            updated_at: str_of(&t, "updated_at"),
            raw: t,
        }
    }

    fn to_value(&self) -> toml::Value {
        let mut t = self.raw.clone();
        t.insert("kind".into(), "field".into());
        t.insert("field".into(), self.field.clone().into());
        t.insert("verdict".into(), self.verdict.as_str().into());
        t.insert("scope".into(), self.scope.as_str().into());
        t.insert("status".into(), self.status.as_str().into());
        if self.domain.is_empty() {
            t.remove("domain");
        } else {
            t.insert("domain".into(), self.domain.clone().into());
        }
        t.insert(
            "confidence".into(),
            toml::Value::Float(f64::from(self.confidence)),
        );
        t.insert("reason".into(), self.reason.clone().into());
        t.insert(
            "evidence".into(),
            toml::Value::Array(self.evidence.iter().cloned().map(Into::into).collect()),
        );
        t.insert("source".into(), self.source.clone().into());
        t.insert("updated_at".into(), self.updated_at.clone().into());
        toml::Value::Table(t)
    }

    fn matches_field(&self, field: &str) -> bool {
        match self.field.strip_prefix('*') {
            Some(suffix) => !suffix.is_empty() && field.ends_with(suffix),
            None => self.field == field,
        }
    }

    /// Exact entries beat suffix entries, so a specific `key` overrides a broad
    /// `*key`. Ties are broken by the caller (skip wins).
    fn specificity(&self) -> u8 {
        if self.field.starts_with('*') { 0 } else { 1 }
    }
}

/// `kind = "note"` — free-form experience. Auto-approved: the worst case is a
/// few extra sentences in a prompt, never a lost line of text.
#[derive(Debug, Clone)]
pub struct NoteEntry {
    /// `prompt` notes reach the translation system prompt; every other topic is
    /// for humans and agents reading `attx learn list`.
    pub topic: String,
    pub text: String,
    pub status: Status,
    pub source: String,
    pub updated_at: String,
    raw: toml::value::Table,
}

impl NoteEntry {
    pub fn new(topic: &str, text: &str) -> Self {
        Self {
            topic: topic.to_string(),
            text: text.to_string(),
            status: Status::Approved,
            source: String::new(),
            updated_at: String::new(),
            raw: toml::value::Table::new(),
        }
    }

    fn from_table(t: toml::value::Table) -> Self {
        Self {
            topic: str_of(&t, "topic"),
            text: str_of(&t, "text"),
            status: t
                .get("status")
                .and_then(|v| v.as_str())
                .and_then(Status::parse)
                .unwrap_or_default(),
            source: str_of(&t, "source"),
            updated_at: str_of(&t, "updated_at"),
            raw: t,
        }
    }

    fn to_value(&self) -> toml::Value {
        let mut t = self.raw.clone();
        t.insert("kind".into(), "note".into());
        t.insert("topic".into(), self.topic.clone().into());
        t.insert("text".into(), self.text.clone().into());
        t.insert("status".into(), self.status.as_str().into());
        t.insert("source".into(), self.source.clone().into());
        t.insert("updated_at".into(), self.updated_at.clone().into());
        toml::Value::Table(t)
    }
}

/// One experience entry.
///
/// `Unknown` is not an error path — it is the extension point. An agent may
/// invent `kind = "voice-hint"`; attx does not act on it, but must hand it back
/// unchanged on the next write, or the agent's additions would evaporate.
#[derive(Debug, Clone)]
pub enum Entry {
    Field(FieldEntry),
    Note(NoteEntry),
    Unknown(toml::Value),
}

impl Entry {
    fn from_value(v: toml::Value) -> Entry {
        let Some(t) = v.as_table() else {
            return Entry::Unknown(v);
        };
        match t.get("kind").and_then(|k| k.as_str()) {
            Some("field") => Entry::Field(FieldEntry::from_table(t.clone(), Status::default())),
            Some("note") => Entry::Note(NoteEntry::from_table(t.clone())),
            _ => Entry::Unknown(v),
        }
    }

    fn to_value(&self) -> toml::Value {
        match self {
            Entry::Field(f) => f.to_value(),
            Entry::Note(n) => n.to_value(),
            Entry::Unknown(v) => v.clone(),
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            Entry::Field(_) => "field",
            Entry::Note(_) => "note",
            Entry::Unknown(v) => v.get("kind").and_then(|k| k.as_str()).unwrap_or("unknown"),
        }
    }

    /// Machine-readable view for `attx learn list` / `pending`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Entry::Field(f) => serde_json::json!({
                "kind": "field",
                "field": f.field,
                "verdict": f.verdict.as_str(),
                "scope": f.scope.as_str(),
                "status": f.status.as_str(),
                "domain": f.domain,
                "confidence": f.confidence,
                "reason": f.reason,
                "evidence": f.evidence,
                "source": f.source,
            }),
            Entry::Note(n) => serde_json::json!({
                "kind": "note",
                "topic": n.topic,
                "text": n.text,
                "status": n.status.as_str(),
                "source": n.source,
            }),
            Entry::Unknown(v) => serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
        }
    }
}

fn str_of(t: &toml::value::Table, k: &str) -> String {
    t.get(k)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn num_of(t: &toml::value::Table, k: &str) -> f64 {
    match t.get(k) {
        Some(toml::Value::Float(f)) => *f,
        Some(toml::Value::Integer(i)) => *i as f64,
        _ => 0.0,
    }
}

fn strings_of(t: &toml::value::Table, k: &str) -> Vec<String> {
    t.get(k)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// One on-disk experience file.
#[derive(Debug, Clone)]
pub struct ExperienceFile {
    pub format: String,
    pub version: u32,
    pub entries: Vec<Entry>,
}

impl ExperienceFile {
    pub fn new(format: &str) -> Self {
        Self {
            format: format.to_string(),
            version: EXPERIENCE_VERSION,
            entries: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Replace an equivalent entry, else append.
    ///
    /// Field entries are keyed by (field, domain). Notes are keyed by
    /// (topic, source) so a re-run of the automatic summary replaces its own
    /// previous note instead of piling up — while notes a human or agent wrote
    /// carry a different `source` and are never touched.
    pub fn upsert(&mut self, entry: Entry) {
        match &entry {
            Entry::Field(new) => self.entries.retain(|e| match e {
                Entry::Field(old) => !(old.field == new.field && old.domain == new.domain),
                _ => true,
            }),
            Entry::Note(new) => self.entries.retain(|e| match e {
                Entry::Note(old) => !(old.topic == new.topic && old.source == new.source),
                _ => true,
            }),
            Entry::Unknown(_) => {}
        }
        self.entries.push(entry);
    }
}

#[derive(Serialize)]
struct OutFile<'a> {
    // Declaration order is emission order: scalars before the entry array.
    format: &'a str,
    version: u32,
    #[serde(rename = "entry", skip_serializing_if = "Vec::is_empty")]
    entry: Vec<toml::Value>,
}

pub fn parse_file(raw: &str) -> Result<ExperienceFile> {
    let v: toml::Value = toml::from_str(raw).context("parse experience toml")?;
    let t = v
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("experience file must be a table"))?;
    let mut entries = Vec::new();
    if let Some(arr) = t.get("entry").and_then(|x| x.as_array()) {
        for item in arr {
            entries.push(Entry::from_value(item.clone()));
        }
    }
    // v1 compatibility: 0.5.0 wrote `[[rule]]` with no kind and no status.
    // Those were only ever written after an explicit human approval, so they
    // migrate in as approved.
    if let Some(arr) = t.get("rule").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(tab) = item.as_table() {
                entries.push(Entry::Field(FieldEntry::from_table(
                    tab.clone(),
                    Status::Approved,
                )));
            }
        }
    }
    Ok(ExperienceFile {
        format: str_of(t, "format"),
        version: t
            .get("version")
            .and_then(|x| x.as_integer())
            .unwrap_or(i64::from(EXPERIENCE_VERSION)) as u32,
        entries,
    })
}

pub fn serialize_file(f: &ExperienceFile) -> Result<String> {
    let out = OutFile {
        format: &f.format,
        version: EXPERIENCE_VERSION,
        entry: f.entries.iter().map(Entry::to_value).collect(),
    };
    toml::to_string_pretty(&out).context("serialize experience")
}

// ---- storage ----

/// Directories that may hold experience files, most specific first. Mirrors
/// `profile::config_dirs` so profiles and experience live side by side.
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

/// Writable experience dir, created on demand.
pub fn knowledge_dir() -> Result<PathBuf> {
    let dir = knowledge_dirs()
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no config dir available; set $ATTX_HOME"))?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create knowledge dir {}", dir.display()))?;
    Ok(dir)
}

fn file_path_in(dir: &Path, format: &str) -> PathBuf {
    dir.join(format!("{format}.toml"))
}

/// Load the global experience file for a format. Missing file → empty set (not
/// an error). A malformed file is reported to stderr and treated as empty so a
/// bad edit degrades to "no learning" rather than breaking extraction.
pub fn load_file(format: &str) -> ExperienceFile {
    for dir in knowledge_dirs() {
        let p = file_path_in(&dir, format);
        if !p.is_file() {
            continue;
        }
        match std::fs::read_to_string(&p)
            .map_err(anyhow::Error::from)
            .and_then(|raw| parse_file(&raw))
        {
            Ok(mut f) => {
                if f.format.is_empty() {
                    f.format = format.to_string();
                }
                return f;
            }
            Err(e) => eprintln!("knowledge: ignoring {}: {e:#}", p.display()),
        }
    }
    ExperienceFile::new(format)
}

pub fn save_file(f: &ExperienceFile) -> Result<PathBuf> {
    let dir = knowledge_dir()?;
    let path = file_path_in(&dir, &f.format);
    let body = serialize_file(f)?;
    let header = format!(
        "# attx experience for format `{}`.\n\
         # Edit or delete freely: `attx learn list` shows what is active,\n\
         # `attx extract --no-knowledge` ignores this file entirely.\n\
         # Entry kinds attx does not understand are preserved as-is.\n",
        f.format
    );
    std::fs::write(&path, format!("{header}{body}"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// All formats that currently have a global experience file.
pub fn all_files() -> Vec<ExperienceFile> {
    let mut seen: BTreeMap<String, ExperienceFile> = BTreeMap::new();
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
            let f = load_file(stem);
            if !f.is_empty() {
                seen.insert(stem.to_string(), f);
            }
        }
    }
    seen.into_values().collect()
}

// ---- merged view ----

/// Experience for one format, merged across layers. Later layers override
/// earlier ones; within a layer, exact beats suffix and `skip` beats `extract`.
pub struct Experience {
    pub format: String,
    pub entries: Vec<(u8, Entry)>,
}

impl Experience {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn field_entries(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, e)| matches!(e, Entry::Field(_)))
            .count()
    }

    /// Best entry for `field`, or None.
    ///
    /// Within the same layer and specificity `Skip` beats `Extract` — a missed
    /// translation is visible in `attx status`, a translated identity handle
    /// silently breaks saves.
    fn lookup(&self, field: &str, nested: bool, domain: &str) -> Option<&FieldEntry> {
        self.entries
            .iter()
            .filter_map(|(layer, e)| match e {
                Entry::Field(f) => Some((*layer, f)),
                _ => None,
            })
            .filter(|(_, f)| f.status == Status::Approved)
            .filter(|(_, f)| f.domain.is_empty() || f.domain == domain)
            .filter(|(_, f)| f.scope.matches(nested) && f.matches_field(field))
            .max_by_key(|(layer, f)| {
                (
                    *layer,
                    f.specificity(),
                    u8::from(f.verdict == Verdict::Skip),
                )
            })
            .map(|(_, f)| f)
    }

    /// Approved `topic = "prompt"` notes, for the translation system prompt.
    pub fn prompt_notes(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Note(n)
                    if n.status == Status::Approved
                        && n.topic == "prompt"
                        && !n.text.trim().is_empty() =>
                {
                    Some(n.text.clone())
                }
                _ => None,
            })
            .collect()
    }
}

/// Load and merge every layer for a format.
pub fn load_experience(format: &str, workspace: Option<&Path>) -> Experience {
    let mut entries: Vec<(u8, Entry)> = Vec::new();
    if let Some(raw) = embedded_defaults(format) {
        match parse_file(raw) {
            Ok(f) => entries.extend(f.entries.into_iter().map(|e| (LAYER_BUILTIN, e))),
            // Guarded by a test, so this can only fire on a corrupted binary.
            Err(e) => eprintln!("knowledge: embedded defaults for {format} are broken: {e:#}"),
        }
    }
    entries.extend(
        load_file(format)
            .entries
            .into_iter()
            .map(|e| (LAYER_GLOBAL, e)),
    );
    if let Some(ws) = workspace {
        let p = ws.join(WORKSPACE_EXPERIENCE);
        if p.is_file() {
            match std::fs::read_to_string(&p)
                .map_err(anyhow::Error::from)
                .and_then(|raw| parse_file(&raw))
            {
                Ok(f) => entries.extend(f.entries.into_iter().map(|e| (LAYER_WORKSPACE, e))),
                Err(e) => eprintln!("knowledge: ignoring {}: {e:#}", p.display()),
            }
        }
    }
    Experience {
        format: format.to_string(),
        entries,
    }
}

/// What `apply` did, for reporting and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub skipped: usize,
    /// Extract entries that fired but were vetoed by the machine-literal check.
    pub extract_vetoed: usize,
}

/// Filter extracted units through the merged experience.
///
/// Only removal happens here: an `Extract` entry cannot invent a unit the
/// adapter never produced. Its job is to stop *other* layers from dropping the
/// field — and it is refused outright when the value is a machine literal, so a
/// bad entry can never send switch ids or filenames to the model.
pub fn apply(units: Vec<TextUnit>, exp: &Experience) -> (Vec<TextUnit>, ApplyReport) {
    if exp.is_empty() {
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
        match exp.lookup(&field, nested, &u.domain).map(|f| f.verdict) {
            Some(Verdict::Skip) => {
                report.skipped += 1;
            }
            Some(Verdict::Extract) => {
                // Gate: a learned entry may override the *name* heuristic, never
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

/// Field name an entry can match against, lowercased.
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
/// `Extract` entry is judged by the same standard the extractor uses.
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

    fn exp(entries: Vec<(u8, Entry)>) -> Experience {
        Experience {
            format: "rmmz".into(),
            entries,
        }
    }

    fn field(f: &str, v: Verdict, s: Scope) -> Entry {
        Entry::Field(FieldEntry::new(f, v, s))
    }

    fn at(layer: u8, e: Entry) -> (u8, Entry) {
        (layer, e)
    }

    // ---- extension point ----

    #[test]
    fn unknown_kind_survives_roundtrip() {
        // The whole point of the extension point: a kind this build has never
        // heard of must come back byte-for-byte, or an agent's additions vanish
        // the next time attx writes the file.
        let src = r#"
format = "rmmz"
version = 2

[[entry]]
kind = "voice-hint"
character = "アレイ"
note = "関西弁"
weight = 3

[[entry]]
kind = "field"
field = "key"
verdict = "skip"
"#;
        let f = parse_file(src).unwrap();
        assert_eq!(f.entries.len(), 2);
        assert_eq!(f.entries[0].kind(), "voice-hint");
        let back = parse_file(&serialize_file(&f).unwrap()).unwrap();
        assert_eq!(back.entries.len(), 2);
        let Entry::Unknown(v) = &back.entries[0] else {
            panic!("unknown kind must stay unknown, not be coerced");
        };
        assert_eq!(v.get("character").and_then(|x| x.as_str()), Some("アレイ"));
        assert_eq!(v.get("weight").and_then(|x| x.as_integer()), Some(3));
    }

    #[test]
    fn unclaimed_fields_on_known_kinds_survive_roundtrip() {
        // Same hazard one level down: an agent annotates a `field` entry with
        // its own key. Parsing must not drop it.
        let src = r#"
format = "rmmz"
version = 2

[[entry]]
kind = "field"
field = "key"
verdict = "skip"
agent_note = "confirmed by playtest"
"#;
        let f = parse_file(src).unwrap();
        let back = parse_file(&serialize_file(&f).unwrap()).unwrap();
        let Entry::Field(fe) = &back.entries[0] else {
            panic!("expected field entry");
        };
        assert_eq!(
            fe.raw.get("agent_note").and_then(|x| x.as_str()),
            Some("confirmed by playtest")
        );
    }

    #[test]
    fn v1_rule_file_reads_as_approved_entries() {
        // 0.5.0 files only ever contained human-approved rules.
        let src = r#"
format = "rmmz"
version = 1

[[rule]]
field = "key"
verdict = "skip"
scope = "nested"
confidence = 0.9
reason = "identity handle"
"#;
        let f = parse_file(src).unwrap();
        assert_eq!(f.entries.len(), 1);
        let Entry::Field(fe) = &f.entries[0] else {
            panic!("v1 rule must migrate to a field entry");
        };
        assert_eq!(fe.field, "key");
        assert_eq!(fe.verdict, Verdict::Skip);
        assert_eq!(fe.scope, Scope::Nested);
        assert_eq!(fe.status, Status::Approved);
    }

    #[test]
    fn integer_confidence_is_accepted() {
        // TOML `confidence = 1` is an integer, not a float.
        let f = parse_file("[[entry]]\nkind='field'\nfield='k'\nverdict='skip'\nconfidence=1\n")
            .unwrap();
        let Entry::Field(fe) = &f.entries[0] else {
            panic!()
        };
        assert!((fe.confidence - 1.0).abs() < f32::EPSILON);
    }

    // ---- embedded defaults ----

    #[test]
    fn every_embedded_default_parses() {
        // A typo in a hand-written defaults table must fail the build's tests,
        // not surface as a broken extraction on a user's machine.
        for f in embedded_formats() {
            let raw = embedded_defaults(f).expect("registered format has content");
            let parsed = parse_file(raw).unwrap_or_else(|e| panic!("defaults/{f}.toml: {e:#}"));
            assert!(!parsed.is_empty(), "defaults/{f}.toml must not be empty");
        }
    }

    #[test]
    fn embedded_rmmz_defaults_are_scoped_to_plugin_params() {
        // Unscoped skip entries would also fire on Map*.json units, where the
        // same field name means something else.
        let parsed = parse_file(embedded_defaults("rmmz").unwrap()).unwrap();
        for e in &parsed.entries {
            if let Entry::Field(f) = e {
                assert_eq!(
                    f.domain, "plugins",
                    "entry {:?} must carry a domain",
                    f.field
                );
            }
        }
    }

    // ---- apply ----

    #[test]
    fn empty_experience_is_identity() {
        let units = vec![unit("a#0/key", "x"), unit("b#0/title", "y")];
        let (out, rep) = apply(units.clone(), &exp(vec![]));
        assert_eq!(out.len(), units.len());
        assert_eq!(rep, ApplyReport::default());
    }

    #[test]
    fn skip_entry_removes_matching_units() {
        let units = vec![
            unit("js/plugins.js/0/data#0/key", "実績_a"),
            unit("js/plugins.js/0/data#0/title", "称号"),
        ];
        let e = exp(vec![at(1, field("key", Verdict::Skip, Scope::Any))]);
        let (out, rep) = apply(units, &e);
        assert_eq!(rep.skipped, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(field_of(&out[0]).as_deref(), Some("title"));
    }

    #[test]
    fn pending_skip_is_inert() {
        // The core safety property of automatic learning: an unapproved entry
        // that would delete text must do nothing at all.
        let mut fe = FieldEntry::new("key", Verdict::Skip, Scope::Any);
        fe.status = Status::Pending;
        let e = exp(vec![at(1, Entry::Field(fe))]);
        let (out, rep) = apply(vec![unit("a#0/key", "実績_a")], &e);
        assert_eq!(rep.skipped, 0);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn domain_guard_limits_an_entry_to_its_own_units() {
        let mut fe = FieldEntry::new("name", Verdict::Skip, Scope::Any);
        fe.domain = "plugins".into();
        let e = exp(vec![at(1, Entry::Field(fe))]);

        let plugin_unit = unit("js/plugins.js/0/d#0/name", "アレイ");
        let mut map_unit = unit("Map003.json/2/0/5", "アレイ");
        map_unit.domain = "dialogue".into();
        map_unit.location = "Map003.json/2/0/name".into();

        let (out, rep) = apply(vec![plugin_unit, map_unit], &e);
        assert_eq!(rep.skipped, 1, "only the plugins-domain unit is skipped");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].domain, "dialogue");
    }

    #[test]
    fn later_layer_overrides_earlier() {
        // Builtin says skip; the user's global file says extract. User wins.
        let e = exp(vec![
            at(LAYER_BUILTIN, field("key", Verdict::Skip, Scope::Any)),
            at(LAYER_GLOBAL, field("key", Verdict::Extract, Scope::Any)),
        ]);
        let (out, rep) = apply(vec![unit("a#0/key", "本当のテキスト")], &e);
        assert_eq!(rep.skipped, 0);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn workspace_layer_beats_global() {
        let e = exp(vec![
            at(LAYER_GLOBAL, field("title", Verdict::Extract, Scope::Any)),
            at(LAYER_WORKSPACE, field("title", Verdict::Skip, Scope::Any)),
        ]);
        let (_out, rep) = apply(vec![unit("a#0/title", "称号")], &e);
        assert_eq!(rep.skipped, 1);
    }

    #[test]
    fn scope_nested_does_not_touch_top_level() {
        let nested = unit("js/plugins.js/0/data#0/key", "実績_a");
        let top = unit("js/plugins.js/0/key", "実績_b");
        let e = exp(vec![at(1, field("key", Verdict::Skip, Scope::Nested))]);
        let (out, rep) = apply(vec![nested, top], &e);
        assert_eq!(rep.skipped, 1, "only the nested unit is skipped");
        assert_eq!(out.len(), 1);
        assert!(!is_nested(&out[0]));
    }

    #[test]
    fn suffix_entry_matches_by_ending() {
        let units = vec![
            unit("a#0/commandText", "コマンド"),
            unit("a#0/other", "その他"),
        ];
        let e = exp(vec![at(1, field("*text", Verdict::Skip, Scope::Any))]);
        let (out, rep) = apply(units, &e);
        assert_eq!(rep.skipped, 1);
        assert_eq!(field_of(&out[0]).as_deref(), Some("other"));
    }

    #[test]
    fn exact_entry_beats_suffix_entry_in_the_same_layer() {
        let e = exp(vec![
            at(1, field("*name", Verdict::Skip, Scope::Any)),
            at(1, field("displayname", Verdict::Extract, Scope::Any)),
        ]);
        let (out, rep) = apply(vec![unit("a#0/displayName", "名前")], &e);
        assert_eq!(rep.skipped, 0, "exact extract entry wins");
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn skip_wins_ties_against_extract() {
        // Same layer, same specificity, opposite verdicts: the safe one wins.
        let e = exp(vec![
            at(1, field("key", Verdict::Extract, Scope::Any)),
            at(1, field("key", Verdict::Skip, Scope::Any)),
        ]);
        let (out, rep) = apply(vec![unit("a#0/key", "実績_a")], &e);
        assert_eq!(rep.skipped, 1);
        assert!(out.is_empty());
    }

    #[test]
    fn extract_entry_is_vetoed_for_machine_literals() {
        let e = exp(vec![at(1, field("switchid", Verdict::Extract, Scope::Any))]);
        let (_out, rep) = apply(vec![unit("a#0/switchId", "12")], &e);
        assert_eq!(rep.extract_vetoed, 1, "numeric value must trip the gate");

        let (_out, rep) = apply(vec![unit("a#0/switchId", "本当のテキスト")], &e);
        assert_eq!(rep.extract_vetoed, 0, "real text passes");
    }

    // ---- notes ----

    #[test]
    fn only_approved_prompt_notes_reach_the_prompt() {
        let mut pending = NoteEntry::new("prompt", "pending advice");
        pending.status = Status::Pending;
        let e = exp(vec![
            at(1, Entry::Note(NoteEntry::new("prompt", "keep honorifics"))),
            at(1, Entry::Note(NoteEntry::new("extraction", "not a prompt"))),
            at(1, Entry::Note(pending)),
            at(1, Entry::Note(NoteEntry::new("prompt", "   "))),
        ]);
        assert_eq!(e.prompt_notes(), vec!["keep honorifics".to_string()]);
    }

    // ---- field name parsing ----

    #[test]
    fn field_of_uses_last_non_numeric_segment() {
        let u = unit("js/plugins.js/0/baseAchievementData#0/description", "x");
        assert_eq!(field_of(&u).as_deref(), Some("description"));
    }

    #[test]
    fn field_of_skips_array_indices() {
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
    fn machine_literal_covers_numbers_paths_and_scripts() {
        for s in ["12", "-3.5", "true", "", "img/pictures/a.png", "x.ogg"] {
            assert!(is_machine_literal(s), "{s:?} should be machine data");
        }
        for s in ["実績_a", "所持金", "Hello there"] {
            assert!(!is_machine_literal(s), "{s:?} should be text");
        }
    }

    // ---- storage ----

    #[test]
    fn entry_toml_roundtrip_preserves_semantics() {
        let mut f = ExperienceFile::new("rmmz");
        let mut fe = FieldEntry::new("key", Verdict::Skip, Scope::Nested);
        fe.status = Status::Pending;
        fe.domain = "plugins".into();
        fe.reason = "identity handle".into();
        fe.evidence = vec!["ws: identical=6/6".into()];
        f.entries.push(Entry::Field(fe));
        f.entries
            .push(Entry::Note(NoteEntry::new("prompt", "keep honorifics")));

        let back = parse_file(&serialize_file(&f).unwrap()).unwrap();
        assert_eq!(back.version, EXPERIENCE_VERSION);
        assert_eq!(back.entries.len(), 2);
        let Entry::Field(fe) = &back.entries[0] else {
            panic!()
        };
        assert_eq!(fe.field, "key");
        assert_eq!(fe.scope, Scope::Nested);
        assert_eq!(fe.status, Status::Pending);
        assert_eq!(fe.domain, "plugins");
        assert_eq!(fe.evidence, vec!["ws: identical=6/6".to_string()]);
        let Entry::Note(n) = &back.entries[1] else {
            panic!()
        };
        assert_eq!(n.topic, "prompt");
    }

    #[test]
    fn upsert_replaces_same_field_and_domain() {
        let mut f = ExperienceFile::new("rmmz");
        f.upsert(field("key", Verdict::Skip, Scope::Any));
        f.upsert(field("key", Verdict::Extract, Scope::Any));
        assert_eq!(f.entries.len(), 1);
        let Entry::Field(fe) = &f.entries[0] else {
            panic!()
        };
        assert_eq!(fe.verdict, Verdict::Extract);
    }

    #[test]
    fn file_missing_or_broken_degrades_to_empty() {
        let dir = std::env::temp_dir().join(format!("attx-kn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("knowledge")).unwrap();
        std::fs::write(dir.join("knowledge/brokenfmt.toml"), "not = [valid").unwrap();
        // SAFETY: single-threaded test process section; no other thread reads env here.
        unsafe { std::env::set_var("ATTX_HOME", &dir) };
        assert!(
            load_file("brokenfmt").is_empty(),
            "malformed file degrades to no learning"
        );
        assert!(
            load_file("nosuchfmt").is_empty(),
            "missing file is not an error"
        );
        unsafe { std::env::remove_var("ATTX_HOME") };
    }
}
