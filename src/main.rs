mod commands;
mod core;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "skill7", version, about = "Cross-CLI plugin distribution for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install a plugin for detected CLIs
    Install {
        /// Plugin name from registry
        name: String,
        /// Install only for Claude Code
        #[arg(long)]
        claude_code: bool,
        /// Install only for Codex
        #[arg(long)]
        codex: bool,
        /// Install only for Gemini
        #[arg(long)]
        gemini: bool,
        /// Claude Code install scope
        #[arg(long, default_value = "user")]
        scope: String,
    },
    /// Remove a plugin from all platforms
    Uninstall {
        /// Plugin name
        name: String,
    },
    /// List installed plugins
    List,
    /// Check for available updates
    Check {
        /// Force registry refresh
        #[arg(long)]
        refresh: bool,
    },
    /// Update a plugin to latest version
    Update {
        /// Plugin name (or --all)
        name: Option<String>,
        /// Update all outdated plugins
        #[arg(long)]
        all: bool,
    },
    /// Manage Claude Code marketplaces
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Publish plugin: compute SHA-256 and generate registry entry
    Publish {
        /// Plugin name
        name: String,
        /// Git tag to use (defaults to HEAD tag or plugin.json version)
        #[arg(long)]
        tag: Option<String>,
    },
    /// Manage local development symlinks
    Dev {
        #[command(subcommand)]
        action: DevAction,
    },
    /// Scaffold a new plugin directory
    Init {
        /// Plugin name [a-z0-9-]+
        name: String,
    },
    /// Convert Claude Code plugin to universal format
    Convert,
}

#[derive(Subcommand)]
enum DevAction {
    /// Create a symlink for local plugin development
    Link {
        /// Path to local plugin directory
        path: String,
    },
    /// Remove a development symlink
    Unlink {
        /// Plugin name to unlink
        name: String,
    },
}

#[derive(Subcommand)]
enum MarketplaceAction {
    /// Register a skill7 marketplace category in Claude Code
    Add {
        /// Category name (devtools, trading, creative) or --all
        category: Option<String>,
        /// Register all categories
        #[arg(long)]
        all: bool,
    },
    /// List registered skill7 marketplaces
    List,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Install { name, claude_code, codex, gemini, scope } => {
            commands::install::run(&name, claude_code, codex, gemini, &scope)
        }
        Commands::Uninstall { name } => {
            commands::uninstall::run(&name)
        }
        Commands::List => {
            commands::list::run()
        }
        Commands::Check { refresh } => {
            commands::check::run(refresh)
        }
        Commands::Update { name, all } => {
            commands::update::run(name.as_deref(), all)
        }
        Commands::Marketplace { action } => match action {
            MarketplaceAction::Add { category, all } => {
                commands::marketplace::add(category.as_deref(), all)
            }
            MarketplaceAction::List => {
                commands::marketplace::list()
            }
        },
        Commands::Publish { name, tag } => {
            commands::publish::run(&name, tag.as_deref())
        }
        Commands::Dev { action } => match action {
            DevAction::Link { path } => {
                commands::dev::dev_link(&path)
            }
            DevAction::Unlink { name } => {
                commands::dev::dev_unlink(&name)
            }
        },
        Commands::Init { name } => {
            commands::init::run(&name)
        }
        Commands::Convert => {
            commands::convert::run()
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
