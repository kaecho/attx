# Configuration

`setting.toml` configures the model endpoint and translation policy. The canonical schema is [src/config.rs](https://github.com/kaecho/attx/blob/main/src/config.rs); the shipped [setting.example.toml](https://github.com/kaecho/attx/blob/main/setting.example.toml) is the editable starting point. This page lists every current settings key.

## File lookup and path resolution

1. `--config "path/to/setting.toml"` wins when supplied, even if it does not exist.
2. Otherwise use `$ATTX_HOME/setting.toml` only if that file exists.
3. Otherwise use `./setting.toml` in the process's current working directory.

A missing file produces empty model settings plus translation/glossary/learning defaults. Local commands such as `formats`, `detect`, `init`, `extract`, `status`, and `review` can run without a model client. A malformed existing file is an error, including for commands that do not use HTTP. A missing explicit config does not fall back to another file.

For a service that explicitly requires no authentication, keep `api_key = ""` instead of inventing a dummy key. Configured credential strings are refused in model content and redacted from matching HTTP diagnostics. These checks are not permission to publish private source text or settings.

Relative CLI paths, including `--config`, `--workspace`, profile paths, and JSONL paths, resolve from the working directory, not the directory containing the configuration. attx does not expand a quoted `~` or interpolate `${VARIABLE}` in TOML strings. The shell may expand its own unquoted path syntax before attx receives it.

`ATTX_HOME` is a local application-state root, not a workspace override:

| Resource | With `ATTX_HOME` | Without it |
|---|---|---|
| Settings lookup | Existing `$ATTX_HOME/setting.toml`, then cwd | cwd `setting.toml` |
| Saved profiles | `$ATTX_HOME/profiles/` first | Platform config directory `attx/profiles/` |
| Global experience | `$ATTX_HOME/knowledge/` first | Platform config directory `attx/knowledge/` |
| Workspace/database | Input-derived or `--workspace` | Same |

Profiles and knowledge also search the platform configuration directory after the `ATTX_HOME` directory. New saved state goes to the first available directory. Linux commonly uses `~/.config/attx`, macOS its user configuration directory, and Windows its roaming configuration directory. Do not assume these platform directories are also searched for settings.

```bash
export ATTX_HOME="$HOME/.config/attx"
attx --config "./private settings.toml" --client main doctor --json
```

```powershell
$env:ATTX_HOME = Join-Path $HOME ".config\attx"
attx --config ".\private settings.toml" --client main doctor --json
```

## Complete baseline

```toml
[llm]
default_client = "main"

[[llm.clients]]
name = "main"
provider_type = "openai"
base_url = "https://api.example.com/v1"
api_key = "YOUR_LOCAL_API_KEY"
model = "your-provider-model"
timeout = 600
# temperature = 0.3
# reasoning_effort = "medium"
# max_tokens = 8192
stream = false
extra = {}

[translation]
worker_count = 8
rpm = 60
retry_count = 3
retry_delay = 2
batch_chars = 2500
max_context_items = 6
repair_rounds = 2
context_chars = 1200

[glossary]
enabled = false
min_occurrences = 10
max_terms = 200
inject_limit = 30

[learn]
auto_summarize = true
llm_review = false
```

`[llm]`, `default_client`, and `clients` are required when parsing an existing settings file. The other sections can be omitted as a whole; their missing keys also use defaults. Required client fields are not supplied by an implicit provider preset.

## Model selection

| Key | Type | Default | Effect |
|---|---|---|---|
| `llm.default_client` | string | Required | Name selected unless `--client` overrides it. |
| `llm.clients` | array of tables | Required | Named client definitions, written as `[[llm.clients]]`. |
| `name` | string | Required | Client lookup key. Keep names unique; lookup selects the first match. |
| `provider_type` | string | `"openai"` | Metadata only. It does not change endpoint, authentication, or request shape. |
| `base_url` | string | Required | Prefix for `{base_url}/chat/completions`, with trailing slashes trimmed. |
| `api_key` | string | Required | Local credential used for HTTP bearer authentication. |
| `model` | string | Required | Provider's model identifier, sent in the request. |
| `timeout` | unsigned integer seconds (`u64`) | `600` | Per-request timeout, with an effective minimum of 30 seconds. |
| `temperature` | optional floating-point number | Omitted | Overrides call-site temperature. If absent, translation sends `0.3`; JSON helpers send `0.0`. |
| `reasoning_effort` | optional string | Omitted | Nonempty value sent as `reasoning_effort`; empty value is not sent. |
| `max_tokens` | optional unsigned integer (`u32`) | Omitted | Sends `max_tokens` when supplied. |
| `stream` | boolean | `false` | Requests SSE when true; concatenates `choices[0].delta.content`. |
| `extra` | TOML table | `{}` | Additional JSON request fields merged last, except `messages`. |

Only the OpenAI-compatible Chat Completions protocol is implemented. Setting `provider_type = "anthropic"` does not add Anthropic Messages support, and setting it to `"google"` does not add Gemini's native API. Use a compatible gateway if the provider does not expose this protocol. There is no automatic key rotation, provider failover, or model discovery.

TOML numeric types and ranges must fit the schema. Provider-specific temperature ranges and reasoning values are not validated into a vendor-neutral preset; the provider can reject them. Configuration is not a secret-variable substitution language.

### Optional fields and precedence

The request starts with `model`, call-site/default `temperature`, and attx's `messages`. Named optional fields are added next. `extra` then adds or overrides fields, including `model`, `temperature`, `reasoning_effort`, token limits, or `stream`. An `extra.messages` entry is ignored so it cannot replace attx's prompts. A final `stream` value other than boolean `true` is removed and ordinary JSON response parsing is used.

```toml
[[llm.clients]]
name = "gateway"
base_url = "https://gateway.example/v1"
api_key = "YOUR_LOCAL_API_KEY"
model = "provider-model"
max_tokens = 4096
extra = { max_tokens = 8192, top_p = 0.9 }
```

This sends `max_tokens = 8192`. Arbitrary request fields are passed through, not checked for model compatibility. Avoid sending both `max_tokens` and `max_completion_tokens` unless the endpoint explicitly permits it. `extra` cannot delete the ordinary `temperature` field by omission; a restrictive endpoint may require a supported override or compatible gateway.

Streaming is a transport option, not a live token display or lower-cost mode. The response is read before translation validation. The parser can accept ordinary Chat Completions JSON from a gateway that ignores streaming. Truncated output, malformed SSE, and invalid completion structures remain failures.

### Provider examples

These examples show endpoint/model shapes, not an assertion that your account has access or that every optional field is supported.

```toml
[llm]
default_client = "openai"

[[llm.clients]]
name = "openai"
base_url = "https://api.openai.com/v1"
api_key = "YOUR_LOCAL_API_KEY"
model = "gpt-4o"

[[llm.clients]]
name = "deepseek"
base_url = "https://api.deepseek.com/v1"
api_key = "YOUR_LOCAL_API_KEY"
model = "deepseek-chat"

[[llm.clients]]
name = "local"
base_url = "http://127.0.0.1:8000/v1"
api_key = "local-server-token"
model = "your-loaded-model"
```

Choose one without changing files:

```bash
attx --config "./setting.toml" --client deepseek doctor --ping --json
```

For a compatible reasoning model that expects a completion-token limit:

```toml
# Inside its [[llm.clients]] block:
reasoning_effort = "medium"
extra = { max_completion_tokens = 8192 }
# Do not also set max_tokens for this example.
```

Consult the endpoint's model documentation before adding `response_format`, vendor reasoning objects, or sampling controls. Translation expects a top-level JSON array in model content; forcing an incompatible JSON-object response mode can cause retries.

## Translation policy

| Key | Type | Default | Effect |
|---|---|---|---|
| `worker_count` | unsigned integer (`usize`) | `8` | Concurrent blocking worker threads. Effective minimum 1, capped by available batches. |
| `rpm` | unsigned integer (`u32`) | `60` | Shared request-start rate; `0` disables the limiter. Initial, retry, and split requests share it within a translator. |
| `retry_count` | unsigned integer (`u32`) | `3` | Additional per-unit attempts within one translator pass. Network/408/429/5xx retry; model failures retry only rejected units with narrower batches. |
| `retry_delay` | unsigned integer seconds (`u64`) | `2` | Delay before a retry. |
| `batch_chars` | unsigned integer (`usize`) | `2500` | Source character budget per batch. Not a token count or model-context-window limit; an indivisible large unit can exceed it. `0` still sends individual nonempty units, not zero work. |
| `max_context_items` | unsigned integer (`usize`) | `6` | Maximum units in one batch, with effective minimum 1. Despite the name, it is not the neighboring-context character budget. |
| `repair_rounds` | unsigned integer (`usize`) | `2` | Extra repair passes after the initial workspace translation pass. `0` disables extra passes. Explicit `repair` still makes at least one pass. |
| `context_chars` | unsigned integer (`usize`) | `1200` | Total character budget for previous/next context per request, including labels. `0` disables neighbor injection. |

Neighbor context stays within scene/file grouping and follows natural location order. It can include already committed translations. Glossary and prompt notes add separate prompt content; `context_chars` does not cap the whole HTTP body.

One unit gets at most `1 + retry_count` attempts in a translator pass. Workspace `translate` and `run` permit an initial pass plus `repair_rounds` extra passes. Explicit `repair` permits `max(1, repair_rounds)` passes. A fatal permanent HTTP 4xx such as 400, 401, or 403 aborts queued work without narrower retry spam; already in-flight HTTP requests may finish. 408 and 429 are retryable exceptions. Exhausted recoverable failures become visible source placeholders, not successful translations.

Standalone `translate-jsonl` uses translator retries but not workspace repair passes or workspace resumption. Starting another command is a new request budget.

## Glossary policy

| Key | Type | Default | Effect |
|---|---|---|---|
| `enabled` | boolean | `false` | Builds a paid glossary during `run`. Explicit `glossary build` is independent of this flag. |
| `min_occurrences` | unsigned integer (`usize`) | `10` | Required number of source units containing a model-proposed term; multiple hits in one unit count once. Speaker/namebox terms have special handling. Effective floor 1. |
| `max_terms` | unsigned integer (`usize`) | `200` | Candidate cap, ranked by recurrence. Effective floor 1. |
| `inject_limit` | unsigned integer (`usize`) | `30` | Maximum relevant active terms injected into a translation batch; `0` disables term injection. |

The extractor has a hard cap of 40 source batches, each using a 3500-character source budget, so glossary discovery is not exhaustive. `truncated` reports candidates dropped by `max_terms`, not source batches omitted by that hard cap. `glossary check` uses substring matching and is advisory, especially for inflected languages.

`run --glossary` forces construction; `run --no-glossary` disables it for that invocation. These flags conflict and cannot be combined. An ordinary optional glossary failure is reported without stopping translation; fatal provider configuration/authentication failures stop the run.

## Learning policy

| Key | Type | Default | Effect |
|---|---|---|---|
| `auto_summarize` | boolean | `true` | Captures extraction experience after actual successful writeback using cache evidence. Ordinary capture makes no HTTP requests. |
| `llm_review` | boolean | `false` | Adds paid model review to proposed experience entries when summarizing. |

`writeback --no-learn` disables that invocation's automatic summary. `learn summarize --llm` explicitly requests model review. Pending destructive extraction rules need user review; the model does not authorize them. Prompt notes are separate from glossary terms and do not automatically retranslate completed cache entries.

## Cost controls and incompatible expectations

To avoid surprise format-inference costs, use `run --no-infer`. It does not disable local Auto detection. Disable glossary with `--no-glossary` or leave `enabled=false`; leave `llm_review=false` for local-only learning. Use `--limit N --no-writeback` when you deliberately want a trial, and reduce `repair_rounds` or `retry_count` if the budget matters more than recovery attempts.

Lower concurrency does not itself reduce total tokens. Smaller batches can improve response reliability but repeat more prompt overhead. Higher output-token limits reduce truncation risk but are not a price quote. Neighbor context and glossary injection increase input tokens. `rpm=0` plus many workers can exceed provider limits. Each independently started process has its own limiter, not a shared account-wide quota.

Do not combine a trial limit with an expectation of complete default writeback. Do not use `--allow-partial` merely to hide a blocked result. `--no-translate` prevents both translation and normal writeback; it also prevents automatic paid inference and run-time glossary construction.

## Secrets and migration

Keep the real key in the local settings file. attx does not write it to `attx.db`, JSONL, or normal reports. The request uses it for authentication, not as translation prompt content. Protect local file permissions, backups, environment captures, and provider error logs. A provider can echo sensitive content in an error; inspect and redact diagnostics before sharing. `setting.toml` is gitignored, but a differently named private file may not be.

For an upgrade to 0.10.1, keep existing clients and optional settings. Missing `repair_rounds` and `context_chars` use `2` and `1200`; add explicit values if you want older no-extra-repair behavior. Do not overwrite a working private file with the shipped example. Completion automation must now handle structured exit 2, blocked default writeback, honest passthrough counts, and Auto coverage. Old cache text is checked and normalized through current quality rules. Keep workspace identity and profile snapshots unchanged; use a new workspace for a different language pair or profile.

Related schemas: [profiles](profiles.md), [preserve/glossary/experience files](usage.md), and [workspace interchange](workspace.md).