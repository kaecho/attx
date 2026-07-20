mod adapter;
mod config;
mod llm;
mod model;
mod pipeline;
mod quality;
mod store;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "attx",
    version,
    about = "Agent Translation Toolkit eXtensible — universal game text translation framework"
)]
struct Cli {
    /// Path to setting.toml (default: ./setting.toml or $ATTX_HOME/setting.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

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
    },
    /// Detect game engine for a directory
    Detect {
        #[arg(long)]
        game: PathBuf,
    },
    /// Register / open a workspace for a game
    Init {
        #[arg(long)]
        game: PathBuf,
        /// Force engine id (rmmz | jsonl). Auto-detect when omitted.
        #[arg(long)]
        engine: Option<String>,
        /// Source language: ja | en
        #[arg(long, default_value = "ja")]
        src: String,
        /// Target language
        #[arg(long, default_value = "zh")]
        dst: String,
        /// Workspace dir (default: <game>/.attx)
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Extract texts from game into workspace DB
    Extract {
        #[arg(long)]
        workspace: PathBuf,
    },
    /// Translate pending texts via LLM
    Translate {
        #[arg(long)]
        workspace: PathBuf,
        /// Limit number of units (debug)
        #[arg(long)]
        limit: Option<usize>,
        /// Dry-run: print batch plan only
        #[arg(long)]
        dry_run: bool,
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
        #[arg(long)]
        game: PathBuf,
        #[arg(long)]
        engine: Option<String>,
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
        /// pending | all | translated
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

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let settings = config::load(cli.config.as_deref())?;

    match cli.command {
        Commands::Doctor { ping } => pipeline::doctor(&settings, ping),
        Commands::Detect { game } => {
            let hit = adapter::detect(&game)?;
            println!(
                "{}",
                serde_json::json!({
                    "engine": hit.engine_id,
                    "content_root": hit.content_root,
                    "label": hit.label,
                })
            );
            Ok(())
        }
        Commands::Init {
            game,
            engine,
            src,
            dst,
            workspace,
        } => {
            let ws = pipeline::init_workspace(&game, engine.as_deref(), &src, &dst, workspace)?;
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
        } => {
            let r = pipeline::translate(&workspace, &settings, limit, dry_run)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Commands::Writeback { workspace, dry_run } => {
            let r = pipeline::writeback(&workspace, &settings, dry_run)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Commands::Run {
            game,
            engine,
            src,
            dst,
            workspace,
            limit,
            no_translate,
            no_writeback,
        } => {
            let ws = pipeline::init_workspace(&game, engine.as_deref(), &src, &dst, workspace)?;
            let extracted = pipeline::extract(&ws, &settings)?;
            let mut out = serde_json::json!({
                "workspace": ws,
                "extracted": extracted,
            });
            if !no_translate {
                let tr = pipeline::translate(&ws, &settings, limit, false)?;
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
