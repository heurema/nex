mod commands;
mod core;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nex", version, about = "Cross-CLI plugin distribution for AI agents")]
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
    /// Check plugin health and detect drift
    Doctor {
        /// Re-verify SHA256 hashes (slow)
        #[arg(long)]
        deep: bool,
    },
    /// Search plugins in registry
    Search {
        /// Search query (matches name and description)
        query: Option<String>,
        /// Filter by category
        #[arg(long)]
        category: Option<String>,
    },
    /// Show detailed plugin information
    Info {
        /// Plugin name
        name: String,
    },
    /// Cross-platform plugin health view
    Status,
    /// Manage nex profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Automated plugin release pipeline (dry-run by default)
    Release {
        /// Bump level: major, minor, or patch (default: patch)
        #[arg(default_value = "patch")]
        level: String,
        /// Actually perform the release (default: dry-run)
        #[arg(long)]
        execute: bool,
        /// Explicit version (overrides LEVEL)
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// Override marketplace from config
        #[arg(long, value_name = "NAME")]
        marketplace: Option<String>,
        /// Override tag format (e.g. '{version}-custom')
        #[arg(long, value_name = "FMT")]
        tag_format: Option<String>,
        /// Skip marketplace propagation step
        #[arg(long)]
        no_propagate: bool,
        /// Skip changelog step
        #[arg(long)]
        no_changelog: bool,
        /// Plugin directory (default: current directory)
        #[arg(long, value_name = "DIR")]
        path: Option<String>,
        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List all profiles
    List,
    /// Show profile details
    Show { name: String },
    /// Apply profile (create/remove Codex/Gemini symlinks)
    Apply { name: String },
    /// Set active profile without applying
    Activate { name: String },
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
    /// Register a nex marketplace category in Claude Code
    Add {
        /// Category name (devtools, trading, creative) or --all
        category: Option<String>,
        /// Register all categories
        #[arg(long)]
        all: bool,
    },
    /// List registered nex marketplaces
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
        Commands::Status => {
            commands::status::run()
        }
        Commands::Profile { action } => match action {
            ProfileAction::List => commands::profile::run_list(),
            ProfileAction::Show { name } => commands::profile::run_show(&name),
            ProfileAction::Apply { name } => commands::profile::run_apply(&name),
            ProfileAction::Activate { name } => commands::profile::run_activate(&name),
        },
        Commands::Doctor { deep } => {
            commands::doctor::run(deep)
        }
        Commands::Search { query, category } => {
            commands::search::run(query.as_deref(), category.as_deref())
        }
        Commands::Info { name } => {
            commands::info::run(&name)
        }
        Commands::Release { level, execute, version, marketplace, tag_format, no_propagate, no_changelog, path, verbose } => {
            commands::release::run(
                &level,
                execute,
                version.as_deref(),
                marketplace.as_deref(),
                tag_format.as_deref(),
                no_propagate,
                no_changelog,
                path.as_deref(),
                verbose,
            )
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
