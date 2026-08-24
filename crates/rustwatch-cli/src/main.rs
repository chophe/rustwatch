mod commands;
mod tui;

use clap::{Parser, Subcommand};
use rustwatch_core::paths::load_or_create_config;
use rustwatch_core::DataPaths;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "rustwatch", about = "Local activity memory (Pieces-lite)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Install,
    Start,
    Stop,
    Status,
    Permissions,
    Tail {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Screenshot {
        #[arg(long)]
        window: bool,
        #[arg(long)]
        screen: bool,
    },
    Export {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    Analyze {
        #[arg(long)]
        today: bool,
    },
    Chart {
        #[arg(long)]
        date: Option<String>,
        #[arg(long, default_value = "terminal")]
        format: String,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
    Tui,
}

#[derive(Subcommand)]
enum MemoryCommands {
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    Ingest {
        #[arg(long)]
        rebuild: bool,
    },
    Graph {
        #[arg(long)]
        around: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let indicatif_layer = IndicatifLayer::new();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(indicatif_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let paths = DataPaths::new(None)?;
    paths.ensure_dirs()?;
    let config = load_or_create_config(&paths.config)?;

    match cli.command {
        Commands::Install => commands::install(&paths)?,
        Commands::Start => commands::start(&paths).await?,
        Commands::Stop => commands::stop(&paths)?,
        Commands::Status => commands::status(&paths, &config).await?,
        Commands::Permissions => commands::permissions()?,
        Commands::Tail { limit } => commands::tail(&paths, limit).await?,
        Commands::Screenshot { window, screen } => {
            commands::screenshot(&paths, window, screen).await?
        }
        Commands::Export { from, to } => commands::export(&paths, from, to)?,
        Commands::Analyze { today: _ } => commands::analyze(&paths, &config).await?,
        Commands::Chart { date, format } => commands::chart(&paths, date, format)?,
        Commands::Memory { command } => match command {
            MemoryCommands::Search { query, limit } => {
                commands::memory_search(&paths, &config, &query, limit).await?
            }
            MemoryCommands::Ingest { rebuild } => {
                commands::memory_ingest(&paths, &config, rebuild).await?
            }
            MemoryCommands::Graph { around } => {
                commands::memory_graph(&paths, &config, &around).await?
            }
        },
        Commands::Tui => tui::run(&paths, &config).await?,
    }

    Ok(())
}
