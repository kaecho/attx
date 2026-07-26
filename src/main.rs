mod adapter;
mod config;
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
        Commands::Extract { workspace } => {
            let n = pipeline::extract(&workspace, &settings)?;
            println!("{}", serde_json::json!({"extracted": n, "status": "ok"}));
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
        Commands::Writeback { workspace, dry_run } => {
            let r = pipeline::writeback(&workspace, &settings, dry_run)?;
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
        } => {
            let ws = pipeline::init_workspace(
                &input,
                engine.as_deref(),
                profile.as_deref(),
                &src,
                &dst,
                workspace,
            )?;
            let extracted = pipeline::extract(&ws, &settings)?;
            let mut out = serde_json::json!({
                "workspace": ws,
                "extracted": extracted,
            });
            if !no_translate {
                let tr = pipeline::translate(&ws, &settings, limit, false, false)?;
                out["translate"] = serde_json::to_value(tr)?;
            }
            if !no_writeback && !no_translate {
                let wb = pipeline::writeback(&ws, &settings, false)?;
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
