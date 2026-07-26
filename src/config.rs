use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub llm: LlmSection,
    #[serde(default)]
    pub translation: TranslationSection,
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
