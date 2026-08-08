use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub llm: LlmSection,
    #[serde(default)]
    pub translation: TranslationSection,
    #[serde(default)]
    pub glossary: GlossarySection,
    #[serde(default)]
    pub learn: LearnSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSection {
    pub default_client: String,
    pub clients: Vec<LlmClient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmClient {
    pub name: String,
    #[serde(default = "default_provider")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_provider() -> String {
    "openai".into()
}
fn default_timeout() -> u64 {
    600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationSection {
    #[serde(default = "default_workers")]
    pub worker_count: usize,
    #[serde(default = "default_rpm")]
    pub rpm: u32,
    #[serde(default = "default_retry")]
    pub retry_count: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay: u64,
    #[serde(default = "default_batch_chars")]
    pub batch_chars: usize,
    #[serde(default = "default_max_ctx")]
    pub max_context_items: usize,
}

impl Default for TranslationSection {
    fn default() -> Self {
        Self {
            worker_count: 8,
            rpm: 60,
            retry_count: 3,
            retry_delay: 2,
            batch_chars: 2500,
            max_context_items: 6,
        }
    }
}

fn default_workers() -> usize {
    8
}
fn default_rpm() -> u32 {
    60
}
fn default_retry() -> u32 {
    3
}
fn default_retry_delay() -> u64 {
    2
}
fn default_batch_chars() -> usize {
    2500
}
fn default_max_ctx() -> usize {
    6
}

/// Glossary generation. Off by default: building one spends extra LLM calls,
/// and a user who did not ask for that should never be surprised by it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossarySection {
    /// Whether `attx run` builds a glossary between extract and translate.
    /// Explicit `attx glossary build` ignores this — asking is consent.
    #[serde(default)]
    pub enabled: bool,
    /// How candidates are found. `llm` (default) sends source batches to the
    /// model; `stats` mines with regex then only asks the model to name hits.
    #[serde(default)]
    pub method: GlossaryMethod,
    /// Stats method only: a candidate must appear at least this often.
    #[serde(default = "default_min_occurrences")]
    pub min_occurrences: usize,
    /// Cap on terms kept after extraction / naming (highest signal first).
    #[serde(default = "default_max_terms")]
    pub max_terms: usize,
    /// Upper bound on terms injected into any one translation batch.
    #[serde(default = "default_inject_limit")]
    pub inject_limit: usize,
}

/// How `glossary build` discovers proper nouns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GlossaryMethod {
    /// Model reads source batches and emits `{src,dst,info}` (LinguaGacha-style).
    #[default]
    Llm,
    /// Regex mine → frequency gate → model names survivors (cheaper on huge works).
    Stats,
}

impl GlossaryMethod {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "llm" => Some(Self::Llm),
            "stats" | "stat" | "regex" => Some(Self::Stats),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Stats => "stats",
        }
    }
}

impl Default for GlossarySection {
    fn default() -> Self {
        Self {
            enabled: false,
            method: GlossaryMethod::Llm,
            min_occurrences: default_min_occurrences(),
            max_terms: default_max_terms(),
            inject_limit: default_inject_limit(),
        }
    }
}

fn default_min_occurrences() -> usize {
    10
}
fn default_max_terms() -> usize {
    200
}
fn default_inject_limit() -> usize {
    30
}

/// Automatic experience capture after writeback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnSection {
    /// Summarise every successful writeback into experience entries. On by
    /// default because it costs nothing: the evidence is already in the DB.
    #[serde(default = "default_true")]
    pub auto_summarize: bool,
    /// Additionally ask the model to sanity-check proposed entries. Off by
    /// default — this one costs money.
    #[serde(default)]
    pub llm_review: bool,
}

impl Default for LearnSection {
    fn default() -> Self {
        Self {
            auto_summarize: true,
            llm_review: false,
        }
    }
}

fn default_true() -> bool {
    true
}

impl Settings {
    pub fn client(&self, name: Option<&str>) -> Result<&LlmClient> {
        let key = name.unwrap_or(self.llm.default_client.as_str());
        self.llm
            .clients
            .iter()
            .find(|c| c.name == key)
            .with_context(|| format!("llm client not found: {key}"))
    }
}

pub fn load(explicit: Option<&Path>) -> Result<Settings> {
    let path = resolve_config_path(explicit)?;
    if !path.exists() {
        // Allow commands that don't need LLM (detect/extract/status) with empty settings.
        return Ok(Settings {
            llm: LlmSection {
                default_client: "main".into(),
                clients: vec![],
            },
            translation: TranslationSection::default(),
            glossary: GlossarySection::default(),
            learn: LearnSection::default(),
        });
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read config {}", path.display()))?;
    let s: Settings = toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(s)
}

pub fn resolve_config_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(home) = std::env::var("ATTX_HOME") {
        let p = PathBuf::from(home).join("setting.toml");
        if p.exists() {
            return Ok(p);
        }
    }
    let cwd = PathBuf::from("setting.toml");
    if cwd.exists() {
        return Ok(cwd);
    }
    // fallthrough — missing file is ok for non-LLM cmds
    Ok(cwd)
}

pub fn example_toml() -> &'static str {
    r#"# attx setting.toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "YOUR_API_KEY"
model = "example-model"
timeout = 600

[translation]
worker_count = 8
rpm = 60          # global request rate limit per minute (0 = unlimited)
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6

[glossary]
# Off by default: building a glossary spends extra LLM calls.
# It keeps character/place names consistent across an entire work, which
# matters most for novels and long games.
enabled = false
method = "llm"         # llm = model extracts terms from text; stats = regex mine + name
min_occurrences = 10   # stats only: a term must appear this often to be worth a slot
max_terms = 200        # cap on terms kept (cost / noise control)
inject_limit = 30      # cap on terms injected into one translation batch

[learn]
auto_summarize = true  # capture experience after writeback (no API cost)
llm_review = false     # also ask the model to check proposals (costs money)
"#
}

pub fn ensure_example_written(dir: &Path) -> Result<PathBuf> {
    let p = dir.join("setting.example.toml");
    if !p.exists() {
        std::fs::write(&p, example_toml())?;
    }
    Ok(p)
}

pub fn require_llm(settings: &Settings) -> Result<&LlmClient> {
    if settings.llm.clients.is_empty() {
        bail!(
            "no LLM clients configured. Create setting.toml (see setting.example.toml) with [llm] clients."
        );
    }
    settings.client(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sections_fall_back_to_defaults() {
        // Every existing 0.5.0 setting.toml lacks these sections; loading one
        // must not fail, and must not silently switch the paid feature on.
        let s: Settings = toml::from_str(
            r#"
[llm]
default_client = "main"
clients = []
"#,
        )
        .unwrap();
        assert!(!s.glossary.enabled, "the paid feature stays off by default");
        assert_eq!(s.glossary.method, GlossaryMethod::Llm);
        assert_eq!(s.glossary.min_occurrences, 10);
        assert_eq!(s.glossary.max_terms, 200);
        assert_eq!(s.glossary.inject_limit, 30);
        assert!(s.learn.auto_summarize, "free capture is on by default");
        assert!(!s.learn.llm_review, "the paid check stays off by default");
    }

    #[test]
    fn partial_sections_keep_other_defaults() {
        let s: Settings = toml::from_str(
            r#"
[llm]
default_client = "main"
clients = []

[glossary]
enabled = true
"#,
        )
        .unwrap();
        assert!(s.glossary.enabled);
        assert_eq!(s.glossary.method, GlossaryMethod::Llm);
        assert_eq!(s.glossary.min_occurrences, 10);
    }

    #[test]
    fn shipped_example_config_parses() {
        let s: Settings = toml::from_str(example_toml()).expect("example_toml must be valid");
        assert!(!s.glossary.enabled);
        assert!(s.learn.auto_summarize);
    }
}
