use anyhow::Result;
use clap::Parser;
use tracing::{info, error};

mod config;
mod error;

use config::DoorcamConfig;
use error::DoorcamError;

#[derive(Parser, Debug)]
#[command(name = "doorcam")]
#[command(about = "Rust-based door camera system")]
#[command(version)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "doorcam.toml")]
    config: String,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.debug, args.verbose)?;

    info!("Starting Doorcam system");

    // Load configuration
    let config = match DoorcamConfig::load_from_file(&args.config) {
        Ok(config) => {
            info!("Configuration loaded successfully from: {}", args.config);
            config
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e.into());
        }
    };

    info!("Doorcam configuration: {:?}", config);

    // TODO: Initialize and start system components
    info!("System initialization complete");

    // Keep the application running
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, stopping Doorcam system");

    Ok(())
}

fn init_logging(debug: bool, verbose: bool) -> Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let log_level = if debug {
        "debug"
    } else if verbose {
        "info"
    } else {
        "warn"
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("doorcam={}", log_level)));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true)
        )
        .with(env_filter)
        .init();

    Ok(())
}