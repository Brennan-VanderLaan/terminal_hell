//! th_record — dev tool for capturing samples and CV envelopes from
//! an ES-8 (or any cpal-visible input) straight into Terminal Hell's
//! content tree. See `audio-spec.md §14`.
//!
//! Runs as a separate binary (not a `terminal_hell` subcommand) so a
//! GUI crash can't kill a live `bench --loop` session the user is
//! auditioning sounds against.

mod app;
mod capture;
mod io;
mod queue;
mod waveform;

use anyhow::Result;

fn main() -> Result<()> {
    // Logs to stderr — this is a dev tool, not the game.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Terminal Hell — Audio Recorder")
            .with_inner_size([1200.0, 820.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "th_record",
        native_options,
        Box::new(|cc| Ok(Box::new(app::RecorderApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe exited with error: {e}"))?;
    Ok(())
}
