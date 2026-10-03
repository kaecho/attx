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
    /// Detect: every regex must match a candidate's first 64 KiB of decoded text.
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
    detect_rules: Vec<Regex>,
    json_keys: Vec<String>,
    json_paths: Vec<String>,
    published: BTreeMap<String, Vec<String>>,
    output_paths: std::collections::BTreeSet<PathBuf>,
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
        let detect_rules = profile.detect_regex.iter()
            .map(|pattern| Regex::new(pattern).with_context(|| format!("bad detect_regex: {pattern}")))
            .collect::<Result<Vec<_>>>()?;

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
            detect_rules,
            json_keys,
            json_paths,
            published: BTreeMap::new(),
            output_paths: std::collections::BTreeSet::new(),
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

    /// Previously committed translations are valid live anchor contents on later writeback.
    pub fn with_published(mut self, published: BTreeMap<String, Vec<String>>) -> Self {
        self.published = published;
        self
    }

    /// Only committed generated siblings are excluded; language-named source assets remain candidates.
    pub fn with_output_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.output_paths = paths.iter().map(|path| normalized_path(path)).collect();
        self
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
        let metadata = std::fs::symlink_metadata(input)
            .with_context(|| format!("inspect custom input {}", input.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("custom input cannot be a symlink: {}", input.display());
        }
        if metadata.is_file() {
            return Ok(vec![input.to_path_buf()]);
        }
        if !metadata.is_dir() {
            bail!("input is not a file or directory: {}", input.display());
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
                !(e.file_type().is_symlink() || name.starts_with(".attx") || name.starts_with("translated-")
                    || name == ".git" || name == "node_modules" || name.ends_with(".attxbak"))
            })
        {
            let entry = entry?;
            if entry.file_type().is_file() && self.claims_extension(entry.path())
                && (self.profile.overwrite || !self.output_paths.contains(&normalized_path(entry.path())))
            {
                files.push(entry.path().to_path_buf());
            }
        }
        Ok(files)
    }

    fn source_path(&self, file: &Path) -> Result<PathBuf> {
        if self.profile.overwrite {
            let mut backup = file.as_os_str().to_os_string();
            backup.push(".attxbak");
            let backup = PathBuf::from(backup);
            match std::fs::symlink_metadata(&backup) {
                Ok(metadata) => {
                    if !metadata.is_file() || metadata.file_type().is_symlink() {
                        bail!("custom backup must be a regular file: {}", backup.display());
                    }
                    return Ok(backup);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("inspect {}", backup.display())),
            }
        }
        Ok(file.to_path_buf())
    }

    fn source_text(&self, file: &Path) -> Result<String> {
        strict_source_text(&self.source_path(file)?)
    }

    fn extract_file(
        &self,
        file: &Path,
        rel: &str,
        source_lang: &str,
        units: &mut Vec<TextUnit>,
    ) -> Result<()> {
        let body = self.source_text(file)?;
        self.extract_body(&body, rel, source_lang, units)
    }

    fn extract_body(&self, body: &str, rel: &str, source_lang: &str, units: &mut Vec<TextUnit>) -> Result<()> {
        let json = parse_json_if_rules(body, &self.json_keys, &self.json_paths);
        if let Some(root) = json {
            let mut hits: Vec<(String, String)> = Vec::new(); // (path, text)
            collect_json_units(&root, &self.json_keys, &self.json_paths, &mut hits);
            for (path, text) in hits {
                let previous_len = units.len();
                push_json_unit(units, self.engine_id, rel, &path, &text, source_lang);
                if units.len() != previous_len {
                    units.last_mut().unwrap().payload = serde_json::to_string(&json_array_anchor(&root, &path)?)?;
                }
            }
            return Ok(());
        }
        if self.line_rules.is_empty() {
            bail!("custom JSON source does not parse: {rel}");
        }
        for (i, raw_line) in body.lines().enumerate() {
            let line = raw_line.trim_end_matches('\r');
            if self.skip_rules.iter().any(|re| re.is_match(line)) {
                continue;
            }
            let Some((text, role, span)) = self.match_line(line) else {
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
                payload: serde_json::to_string(&LineAnchor {
                    prefix: line[..span.0].to_string(),
                    suffix: line[span.1..].to_string(),
                })?,
            });
        }
        Ok(())
    }

    /// First rule whose `text` group matches wins.
    fn match_line(&self, line: &str) -> Option<(String, String, (usize, usize))> {
        for re in &self.line_rules {
            if let Some(caps) = re.captures(line)
                && let Some(m) = caps.name("text")
            {
                let role = caps
                    .name("role")
                    .map(|r| r.as_str().to_string())
                    .unwrap_or_default();
                return Some((m.as_str().to_string(), role, (m.start(), m.end())));
            }
        }
        None
    }

    fn writeback_file(
        &self,
        file: &Path,
        rel: &str,
        target_lang: &str,
        by_location: &BTreeMap<&str, (&TextUnit, &Translation)>,
    ) -> Result<Option<OutputFile>> {
        let source = self.source_text(file)?;
        let body = strict_source_text(file)?;
        let source_json = parse_json_if_rules(&source, &self.json_keys, &self.json_paths);
        let live_json = parse_json_if_rules(&body, &self.json_keys, &self.json_paths);
        let rendered = if let Some(mut root) = live_json {
            let original = source_json.context("custom source changed from lines to JSON")?;
            let mut allowed = Vec::new();
            collect_json_units(&original, &self.json_keys, &self.json_paths, &mut allowed);
            allowed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let prefix = format!("{rel}#");
            let mut touched = false;
            for (loc, (unit, tr)) in by_location {
                let path = loc.strip_prefix(&prefix)
                    .with_context(|| format!("invalid custom JSON anchor: {loc}"))?;
                let expected = unit.joined_text();
                if !allowed.binary_search_by(|(p, _)| p.as_str().cmp(path))
                    .is_ok_and(|index| allowed[index].1 == expected)
                {
                    bail!("custom source anchor changed at {loc}");
                }
                let snapshot: JsonAnchor = serde_json::from_str(&unit.payload)
                    .with_context(|| format!("custom JSON snapshot missing or invalid at {loc}; run extract --workspace again"))?;
                if json_array_anchor(&original, path)? != snapshot || json_array_anchor(&root, path)? != snapshot {
                    bail!("custom JSON array anchor shifted at {loc}");
                }
                let joined = tr.translation_lines.join("\n");
                let current = crate::adapter::get_json_path(&root, path)
                    .and_then(Value::as_str)
                    .with_context(|| format!("custom source anchor missing at {loc}"))?;
                if current != expected && current != joined
                    && self.published.get(&unit.id).is_none_or(|lines| lines.join("\n") != current)
                {
                    bail!("custom live text changed at {loc}");
                }
                if current != joined {
                    set_json_path(&mut root, path, Value::String(joined))?;
                    touched = true;
                }
            }
            if touched {
                let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
                let mut rendered = serde_json::to_string_pretty(&root)?;
                if newline == "\r\n" {
                    rendered = rendered.replace('\n', newline);
                }
                if body.ends_with('\n') {
                    rendered.push_str(newline);
                }
                rendered
            } else {
                body
            }
        } else {
            if source_json.is_some() {
                bail!("custom live JSON schema no longer parses: {}", file.display());
            }
            let original_lines: Vec<&str> = source.lines().collect();
            let mut lines: Vec<String> = body.split_inclusive('\n').map(str::to_string).collect();
            let prefix = format!("{rel}#L");
            for (loc, (unit, tr)) in by_location {
                let no = loc.strip_prefix(&prefix).and_then(|s| s.parse::<usize>().ok())
                    .with_context(|| format!("invalid custom line anchor: {loc}"))?;
                let index = no.checked_sub(1).context("custom line numbers start at one")?;
                let original = original_lines.get(index)
                    .with_context(|| format!("custom original line anchor missing at {loc}"))?;
                let source_span = self.checked_capture(original, loc)?;
                let expected = unit.joined_text();
                if original[source_span.0..source_span.1] != expected {
                    bail!("custom original text changed at {loc}");
                }
                let snapshot: LineAnchor = serde_json::from_str(&unit.payload)
                    .with_context(|| format!("custom line snapshot missing or invalid at {loc}; run extract --workspace again"))?;
                snapshot.verify(original, source_span, loc)?;
                reject_unsafe_capture(original, source_span, loc)?;
                let slot = lines.get_mut(index)
                    .with_context(|| format!("custom live line anchor missing at {loc}"))?;
                let line = slot.trim_end_matches(['\r', '\n']);
                let span = self.checked_capture(line, loc)?;
                snapshot.verify(line, span, loc)?;
                if tr.translation_lines.len() != 1 || tr.translation_lines[0].contains(['\r', '\n']) {
                    bail!("custom line translation cannot contain newlines at {loc}");
                }
                let joined = &tr.translation_lines[0];
                let current = &line[span.0..span.1];
                if current != expected && current != joined
                    && !self.published.get(&unit.id).is_some_and(|lines| lines.len() == 1 && lines[0] == current)
                {
                    bail!("custom live text changed at {loc}");
                }
                let mut candidate = line.to_string();
                candidate.replace_range(span.0..span.1, joined);
                let candidate_span = self.checked_capture(&candidate, loc)?;
                snapshot.verify(&candidate, candidate_span, loc)?;
                if &candidate[candidate_span.0..candidate_span.1] != joined {
                    bail!("custom translation changes capture boundaries at {loc}");
                }
                reject_target_delimiters(&snapshot, joined, loc)?;
                slot.replace_range(span.0..span.1, joined);
            }
            lines.concat()
        };
        let dest = if self.profile.overwrite {
            file.to_path_buf()
        } else {
            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("txt");
            output_sibling(file, target_lang, ext)
        };
        let mut output = OutputFile::text(dest, rendered);
        output.permissions = Some(std::fs::metadata(file)?.permissions());
        Ok(Some(output))
    }

    fn checked_capture(&self, line: &str, location: &str) -> Result<(usize, usize)> {
        if self.skip_rules.iter().any(|re| re.is_match(line)) {
            bail!("custom line became excluded at {location}");
        }
        line_capture(&self.line_rules, line)
            .with_context(|| format!("custom text capture missing at {location}"))
    }

    /// Apply extension, content signatures and trial extraction to the requested language.
    pub fn detects_source(&self, input: &Path, src: &str) -> bool {
        let Ok(files) = self.candidate_files(input) else { return false; };
        if files.is_empty() || files.iter().any(|file| !self.claims_extension(file)) {
            return false;
        }
        let mut signature_hits = vec![false; self.detect_rules.len()];
        let mut units = Vec::new();
        for file in &files {
            let Ok(body) = self.source_text(file) else { return false; };
            let mut end = body.len().min(65536);
            while !body.is_char_boundary(end) { end -= 1; }
            for (matched, re) in signature_hits.iter_mut().zip(&self.detect_rules) {
                *matched |= re.is_match(&body[..end]);
            }
            if self.extract_body(&body, &rel_name(input, file), src, &mut units).is_err() {
                return false;
            }
        }
        signature_hits.into_iter().all(|hit| hit) && units.len() >= self.profile.min_units.max(1)
    }

    /// Validate every candidate, including files with no extracted units, before accepting inference.
    pub fn validate_inferred_sources(&self, input: &Path, units: &[TextUnit]) -> Result<()> {
        let files = self.candidate_files(input)?;
        for file in &files {
            strict_source_text(file)?;
            let source = self.source_text(file)?;
            if parse_json_if_rules(&source, &self.json_keys, &self.json_paths).is_none() {
                if self.line_rules.is_empty() {
                    bail!("custom JSON source does not parse: {}", file.display());
                }
                for (index, line) in source.lines().enumerate() {
                    if self.skip_rules.iter().any(|re| re.is_match(line)) { continue; }
                    if let Some(span) = line_capture(&self.line_rules, line) {
                        reject_unsafe_capture(line, span, &format!("{}#L{}", file.display(), index + 1))?;
                    }
                }
            }
        }
        let translations: BTreeMap<String, Translation> = units.iter().map(|unit| (
            unit.id.clone(), Translation {
                unit_id: unit.id.clone(), translation_lines: unit.original_lines.clone(),
                source_hash: TextUnit::source_hash(&unit.original_lines), passthrough: false,
            }
        )).collect();
        for output in self.writeback(input, "attx-inference-check", units, &translations)? {
            let file = if self.profile.overwrite {
                output.path.clone()
            } else {
                files.iter().find(|file| {
                    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("txt");
                    output_sibling(file, "attx-inference-check", ext) == output.path
                }).cloned().context("inference output does not belong to a candidate source")?
            };
            if textio::decode_bytes(&output.bytes).text != strict_source_text(&file)? {
                bail!("inferred profile does not preserve decoded source text at {}", file.display());
            }
        }
        Ok(())
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
        if !self.detects_source(input, "auto") {
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
        let candidates: BTreeMap<String, PathBuf> = self.candidate_files(input)?.into_iter()
            .map(|file| (rel_name(input, &file), file)).collect();
        let source_paths: std::collections::BTreeSet<PathBuf> = candidates.values()
            .map(|file| normalized_path(file)).collect();
        let mut per_file: BTreeMap<String, BTreeMap<&str, (&TextUnit, &Translation)>> = BTreeMap::new();
        for u in units {
            let Some(tr) = translations.get(&u.id) else {
                continue;
            };
            let rel = candidates.keys().filter(|rel| {
                u.location.strip_prefix(rel.as_str()).is_some_and(|tail| tail.starts_with('#'))
            }).max_by_key(|rel| rel.len())
                .with_context(|| format!("custom source location is not a candidate: {}", u.location))?;
            if u.engine != self.engine_id {
                bail!("custom unit engine mismatch: {}", u.location);
            }
            if tr.unit_id != u.id || tr.source_hash != TextUnit::source_hash(&u.original_lines) {
                bail!("custom translation source identity mismatch: {}", u.location);
            }
            if per_file.entry(rel.clone()).or_default()
                .insert(u.location.as_str(), (u, tr)).is_some()
            {
                bail!("duplicate custom source location: {}", u.location);
            }
        }
        let mut out = Vec::new();
        for (rel, by_location) in &per_file {
            let file = candidates.get(rel).context("custom source file missing from candidate scan")?;
            if !self.profile.overwrite {
                let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("txt");
                let destination = output_sibling(file, target_lang, ext);
                let normalized = normalized_path(&destination);
                if source_paths.contains(&normalized) && !self.output_paths.contains(&normalized) {
                    bail!("custom output collides with a genuine source file: {}", destination.display());
                }
            }
            if let Some(o) = self.writeback_file(file, rel, target_lang, by_location)? {
                out.push(o);
            }
        }
        Ok(out)
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct JsonAnchor {
    array_lengths: Vec<(usize, usize)>,
}

fn json_array_anchor(root: &Value, path: &str) -> Result<JsonAnchor> {
    let mut current = root;
    let mut array_lengths = Vec::new();
    for (depth, part) in path.split('/').filter(|part| !part.is_empty()).enumerate() {
        current = if let Some(array) = current.as_array() {
            array_lengths.push((depth, array.len()));
            let index: usize = part.parse().with_context(|| format!("invalid custom JSON array index: {path}"))?;
            array.get(index)
        } else {
            current.as_object().and_then(|object| object.get(part))
        }.with_context(|| format!("custom JSON source anchor missing: {path}"))?;
    }
    if !current.is_string() {
        bail!("custom JSON source anchor is not a string: {path}");
    }
    Ok(JsonAnchor { array_lengths })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LineAnchor {
    prefix: String,
    suffix: String,
}

impl LineAnchor {
    fn verify(&self, line: &str, span: (usize, usize), location: &str) -> Result<()> {
        if line[..span.0] != self.prefix || line[span.1..] != self.suffix {
            bail!("custom line schema shifted at {location}");
        }
        Ok(())
    }
}

fn line_capture(rules: &[Regex], line: &str) -> Option<(usize, usize)> {
    rules.iter().find_map(|re| re.captures(line).and_then(|caps| {
        caps.name("text").map(|capture| (capture.start(), capture.end()))
    }))
}

fn strict_source_text(file: &Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(file)
        .with_context(|| format!("inspect custom source {}", file.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("custom source must be a regular file: {}", file.display());
    }
    let bytes = std::fs::read(file).with_context(|| format!("read custom source {}", file.display()))?;
    let decoded = textio::decode_bytes(&bytes);
    if decoded.lossy {
        bail!("custom source decoding is lossy at {} ({})", file.display(), decoded.encoding);
    }
    if decoded.text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n')) {
        bail!("custom source contains binary/control data: {}", file.display());
    }
    Ok(decoded.text)
}

fn reject_unsafe_capture(line: &str, span: (usize, usize), location: &str) -> Result<()> {
    let mut quote = None;
    let mut escaped = false;
    for c in line[..span.0].chars() {
        if escaped { escaped = false; continue; }
        if c == '\\' { escaped = true; continue; }
        if let Some(open) = quote {
            if c == open { quote = None; }
        } else if matches!(c, '\'' | '"' | '`') {
            quote = Some(c);
        }
    }
    let text = &line[span.0..span.1];
    let literal = text.trim();
    if quote.is_some() || escaped || text.contains('\\')
        || ['\'', '"', '`'].into_iter().any(|delimiter| text.chars().filter(|c| *c == delimiter).take(2).count() == 2)
        || matches!(literal.chars().next(), Some('\'' | '"' | '`'))
        || matches!(literal.chars().last(), Some('\'' | '"' | '`'))
    {
        bail!("custom line capture requires a declared escaping strategy at {location}; use a syntax-aware adapter or JSON rules");
    }
    Ok(())
}

fn reject_target_delimiters(anchor: &LineAnchor, target: &str, location: &str) -> Result<()> {
    for delimiter in [anchor.prefix.chars().rev().find(|c| !c.is_whitespace()),
        anchor.suffix.chars().find(|c| !c.is_whitespace())].into_iter().flatten()
    {
        if !delimiter.is_alphanumeric() && !delimiter.is_whitespace() && target.contains(delimiter) {
            bail!("custom translation contains source delimiter {delimiter:?} at {location}");
        }
    }
    let ascii_syntax = [anchor.prefix.chars().rev().find(|c| !c.is_whitespace()),
        anchor.suffix.chars().find(|c| !c.is_whitespace())].into_iter().flatten()
        .any(|c| c.is_ascii_punctuation());
    if ascii_syntax && target.contains(['\'', '"', '`', '\\']) {
        bail!("custom translation contains unsupported literal syntax at {location}; use a syntax-aware adapter or JSON rules");
    }
    if target.chars().any(|c| c.is_control() && c != '\t') {
        bail!("custom translation contains control data at {location}");
    }
    Ok(())
}

fn normalized_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => { normalized.pop(); }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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
    crate::fileio::write_atomic(&dest, &std::fs::read(profile_path)?)?;
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
# Captures in ASCII quotes/backticks or escaped strings need a syntax-aware adapter.
# Line translations cannot contain newlines, source delimiters or literal syntax.
# Use JSON rules for JSON strings so quotes and newlines are escaped correctly.
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

    fn line_adapter(pattern: &str, overwrite: bool) -> CustomAdapter {
        CustomAdapter::compile(FormatProfile {
            name: "safety-test".into(), label: String::new(), extensions: vec!["ks".into()],
            detect_regex: Vec::new(), min_units: 1, overwrite, skip_lines: vec!["^;".into()],
            notes: String::new(), rules: vec![Rule::LineRegex { pattern: pattern.into() }],
        }).unwrap()
    }

    #[test]
    fn overwrite_uses_live_comments_and_original_backup() {
        let dir = crate::adapter::test_dir("profile-live-overwrite");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "; original\r\n【直哉】「おはよう」\r\n").unwrap();
        let adapter = line_adapter("^【(?P<role>[^】]+)】「(?P<text>.+)」$", true);
        let units = adapter.extract(&input, "ja").unwrap();
        std::fs::write(dir.join("scene.ks.attxbak"), std::fs::read(&input).unwrap()).unwrap();
        std::fs::write(&input, "; live edit\r\n【直哉】「旧译」\r\n").unwrap();
        let adapter = adapter.with_published(BTreeMap::from([(units[0].id.clone(), vec!["旧译".into()])]))
            .with_output_paths(vec![input.clone()]);
        let output = adapter.writeback(&input, "zh", &units, &tr_map(&units, "新译")).unwrap();
        assert_eq!(String::from_utf8(output[0].bytes.clone()).unwrap(), "; live edit\r\n【直哉】「新译」\r\n");
        assert_eq!(adapter.extract(&input, "ja").unwrap()[0].original_lines, ["おはよう"]);
        assert_eq!(adapter.extract(&dir, "ja").unwrap().len(), 1, "overwrite manifest must not hide the source");
    }

    #[test]
    fn overwrite_json_preserves_unrelated_live_values() {
        let dir = crate::adapter::test_dir("profile-live-json");
        let input = dir.join("data.ks");
        let original = "{\"message\":\"こんにちは\",\"flag\":1}\r\n";
        std::fs::write(&input, original).unwrap();
        let mut profile = line_adapter("^(?P<text>.+)$", true).profile().clone();
        profile.rules = vec![Rule::JsonKeys { keys: vec!["message".into()] }];
        let adapter = CustomAdapter::compile(profile).unwrap();
        let units = adapter.extract(&input, "ja").unwrap();
        std::fs::write(dir.join("data.ks.attxbak"), original).unwrap();
        std::fs::write(&input, "{\"message\":\"こんにちは\",\"flag\":9,\"added\":true}\r\n").unwrap();
        let output = adapter.writeback(&input, "zh", &units, &tr_map(&units, "译")).unwrap();
        let json: Value = serde_json::from_slice(&output[0].bytes).unwrap();
        assert_eq!(json["flag"], 9);
        assert_eq!(json["added"], true);
        assert_eq!(json["message"], "译");
        assert!(String::from_utf8(output[0].bytes.clone()).unwrap().contains("\r\n"));
    }

    #[test]
    fn shifted_missing_and_changed_anchors_fail() {
        let dir = crate::adapter::test_dir("profile-shifted");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "【直哉】「おはよう」\n").unwrap();
        let adapter = line_adapter("^【(?P<role>[^】]+)】「(?P<text>.+)」$", false);
        let units = adapter.extract(&input, "ja").unwrap();
        for body in ["; inserted\n【直哉】「おはよう」\n", "【別人】「おはよう」\n", "【直哉】「別の台詞」\n", ""] {
            std::fs::write(&input, body).unwrap();
            assert!(adapter.writeback(&input, "zh", &units, &tr_map(&units, "译")).is_err(), "{body:?}");
        }
        std::fs::remove_file(&input).unwrap();
        assert!(adapter.writeback(&input, "zh", &units, &tr_map(&units, "译")).is_err());
    }

    #[test]
    fn inferred_quoted_and_escaped_captures_fail() {
        let dir = crate::adapter::test_dir("profile-quoted");
        let input = dir.join("scene.ks");
        for (pattern, body) in [
            (r#"^say\("(?P<text>.*)"\)$"#, "say(\"こんにちは\")\n"),
            ("^say\\('(?P<text>.*)'\\)$", "say('こんにちは')\n"),
            ("^say\\(`(?P<text>.*)`\\)$", "say(`こんにちは`)\n"),
            ("^text=(?P<text>.*)$", "text=こんにちは\\n世界\n"),
            ("^(?P<text>.*)$", "say(\"こんにちは\")\n"),
        ] {
            std::fs::write(&input, body).unwrap();
            let adapter = line_adapter(pattern, false);
            let units = adapter.extract(&input, "ja").unwrap();
            assert!(!units.is_empty());
            assert!(adapter.validate_inferred_sources(&input, &units).is_err(), "{body:?}");
            assert!(adapter.writeback(&input, "zh", &units, &tr_map(&units, "译")).is_err());
        }
    }

    #[test]
    fn target_delimiters_and_newlines_fail() {
        let dir = crate::adapter::test_dir("profile-delimiter");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "「こんにちは」\r\n").unwrap();
        let adapter = line_adapter("^「(?P<text>.*)」$", false);
        let units = adapter.extract(&input, "ja").unwrap();
        adapter.validate_inferred_sources(&input, &units).unwrap();
        for target in ["译」坏", "译\n坏", "译\r坏"] {
            assert!(adapter.writeback(&input, "zh", &units, &tr_map(&units, target)).is_err());
        }
        let output = adapter.writeback(&input, "en", &units, &tr_map(&units, "He said \"hello\"")).unwrap();
        assert_eq!(String::from_utf8(output[0].bytes.clone()).unwrap(), "「He said \"hello\"」\r\n");
        std::fs::write(&input, "text=こんにちは\n").unwrap();
        let adapter = line_adapter("^text=(?P<text>.*)$", false);
        let units = adapter.extract(&input, "ja").unwrap();
        assert!(adapter.writeback(&input, "en", &units, &tr_map(&units, "He said \"hello\"")).is_err());
    }

    #[test]
    fn every_candidate_is_validated_including_nonunits() {
        let dir = crate::adapter::test_dir("profile-all-source-validation");
        std::fs::write(dir.join("a.ks"), "こんにちは\r\n").unwrap();
        let adapter = line_adapter("^(?P<text>.*)$", false);
        let units = adapter.extract(&dir, "ja").unwrap();
        adapter.validate_inferred_sources(&dir, &units).unwrap();
        std::fs::write(dir.join("z.ks"), b"English only\0binary").unwrap();
        assert!(adapter.validate_inferred_sources(&dir, &units).is_err());
        std::fs::write(dir.join("z.ks"), [0xff, 0xfe, 0x00, 0xd8]).unwrap();
        assert!(adapter.validate_inferred_sources(&dir, &units).is_err());
    }

    #[test]
    fn detect_uses_requested_language_and_directory_signatures() {
        let dir = crate::adapter::test_dir("profile-source-detect");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "Hello world\n").unwrap();
        let mut profile = line_adapter("^(?P<text>.*)$", false).profile().clone();
        profile.detect_regex = vec!["Hello".into()];
        let adapter = CustomAdapter::compile(profile.clone()).unwrap();
        assert!(adapter.detects_source(&input, "en"));
        assert!(!adapter.detects_source(&input, "ja"));
        assert!(adapter.detects_source(&dir, "en"));
        profile.detect_regex = vec!["missing".into()];
        assert!(!CustomAdapter::compile(profile.clone()).unwrap().detects_source(&dir, "en"));
        profile.detect_regex.clear();
        profile.min_units = 2;
        assert!(!CustomAdapter::compile(profile).unwrap().detects_source(&input, "en"));
        let wrong_ext = dir.join("scene.txt");
        std::fs::write(&wrong_ext, "Hello world\n").unwrap();
        assert!(!adapter.detects_source(&wrong_ext, "en"));
    }

    #[test]
    fn legacy_snapshots_refresh_without_changing_unit_ids() {
        let dir = crate::adapter::test_dir("profile-legacy-refresh");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "「こんにちは」\n").unwrap();
        let adapter = line_adapter("^「(?P<text>.*)」$", false);
        let mut legacy = adapter.extract(&input, "ja").unwrap();
        legacy[0].payload.clear();
        let translations = tr_map(&legacy, "Hello");
        let error = adapter.writeback(&input, "en", &legacy, &translations).err().unwrap();
        assert!(format!("{error:#}").contains("run extract --workspace again"));
        let refreshed = adapter.extract(&input, "ja").unwrap();
        assert_eq!(legacy[0].id, refreshed[0].id);
        assert!(adapter.writeback(&input, "en", &refreshed, &translations).is_ok());
    }

    #[test]
    fn generated_manifest_keeps_genuine_language_assets_and_rejects_collisions() {
        let dir = crate::adapter::test_dir("profile-generated-manifest");
        let source = dir.join("scene.ks");
        let language_asset = dir.join("scene.en.ks");
        std::fs::write(&source, "こんにちは\n").unwrap();
        std::fs::write(&language_asset, "別の台詞\n").unwrap();
        std::fs::create_dir_all(dir.join("translated-zh")).unwrap();
        std::fs::write(dir.join("translated-zh/generated.ks"), "生成済み\n").unwrap();
        std::fs::write(dir.join(".attx-note.ks"), "メタデータ\n").unwrap();
        std::fs::write(dir.join("old.ks.attxbak"), "バックアップ\n").unwrap();
        let adapter = line_adapter("^(?P<text>.*)$", false);
        let units = adapter.extract(&dir, "ja").unwrap();
        assert_eq!(units.len(), 2, "genuine language asset must remain a source");
        assert!(adapter.writeback(&dir, "en", &units, &tr_map(&units, "Translated")).is_err());
        let adapter = adapter.with_output_paths(vec![language_asset]);
        let units = adapter.extract(&dir, "ja").unwrap();
        assert_eq!(units.len(), 1, "only recorded generated file is excluded");
        assert_eq!(adapter.writeback(&dir, "en", &units, &tr_map(&units, "Translated")).unwrap().len(), 1);
    }

    #[test]
    fn json_noop_preserves_formatting_and_array_shifts_fail() {
        let dir = crate::adapter::test_dir("profile-json-snapshot");
        let input = dir.join("scene.ks");
        let source = "{ \"items\" : [ { \"message\" : \"こんにちは\" } ], \"flag\":1 }\r\n";
        std::fs::write(&input, source).unwrap();
        let mut profile = line_adapter("^(?P<text>.*)$", true).profile().clone();
        profile.rules = vec![Rule::JsonKeys { keys: vec!["message".into()] }];
        let adapter = CustomAdapter::compile(profile).unwrap();
        let units = adapter.extract(&input, "ja").unwrap();
        adapter.validate_inferred_sources(&input, &units).unwrap();
        let output = adapter.writeback(&input, "en", &units, &tr_map(&units, "こんにちは")).unwrap();
        assert_eq!(output[0].bytes, source.as_bytes());
        std::fs::write(dir.join("scene.ks.attxbak"), source).unwrap();
        std::fs::write(&input, "{\"items\":[{\"message\":\"こんにちは\"},{\"message\":\"こんにちは\"}],\"flag\":1}\n").unwrap();
        assert!(adapter.writeback(&input, "en", &units, &tr_map(&units, "Hello")).is_err());
    }

    #[test]
    fn json_rules_escape_target_quotes_and_newlines() {
        let dir = crate::adapter::test_dir("profile-json-escaping");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "{\"message\":\"こんにちは\"}\n").unwrap();
        let mut profile = line_adapter("^(?P<text>.*)$", false).profile().clone();
        profile.rules = vec![Rule::JsonKeys { keys: vec!["message".into()] }];
        let adapter = CustomAdapter::compile(profile).unwrap();
        let units = adapter.extract(&input, "ja").unwrap();
        let target = "He said \"hello\"\nnext line\\end";
        let output = adapter.writeback(&input, "en", &units, &tr_map(&units, target)).unwrap();
        let json: Value = serde_json::from_slice(&output[0].bytes).unwrap();
        assert_eq!(json["message"], target);
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlinks_are_excluded_and_source_modes_inherit() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = crate::adapter::test_dir("profile-source-permissions");
        let input = dir.join("scene.ks");
        std::fs::write(&input, "こんにちは\n").unwrap();
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&input, dir.join("linked.ks")).unwrap();
        let adapter = line_adapter("^(?P<text>.*)$", false);
        let units = adapter.extract(&dir, "ja").unwrap();
        assert_eq!(units.len(), 1);
        let output = adapter.writeback(&dir, "en", &units, &tr_map(&units, "Hello")).unwrap();
        assert_eq!(output[0].permissions.as_ref().unwrap().mode() & 0o777, 0o640);
        assert!(adapter.extract(&dir.join("linked.ks"), "ja").is_err());
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
