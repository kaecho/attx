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
    /// Omit to keep call-site defaults (translate 0.3, ask_json 0.0).
    #[serde(default)]
    pub temperature: Option<f64>,
    /// OpenAI o-series / Grok-style effort. Omit = not sent.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Omit = not sent. Newer OpenAI models want `max_completion_tokens` in `extra`.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// When true, request `stream: true` and parse SSE `delta.content` chunks.
    #[serde(default)]
    pub stream: bool,
    /// Merged last into the chat/completions JSON. Adds or overrides keys.
    /// `messages` is ignored so the prompt cannot be replaced.
    #[serde(default)]
    pub extra: toml::Table,
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
    r#"# attx 配置文件（setting.toml）
#
# 用法：
#   1. 复制本文件为 setting.toml（`cp setting.example.toml setting.toml`）
#   2. 填入真实的 base_url / api_key / model
#   3. `attx doctor --ping` 验证配置与 LLM 连通性
#
# 配置文件查找顺序（resolve_config_path）：
#   1. `--config <path>` 显式指定
#   2. `$ATTX_HOME/setting.toml`（仅当该文件存在）
#   3. 当前目录 `./setting.toml`（不存在也返回，非 LLM 命令可无配置运行）
# 删除 [llm] 之外的整个小节是安全的：缺失小节自动回退默认值（见下方各键）。

# ---------------------------------------------------------------------------
# [llm]  LLM 服务配置
# ---------------------------------------------------------------------------
[llm]
# 默认使用的客户端名，对应下方 [[llm.clients]] 里的 name。
# 可用 `--client <name>` 在命令行临时切换。
default_client = "main"

# 可配置多个客户端（复制整个 [[llm.clients]] 块即可），按 name 区分。
[[llm.clients]]
name = "main"
# 供应商类型。目前仅作记录，未参与请求构造：
# 所有客户端统一请求 {base_url}/chat/completions（OpenAI 兼容协议）。
provider_type = "openai"
# API 地址，末尾不要带 /chat/completions，程序会自行拼接。
base_url = "https://api.example.com/v1"
# API 密钥。仅保存在本文件，不写入工作区数据库。
api_key = "YOUR_API_KEY"
# 模型名，按供应商实际可用模型填写（如 gpt-4o、deepseek-chat、qwen-max 等）。
model = "example-model"
# 单次 HTTP 请求超时（秒）。实际生效值 = max(本值, 30)。
# 长文本大批次时模型响应慢，默认 600 秒足够；小模型可调小以快速失败。
timeout = 600
# 以下均为可选项，默认省略时不发送对应字段：
# temperature = 0.3            # 省略时：翻译用 0.3，术语/学习等 JSON 请求用 0.0
# reasoning_effort = "medium"  # OpenAI o 系列 / Grok 风格的思考档位；省略则不发送
# max_tokens = 8192            # 输出上限；省略则不发送（新 OpenAI 模型请在 extra 里用 max_completion_tokens）
# stream = true                # 流式输出；省略为 false。开启后解析 SSE 的 delta.content 增量拼接
# extra = { top_p = 0.9 }      # 额外字段，最后合并进 chat/completions 请求体，可新增或覆盖任意键
                              # （唯一例外：messages 会被忽略，防止覆盖提示词本身）

# ---------------------------------------------------------------------------
# [translation]  翻译管线参数
# ---------------------------------------------------------------------------
[translation]
# 并发工作线程数。每个线程独立发 HTTP 请求，实际吞吐还受 rpm 限制。
# 大语言模型服务通常按并发限流，8 是经验值；遇到 429 可调小。
worker_count = 8
# 全局速率限制：每分钟最多请求次数（跨线程共享计数）。0 = 不限速。
rpm = 60          # 按供应商免费额度/付费档位调整；慢模型可以调低避免积压
# 批次失败后的重试次数（重试之间 sleep retry_delay 秒）。
# 重试耗尽后仍失败：批次减半拆分 → 单条重试 → 仍失败则原文透传（passthrough），
# 不会中断整个翻译任务。
retry_count = 3
# 每次重试前的等待秒数。
retry_delay = 2
# 单个 HTTP 批次的最大源字符数。达到任一上限即切分新批次。
# 注意：这是"每批多少原文"的预算，不是模型上下文窗口。
batch_chars = 2500
# 单个批次的最大条数（与 batch_chars 同时生效，先到先切）。
# 批量请求中每条带独立编号，模型按编号返回，超出的编号会被丢弃并重试。
max_context_items = 6

# ---------------------------------------------------------------------------
# [glossary]  术语表（专有名词统一译名）
# ---------------------------------------------------------------------------
# 作用：保证整部作品里同一人名/地名译法一致（长篇小说、RPG 尤甚）。
# 默认关闭：构建术语表会额外消耗 LLM 调用，未经要求不应产生费用。
# `attx run` 仅在 enabled = true 或显式传 --glossary（且未传 --no-glossary）时构建；
# 显式执行 `attx glossary build` 则无视本开关，执行即视为同意。
[glossary]
enabled = false
# 提取方法：
#   "llm"   （默认）原文分批直接交给模型，让它提取专有名词并给出译名（{src,dst,info}）。
#           能识别正则看不到的专有名词，花费与文本量成正比。
#   "stats" 先用正则挖掘候选（日文：片假名串/汉字串；英文：大写词），
#           达到 min_occurrences 门槛后再让模型命名（keep 否决），花费与术语数量成正比。
# 解析时 "stat"/"regex" 也视为 "stats"。
method = "llm"
# 仅 stats 方法：候选出现次数达到该值才值得占用术语表名额。
min_occurrences = 10
# 保留术语数量上限（按出现次数/票数从高到低截断）。
# 超出的候选会被丢弃并在日志中报告；调大本值或调高 min_occurrences 可扩大覆盖。
max_terms = 200
# 单个翻译批次最多注入的术语条数。
# 注入前会先过滤：只注入本批原文中实际出现的术语（子串匹配），
# 避免几百条术语挤占正文的上下文空间。
inject_limit = 30

# ---------------------------------------------------------------------------
# [learn]  经验学习（跨运行沉淀翻译风格/规则）
# ---------------------------------------------------------------------------
[learn]
# 每次成功 writeback 后自动总结为经验条目（experience.toml）。
# 默认开启且不花钱：证据（原文+译文）已存在工作区数据库里，总结不调 LLM。
auto_summarize = true
# 额外让模型审查待入库的经验条目（增加一次 LLM 调用，花钱）。
# 默认关闭；需要更严格的积累质量时再开。
llm_review = false
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
        // setting.example.toml is generated from this function; keep them in sync.
        let s: Settings = toml::from_str(example_toml()).expect("example_toml must be valid");
        assert!(!s.glossary.enabled);
        assert!(s.learn.auto_summarize);
    }

    #[test]
    fn client_optional_fields_default_absent() {
        let s: Settings = toml::from_str(
            r#"
[llm]
default_client = "main"
[[llm.clients]]
name = "main"
base_url = "http://x"
api_key = "k"
model = "m"
"#,
        )
        .unwrap();
        let c = &s.llm.clients[0];
        assert!(c.temperature.is_none());
        assert!(c.reasoning_effort.is_none());
        assert!(c.max_tokens.is_none());
        assert!(!c.stream);
        assert!(c.extra.is_empty());
    }

    #[test]
    fn client_named_fields_and_extra_parse() {
        let s: Settings = toml::from_str(
            r#"
[llm]
default_client = "main"
[[llm.clients]]
name = "main"
base_url = "http://x"
api_key = "k"
model = "m"
temperature = 0.2
reasoning_effort = "high"
max_tokens = 4096
extra = { top_p = 0.8, max_completion_tokens = 2048 }
"#,
        )
        .unwrap();
        let c = &s.llm.clients[0];
        assert_eq!(c.temperature, Some(0.2));
        assert_eq!(c.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(c.max_tokens, Some(4096));
        assert_eq!(c.extra["top_p"].as_float(), Some(0.8));
        assert_eq!(c.extra["max_completion_tokens"].as_integer(), Some(2048));
    }
}
