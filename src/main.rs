use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "terminal_hell", version, about = "Every shooter you have ever loved is trying to kill you.")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Offline practice mode
    Solo,
    /// Host a session; others can `connect` to your address:port.
    Serve {
        #[arg(long, default_value_t = 4646)]
        port: u16,
    },
    /// Connect to a host (accepts `ip:port` or just `ip` for the default port).
    Connect { addr: String },
}

fn main() -> Result<()> {
    // Logs go into an in-memory ring buffer instead of stderr; the TUI
    // render loop would otherwise clobber them. Viewed in-game via
    // backtick-toggled console overlay.
    let log_buf = terminal_hell::log_buf::LogBuffer::install(1024);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(log_buf)
        .with_ansi(false)
        .with_target(false)
        .without_time()
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Command::Solo => terminal_hell::run_solo(),
        Command::Serve { port } => terminal_hell::net::run_serve(port),
        Command::Connect { addr } => terminal_hell::net::run_connect(addr),
    }
}
