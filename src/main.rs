use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lang2lang::{ConvertOptions, Language, convert_project};

#[derive(Debug, Parser)]
#[command(
    name = "lang2lang",
    version,
    about = "Translate Python, JavaScript, and Lua through a shared semantic IR"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Convert an entry module and all locally resolvable dependencies.
    Convert {
        entry: PathBuf,
        /// Root used for module IDs and imports; defaults to the entry's directory.
        #[arg(long)]
        project_root: Option<PathBuf>,
        #[arg(long, short = 't')]
        to: Language,
        #[arg(long, short = 'o')]
        out: PathBuf,
        /// Keep imports whose source is not available locally.
        #[arg(long)]
        allow_external: bool,
    },
    /// Parse an entry module and its local dependencies, then print the shared IR.
    Ir {
        entry: PathBuf,
        /// Root used to resolve and name local modules.
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Permit unresolved external imports in the IR.
        #[arg(long)]
        allow_external: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert {
            entry,
            project_root,
            to,
            out,
            allow_external,
        } => {
            let conversion = convert_project(&ConvertOptions {
                entry,
                project_root,
                target: to,
                allow_external,
            })?;
            conversion.write(&out)?;
            eprintln!(
                "converted {} module(s) to {} in {}",
                conversion.modules.len(),
                to,
                out.display()
            );
        }
        Command::Ir {
            entry,
            project_root,
            allow_external,
        } => {
            let conversion = convert_project(&ConvertOptions {
                entry,
                project_root,
                target: Language::JavaScript,
                allow_external,
            })?;
            println!(
                "{}",
                serde_json::to_string_pretty(&conversion.modules)
                    .context("could not serialize IR")?
            );
        }
    }
    Ok(())
}
