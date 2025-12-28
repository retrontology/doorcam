use anyhow::Result;
use clap::Parser;
use tracing::{error, info};

use doorcam::{DoorcamConfig, DoorcamOrchestrator};

#[derive(Parser, Debug)]
#[command(name = "doorcam")]
#[command(about = "Rust-based door camera system with motion detection and streaming")]
#[command(version)]
#[command(
    long_about = "A Rust-based door camera system that provides motion detection, \
video capture, live streaming, and display functionality for door monitoring applications. \
Supports hardware acceleration on Raspberry Pi and integrates with systemd for service management."
)]
struct Args {
    /// Path to configuration file
    #[arg(
        short,
        long,
        default_value = "doorcam.toml",
        help = "Path to TOML configuration file"
    )]
    config: String,

    /// Enable debug logging (most verbose)
    #[arg(short, long, help = "Enable debug level logging")]
    debug: bool,

    /// Enable verbose logging (info level)
    #[arg(short, long, help = "Enable verbose info level logging")]
    verbose: bool,

    /// Enable quiet mode (errors only)
    #[arg(short, long, help = "Enable quiet mode - only log errors")]
    quiet: bool,

    /// Validate configuration and exit
    #[arg(
        long,
        help = "Validate configuration file and exit without starting the system"
    )]
    validate_config: bool,

    /// Print default configuration and exit
    #[arg(long, help = "Print default configuration in TOML format and exit")]
    print_config: bool,

    /// Dry run mode - initialize but don't start components
    #[arg(
        long,
        help = "Perform dry run - initialize components but don't start them"
    )]
    dry_run: bool,

    /// Override log format (json, pretty, compact)
    #[arg(
        long,
        value_name = "FORMAT",
        help = "Log output format: json, pretty, or compact"
    )]
    log_format: Option<String>,

    /// Enable systemd journal integration
    #[arg(long, help = "Enable systemd journal integration for logging")]
    systemd: bool,

    /// Enable keyboard input for debugging (SPACE=motion, Q/ESC=quit)
    #[arg(
        long,
        help = "Enable keyboard input handler for debugging motion events"
    )]
    enable_keyboard: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle special modes that don't require full initialization
    if args.print_config {
        print_default_config();
        return Ok(());
    }

    // Initialize logging
    init_logging(&args)?;

    info!("Starting Doorcam system v{}", env!("CARGO_PKG_VERSION"));
    info!("Configuration file: {}", args.config);

    // Load and validate configuration
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

    // Validate configuration if requested
    if args.validate_config {
        match config.validate() {
            Ok(()) => {
                info!("Configuration validation successful");
                println!("✓ Configuration is valid");
                return Ok(());
            }
            Err(e) => {
                error!("Configuration validation failed: {}", e);
                eprintln!("✗ Configuration validation failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    info!("Doorcam configuration loaded and validated");

    // Create and initialize the orchestrator
    let mut orchestrator = DoorcamOrchestrator::new(config).await.map_err(|e| {
        error!("Failed to create orchestrator: {}", e);
        e
    })?;

    // Enable keyboard handler if requested
    if args.enable_keyboard {
        orchestrator.set_keyboard_enabled(true);
        info!("Keyboard input handler enabled");
    }

    // Initialize all components
    orchestrator.initialize().await.map_err(|e| {
        error!("Failed to initialize system: {}", e);
        e
    })?;

    // Handle dry run mode
    if args.dry_run {
        info!("Dry run mode - components initialized but not started");
        println!("✓ Dry run completed successfully - all components initialized");
        return Ok(());
    }

    // Start all components
    orchestrator.start().await.map_err(|e| {
        error!("Failed to start system: {}", e);
        e
    })?;

    // Show keyboard controls if enabled
    if args.enable_keyboard {
        info!("=== Doorcam Debug Controls ===");
        info!("Press SPACE to trigger a motion event");
        info!("Press Q or ESC to shutdown gracefully");
        info!("==============================");
    }

    // Run the main application loop with signal handling
    let exit_code = orchestrator.run().await.map_err(|e| {
        error!("System error during execution: {}", e);
        e
    })?;

    info!("Doorcam system exited with code: {}", exit_code);

    // Exit with appropriate code for systemd
    std::process::exit(exit_code);
}

fn init_logging(args: &Args) -> Result<()> {
    use tracing_subscriber::{
        fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer,
    };

    // Determine log level based on flags
    let log_level = if args.debug {
        "debug"
    } else if args.verbose {
        "info"
    } else if args.quiet {
        "error"
    } else {
        "warn"
    };

    // Create environment filter
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("doorcam={}", log_level)));

    // Configure format based on options
    let fmt_layer = match args.log_format.as_deref() {
        Some("json") => fmt::layer()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .boxed(),
        Some("compact") => fmt::layer()
            .compact()
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .boxed(),
        Some("pretty") | None => fmt::layer()
            .pretty()
            .with_target(true)
            .with_thread_ids(args.debug)
            .with_file(args.debug)
            .with_line_number(args.debug)
            .boxed(),
        Some(format) => {
            eprintln!("Warning: Unknown log format '{}', using default", format);
            fmt::layer()
                .with_target(true)
                .with_thread_ids(args.debug)
                .with_file(args.debug)
                .with_line_number(args.debug)
                .boxed()
        }
    };

    // Initialize subscriber
    let subscriber = tracing_subscriber::registry()
        .with(fmt_layer)
        .with(env_filter);

    // Add systemd journal support if requested (placeholder for now)
    if args.systemd {
        // TODO: Add systemd journal integration when available
        eprintln!("Note: Systemd journal integration not yet implemented");
    }

    subscriber.init();

    Ok(())
}

/// Print default configuration in TOML format
fn print_default_config() {
    println!("# Doorcam Configuration File");
    println!("# These are the built-in defaults; override in TOML or env vars");
    println!();

    let default_config = DoorcamConfig::default();
    match toml::to_string_pretty(&default_config) {
        Ok(toml_str) => println!("{}", toml_str),
        Err(e) => eprintln!("Failed to render default config: {}", e),
    }
}
