use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "terminal_hell", version, about = "Every shooter you have ever loved is trying to kill you.")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Offline practice mode.
    Solo {
        /// Audition-pool behavior. `off` (default) plays only locked
        /// samples — what release players hear. `mix` plays both
        /// locked and audition-pool takes. `only` plays only audition.
        #[arg(long, default_value = "off")]
        audition: String,
    },
    /// Host a session; others can `connect` to your address:port.
    Serve {
        #[arg(long, default_value_t = 4646)]
        port: u16,
    },
    /// Connect to a host (accepts `ip:port` or just `ip` for the default port).
    Connect { addr: String },
    /// Run benchmark scenarios — scripted spawns, telemetry capture,
    /// no keyboard/network. Use `--scenario NAME` to pick one; omit
    /// to run the full catalogue. `--headless` skips rendering so CI
    /// can bench without a TTY; `--watch` (the default) renders at
    /// 60 fps for visual inspection. `--loop` + `--playlist` turn the
    /// benchmark into a screensaver soundstage for audio iteration.
    Bench {
        /// Scenario name (see `--scenario list` output on invalid
        /// name). Omit to run every scenario in the catalogue.
        #[arg(long)]
        scenario: Option<String>,
        /// Comma-separated list of scenario names. Overrides
        /// `--scenario`; cycles through them every loop iteration.
        /// Ignored without `--loop` (runs once then stops).
        #[arg(long)]
        playlist: Option<String>,
        /// Skip all rendering. Required on machines without a TTY.
        #[arg(long, default_value_t = false)]
        headless: bool,
        /// Start the bench with the spatial-grid debug overlay on.
        /// Watch mode only; F4 still toggles it at runtime.
        #[arg(long, default_value_t = false)]
        debug_grid: bool,
        /// Write JSON report to this path after the run completes.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Repeat the scenario (or playlist) until Ctrl+C. Also
        /// rescans the audio content tree between iterations so the
        /// recorder's hot-reload workflow lands new samples each pass.
        #[arg(long, alias = "loop", default_value_t = false)]
        looping: bool,
        /// Audition-pool behavior. See `solo --audition` help.
        #[arg(long, default_value = "off")]
        audition: String,
    },
    /// Audio utility subcommands (audit, list, probe).
    Audio {
        #[command(subcommand)]
        action: AudioAction,
    },
}

#[derive(Subcommand)]
enum AudioAction {
    /// Walk the content tree and report sample coverage — which
    /// events have locked takes, which have audition candidates,
    /// which are still needs-recording. Drives the recorder queue.
    Audit {
        /// Write the report to a markdown file in addition to stdout.
        #[arg(long)]
        write: Option<PathBuf>,
    },
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
        Command::Solo { audition } => {
            init_audio_or_warn(&audition)?;
            terminal_hell::run_solo()
        }
        Command::Serve { port } => {
            init_audio_or_warn("off")?;
            terminal_hell::net::run_serve(port)
        }
        Command::Connect { addr } => {
            init_audio_or_warn("off")?;
            terminal_hell::net::run_connect(addr)
        }
        Command::Bench { scenario, playlist, headless, debug_grid, output, looping, audition } => {
            let mode = if headless {
                terminal_hell::bench::RenderMode::Headless
            } else {
                terminal_hell::bench::RenderMode::Watch
            };
            init_audio_or_warn(&audition)?;
            terminal_hell::bench::run_bench(
                scenario,
                playlist,
                mode,
                debug_grid,
                output,
                looping,
            )
        }
        Command::Audio { action } => match action {
            AudioAction::Audit { write } => terminal_hell::audio::audit::run_cli(write),
        },
    }
}

/// Initialize the audio engine + scan the default content root.
/// Non-fatal: prints a warning and continues if audio init fails so
/// the game stays playable on machines with no output device.
fn init_audio_or_warn(audition: &str) -> Result<()> {
    let mode = match terminal_hell::audio::AuditionMode::parse(audition) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("audio: {err}; falling back to --audition=off");
            terminal_hell::audio::AuditionMode::Off
        }
    };
    if let Err(err) = terminal_hell::audio::ensure_init(mode) {
        eprintln!("audio: init failed: {err}");
        return Ok(());
    }
    let root = terminal_hell::audio::default_content_root();
    if let Err(err) = terminal_hell::audio::scan(&root) {
        eprintln!("audio: scan {} failed: {err}", root.display());
    }
    // Kick off the filesystem watcher so the recorder's save-to-disk
    // workflow lands in the engine's sample pools without a restart.
    if let Err(err) = terminal_hell::audio::watch::spawn_watcher(root) {
        eprintln!("audio: watcher failed to start: {err}");
    }
    Ok(())
}
