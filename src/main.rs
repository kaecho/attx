mod adapter;
mod config;
mod glossary;
mod knowledge;
mod learn;
mod llm;
mod model;
mod pipeline;
mod profile;
mod quality;
mod store;
mod textio;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "attx",
    version,
    about = "Agent Translation Toolkit eXtensible — universal AI translation framework (games, ebooks, documents, subtitles, localization files, custom formats)"
)]
struct Cli {
    /// Path to setting.toml (default: ./setting.toml or $ATTX_HOME/setting.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// LLM client name from setting.toml (default: [llm].default_client)
    #[arg(long, global = true)]
    client: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Check config and optional LLM connectivity
    Doctor {
        /// Also ping the LLM with a tiny request
        #[arg(long)]
        ping: bool,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// List supported format adapters (built-in + saved profiles) as JSON
    Formats,
    /// Detect the format adapter for a file or directory
    Detect {
        /// Input file (epub/docx/txt/md/srt/…) or game directory (--game also accepted)
        #[arg(long, alias = "game")]
        input: PathBuf,
    },
    /// Inspect an unknown input: encoding, structure, samples (JSON report)
    Analyze {
        #[arg(long, alias = "game")]
        input: PathBuf,
        /// Source language for text-density stats: ja | en
        #[arg(long, default_value = "ja")]
        src: String,
    },
    /// Manage custom format profiles (teach attx new formats)
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    /// Learn, review and apply extraction experience (self-improvement)
    Learn {
        #[command(subcommand)]
        command: LearnCommands,
    },
    /// Build and manage the workspace glossary (consistent proper-noun names)
    Glossary {
        #[command(subcommand)]
        command: GlossaryCommands,
    },
    /// Register / open a workspace for an input
    Init {
        /// Input file or game directory (--game also accepted)
        #[arg(long, alias = "game")]
        input: PathBuf,
        /// Force format id (see `attx formats`). Auto-detect when omitted.
        #[arg(long)]
        engine: Option<String>,
        /// Custom profile: path to a .toml file, or a saved profile name
        #[arg(long)]
        profile: Option<String>,
        /// Source language: ja | en
        #[arg(long, default_value = "ja")]
        src: String,
        /// Target language (e.g. zh, zh-tw, en, ko)
        #[arg(long, default_value = "zh")]
        dst: String,
        /// Workspace dir (default: <dir>/.attx or <parent>/.attx-<stem>)
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Extract texts from the input into the workspace DB
    Extract {
        #[arg(long)]
        workspace: PathBuf,
        /// Ignore learned extraction rules (escape hatch: pre-knowledge behaviour)
        #[arg(long)]
        no_knowledge: bool,
    },
    /// Translate pending texts via LLM
    Translate {
        #[arg(long)]
        workspace: PathBuf,
        /// Limit number of units (debug / trial run)
        #[arg(long)]
        limit: Option<usize>,
        /// Dry-run: print batch plan only
        #[arg(long)]
        dry_run: bool,
        /// Re-queue units whose translation was a passthrough placeholder
        #[arg(long)]
        retry_passthrough: bool,
    },
    /// Write translations back into the game
    Writeback {
        #[arg(long)]
        workspace: PathBuf,
        /// Dry-run: report planned file changes only
        #[arg(long)]
        dry_run: bool,
        /// Skip the automatic post-writeback experience summary
        #[arg(long)]
        no_learn: bool,
    },
    /// extract → translate → writeback
    Run {
        /// Input file or game directory (--game also accepted)
        #[arg(long, alias = "game")]
        input: PathBuf,
        #[arg(long)]
        engine: Option<String>,
        /// Custom profile: path to a .toml file, or a saved profile name
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "ja")]
        src: String,
        #[arg(long, default_value = "zh")]
        dst: String,
        #[arg(long)]
        workspace: Option<PathBuf>,
        #[arg(long)]
        limit: Option<usize>,
        /// Skip LLM (extract + status only)
        #[arg(long)]
        no_translate: bool,
        /// Skip writeback
        #[arg(long)]
        no_writeback: bool,
        /// Build a glossary before translating, regardless of [glossary].enabled
        #[arg(long)]
        glossary: bool,
        /// Never build a glossary, even when [glossary].enabled is true
        #[arg(long, conflicts_with = "glossary")]
        no_glossary: bool,
    },
    /// Status of workspace translations
    Status {
        #[arg(long)]
        workspace: PathBuf,
    },
    /// Generic JSONL translate (no engine). Issue #11 surface.
    /// Input line: {"id","text","context"?,"role"?,"item_type"?}
    /// Output line: {"id","text","translation","translation_lines"}
    TranslateJsonl {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "ja")]
        src: String,
        #[arg(long, default_value = "zh")]
        dst: String,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Export workspace texts as JSONL
    ExportJsonl {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// pending | all | translated | passthrough
        #[arg(long, default_value = "pending")]
        filter: String,
    },
    /// Import translations from JSONL into workspace
    ImportJsonl {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ProfileCommands {
    /// Write a documented profile template to a file
    New {
        /// Output path for the template (e.g. ./myformat.toml)
        #[arg(long)]
        output: PathBuf,
        /// Profile name baked into the template
        #[arg(long, default_value = "myformat")]
        name: String,
    },
    /// Trial-extract with a profile and report matched units (JSON)
    Test {
        /// Profile .toml path or saved profile name
        #[arg(long)]
        profile: String,
        #[arg(long, alias = "game")]
        input: PathBuf,
        #[arg(long, default_value = "ja")]
        src: String,
        /// Sample size in the report
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Also run an in-memory writeback with marker translations
        #[arg(long)]
        roundtrip: bool,
    },
    /// Remember a profile: copy it into the user profile directory
    Save {
        /// Profile .toml path
        #[arg(long)]
        profile: PathBuf,
        /// Overwrite an existing saved profile with the same name
        #[arg(long)]
        force: bool,
    },
    /// List saved profiles (JSON)
    List,
}

/// Self-improvement: learn extraction experience from evidence, review, apply.
#[derive(Subcommand, Debug)]
enum LearnCommands {
    /// Summarise a translated workspace into experience entries
    #[command(alias = "scan")]
    Summarize {
        #[arg(long)]
        workspace: PathBuf,
        /// Additionally ask the LLM to sanity-check each proposed entry (costs money)
        #[arg(long)]
        llm: bool,
    },
    /// Show entries awaiting approval, with their evidence (JSON)
    Pending,
    /// Approve / reject pending entries by 1-based index
    Review {
        /// Comma-separated indices to approve (e.g. 1,3)
        #[arg(long, value_delimiter = ',')]
        approve: Vec<usize>,
        /// Comma-separated indices to reject
        #[arg(long, value_delimiter = ',')]
        reject: Vec<usize>,
        /// Approve every pending entry
        #[arg(long)]
        approve_all: bool,
    },
    /// List active experience entries (JSON)
    List {
        /// Restrict to one format id
        #[arg(long)]
        format: Option<String>,
    },
    /// Print the built-in default experience for a format (TOML)
    Defaults {
        /// Format id (see `attx formats`)
        #[arg(long)]
        format: String,
    },
    /// Drop entries by field name
    Forget {
        /// Field name as shown by `attx learn list`
        #[arg(long)]
        field: String,
        /// Restrict to one format id
        #[arg(long)]
        format: Option<String>,
    },
}

/// Workspace glossary: one agreed translation per proper noun.
#[derive(Subcommand, Debug)]
enum GlossaryCommands {
    /// Mine high-frequency proper nouns and have the model name them
    Build {
        #[arg(long)]
        workspace: PathBuf,
        /// Override [glossary].min_occurrences for this run
        #[arg(long)]
        min_occurrences: Option<usize>,
        /// Report the candidates that would be sent, without calling the model
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the glossary (JSON)
    List {
        #[arg(long)]
        workspace: PathBuf,
        /// Include terms the model rejected as non-terms
        #[arg(long)]
        all: bool,
    },
    /// Add or overwrite one term
    Add {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        src: String,
        #[arg(long)]
        dst: String,
        /// Disambiguating description, e.g. "女性名字" / "地点"
        #[arg(long, default_value = "")]
        info: String,
    },
    /// Remove one term
    Remove {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        src: String,
    },
    /// Import terms from JSON ([{src,dst,info}] or {src: dst})
    Import {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        file: PathBuf,
    },
    /// Export active terms as JSON
    Export {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        file: PathBuf,
    },
    /// Report glossary terms the translation did not actually use
    Check {
        #[arg(long)]
        workspace: PathBuf,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut settings = config::load(cli.config.as_deref())?;
    if let Some(name) = &cli.client {
        // Global override: all downstream `settings.client(None)` lookups hit it.
        settings.llm.default_client = name.clone();
    }

    match cli.command {
        Commands::Doctor { ping, json } => pipeline::doctor(&settings, ping, json),
        Commands::Formats => {
            println!("{}", serde_json::to_string_pretty(&pipeline::formats())?);
            Ok(())
        }
        Commands::Detect { input } => {
            let hit = pipeline::detect_any(&input)?;
            println!(
                "{}",
                serde_json::json!({
                    "engine": hit.engine,
                    "content_root": hit.content_root,
                    "label": hit.label,
                    "profile": hit.profile_path,
                })
            );
            Ok(())
        }
        Commands::Analyze { input, src } => {
            let report = pipeline::analyze(&input, &src)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Commands::Profile { command } => run_profile(command),
        Commands::Learn { command } => run_learn(command, &settings),
        Commands::Glossary { command } => run_glossary(command, &settings),
        Commands::Init {
            input,
            engine,
            profile,
            src,
            dst,
            workspace,
        } => {
            let ws = pipeline::init_workspace(
                &input,
                engine.as_deref(),
                profile.as_deref(),
                &src,
                &dst,
                workspace,
            )?;
            println!(
                "{}",
                serde_json::json!({
                    "workspace": ws,
                    "status": "ok",
                })
            );
            Ok(())
        }
        Commands::Extract {
            workspace,
            no_knowledge,
        } => {
            let rep = pipeline::extract(&workspace, &settings, !no_knowledge)?;
            println!("{}", serde_json::to_string(&rep)?);
            Ok(())
        }
        Commands::Translate {
            workspace,
            limit,
            dry_run,
            retry_passthrough,
        } => {
            let r = pipeline::translate(&workspace, &settings, limit, dry_run, retry_passthrough)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Commands::Writeback {
            workspace,
            dry_run,
            no_learn,
        } => {
            let r = pipeline::writeback(&workspace, &settings, dry_run, !no_learn)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Commands::Run {
            input,
            engine,
            profile,
            src,
            dst,
            workspace,
            limit,
            no_translate,
            no_writeback,
            glossary: force_glossary,
            no_glossary,
        } => {
            let ws = pipeline::init_workspace(
                &input,
                engine.as_deref(),
                profile.as_deref(),
                &src,
                &dst,
                workspace,
            )?;
            let extracted = pipeline::extract(&ws, &settings, true)?.extracted;
            let mut out = serde_json::json!({
                "workspace": ws,
                "extracted": extracted,
            });
            // The glossary costs extra model calls, so it only runs when the
            // config opted in or this invocation asked for it explicitly.
            let want_glossary =
                !no_glossary && !no_translate && (force_glossary || settings.glossary.enabled);
            if want_glossary {
                match glossary::build(&ws, &settings, None, false) {
                    Ok(r) => out["glossary"] = serde_json::to_value(r)?,
                    Err(e) => {
                        eprintln!("glossary: build skipped ({e:#})");
                        out["glossary"] = serde_json::json!({"error": format!("{e:#}")});
                    }
                }
            }
            if !no_translate {
                let tr = pipeline::translate(&ws, &settings, limit, false, false)?;
                out["translate"] = serde_json::to_value(tr)?;
            }
            if !no_writeback && !no_translate {
                let wb = pipeline::writeback(&ws, &settings, false, true)?;
                out["writeback"] = serde_json::to_value(wb)?;
            }
            out["status"] = serde_json::json!("ok");
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        Commands::Status { workspace } => {
            let s = pipeline::status(&workspace)?;
            println!("{}", serde_json::to_string_pretty(&s)?);
            Ok(())
        }
        Commands::TranslateJsonl {
            input,
            output,
            src,
            dst,
            limit,
        } => {
            let r = pipeline::translate_jsonl(&input, &output, &settings, &src, &dst, limit)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Commands::ExportJsonl {
            workspace,
            output,
            filter,
        } => {
            let n = pipeline::export_jsonl(&workspace, &output, &filter)?;
            println!("{}", serde_json::json!({"exported": n, "output": output}));
            Ok(())
        }
        Commands::ImportJsonl { workspace, input } => {
            let n = pipeline::import_jsonl(&workspace, &input)?;
            println!("{}", serde_json::json!({"imported": n, "status": "ok"}));
            Ok(())
        }
    }
}

fn run_profile(command: ProfileCommands) -> Result<()> {
    match command {
        ProfileCommands::New { output, name } => {
            if output.exists() {
                anyhow::bail!("{} already exists", output.display());
            }
            std::fs::write(&output, profile::template(&name))?;
            println!(
                "{}",
                serde_json::json!({
                    "written": output,
                    "next": "edit the rules, then: attx profile test --profile <file> --input <file>",
                    "status": "ok",
                })
            );
            Ok(())
        }
        ProfileCommands::Test {
            profile: profile_arg,
            input,
            src,
            limit,
            roundtrip,
        } => {
            let path = resolve_profile_path(&profile_arg)?;
            let report = pipeline::profile_test(&path, &input, &src, limit, roundtrip)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        ProfileCommands::Save {
            profile: profile_path,
            force,
        } => {
            let dest = profile::save(&profile_path, force)?;
            println!(
                "{}",
                serde_json::json!({
                    "saved": dest,
                    "status": "ok",
                })
            );
            Ok(())
        }
        ProfileCommands::List => {
            println!(
                "{}",
                serde_json::to_string_pretty(&pipeline::profile_list())?
            );
            Ok(())
        }
    }
}

fn resolve_profile_path(arg: &str) -> Result<PathBuf> {
    let p = PathBuf::from(arg);
    if p.is_file() {
        return Ok(p);
    }
    let (path, _) = profile::find_saved(arg)?;
    Ok(path)
}

/// Glossary commands. `build` needs an LLM; everything else is local.
fn run_glossary(command: GlossaryCommands, settings: &config::Settings) -> Result<()> {
    match command {
        GlossaryCommands::Build {
            workspace,
            min_occurrences,
            dry_run,
        } => {
            let report = glossary::build(&workspace, settings, min_occurrences, dry_run)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.dry_run {
                eprintln!(
                    "glossary: dry run — {} candidate(s) at or above {} occurrence(s) would be named",
                    report.asked, report.min_occurrences
                );
            }
            Ok(())
        }
        GlossaryCommands::List { workspace, all } => {
            let g = glossary::load(&workspace);
            let terms: Vec<_> = g
                .terms
                .iter()
                .filter(|t| all || t.status == glossary::TermStatus::Active)
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "source_lang": g.source_lang,
                    "target_lang": g.target_lang,
                    "count": terms.len(),
                    "terms": terms,
                }))?
            );
            Ok(())
        }
        GlossaryCommands::Add {
            workspace,
            src,
            dst,
            info,
        } => {
            let mut g = glossary::load(&workspace);
            g.upsert(glossary::GlossaryTerm {
                src: src.clone(),
                dst,
                info,
                count: 0,
                status: glossary::TermStatus::Active,
                source: "human".into(),
            });
            glossary::ensure_langs(&workspace, &mut g);
            let p = glossary::save(&workspace, &g)?;
            println!(
                "{}",
                serde_json::json!({"added": src, "file": p.display().to_string(), "status": "ok"})
            );
            Ok(())
        }
        GlossaryCommands::Remove { workspace, src } => {
            let mut g = glossary::load(&workspace);
            let removed = g.remove(&src);
            glossary::save(&workspace, &g)?;
            println!(
                "{}",
                serde_json::json!({"removed": removed, "src": src, "status": "ok"})
            );
            Ok(())
        }
        GlossaryCommands::Import { workspace, file } => {
            let n = glossary::import_json(&workspace, &file)?;
            println!("{}", serde_json::json!({"imported": n, "status": "ok"}));
            Ok(())
        }
        GlossaryCommands::Export { workspace, file } => {
            let n = glossary::export_json(&workspace, &file)?;
            println!(
                "{}",
                serde_json::json!({"exported": n, "file": file, "status": "ok"})
            );
            Ok(())
        }
        GlossaryCommands::Check { workspace } => {
            if glossary::load(&workspace).is_empty() {
                anyhow::bail!(
                    "no glossary in {}; build one with `attx glossary build --workspace {}` \
                     or import one with `attx glossary import`",
                    workspace.display(),
                    workspace.display()
                );
            }
            let report = glossary::check(&workspace)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.violations.is_empty() {
                eprintln!(
                    "glossary: {} term(s) were not applied everywhere; \
                     fix the term with `attx glossary add` or re-translate those units",
                    report.violations.len()
                );
            }
            Ok(())
        }
    }
}

fn run_learn(command: LearnCommands, settings: &config::Settings) -> Result<()> {
    match command {
        LearnCommands::Summarize { workspace, llm } => {
            let report = learn::summarize(&workspace, llm, settings)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.pending > 0 {
                eprintln!(
                    "learn: {} entr(ies) need approval before they take effect. \
                     Review with `attx learn pending` then `attx learn review --approve <n>`",
                    report.pending
                );
            }
            Ok(())
        }
        LearnCommands::Pending => {
            let items = learn::pending_items();
            // 1-based indices here are the same ones `review --approve` takes.
            let listed: Vec<_> = items
                .iter()
                .map(|p| {
                    let mut v = p.entry.to_json();
                    v["index"] = serde_json::json!(p.index);
                    v["format"] = serde_json::json!(p.format);
                    v
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "pending": listed.len(),
                    "entries": listed,
                }))?
            );
            Ok(())
        }
        LearnCommands::Review {
            approve,
            reject,
            approve_all,
        } => {
            let approve = if approve_all {
                learn::pending_items().iter().map(|p| p.index).collect()
            } else {
                approve
            };
            if approve.is_empty() && reject.is_empty() {
                anyhow::bail!(
                    "nothing to do: pass --approve <n[,n]>, --reject <n[,n]> or --approve-all \
                     (see `attx learn pending`)"
                );
            }
            let report = learn::review(&approve, &reject)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        LearnCommands::List { format } => {
            let files = knowledge::all_files();
            let out: Vec<_> = files
                .iter()
                .filter(|f| format.as_ref().is_none_or(|want| &f.format == want))
                .map(|f| {
                    // Kind counts first: an agent scanning this wants to know
                    // what vocabulary is in the file before reading entries.
                    let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
                    for e in &f.entries {
                        *kinds.entry(e.kind()).or_insert(0) += 1;
                    }
                    serde_json::json!({
                        "format": f.format,
                        "version": f.version,
                        "kinds": kinds,
                        "entries": f.entries.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "formats": out.len(),
                    "knowledge": out,
                    "note": "built-in defaults are not listed here; see `attx learn defaults --format <id>`",
                }))?
            );
            Ok(())
        }
        LearnCommands::Defaults { format } => match knowledge::embedded_defaults(&format) {
            Some(raw) => {
                print!("{raw}");
                Ok(())
            }
            None => anyhow::bail!(
                "no built-in defaults for `{format}`. Formats with defaults: {}",
                knowledge::embedded_formats().join(", ")
            ),
        },
        LearnCommands::Forget { field, format } => {
            let n = learn::forget(&field, format.as_deref())?;
            println!(
                "{}",
                serde_json::json!({"forgotten": n, "field": field, "status": "ok"})
            );
            Ok(())
        }
    }
}
