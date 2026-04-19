use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::{Write, stdout};

/// RAII guard that puts the terminal into game mode on construction and
/// restores it on drop. Also installs a panic hook that performs the same
/// restore so a panic doesn't leave the user stuck in raw mode.
pub struct TerminalGuard {
    _private: (),
}

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        install_panic_hook();
        enable_raw_mode().context("enable raw mode")?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture)
            .context("enter alt screen / enable mouse")?;
        out.flush().ok();
        Ok(Self { _private: () })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    let mut out = stdout();
    let _ = execute!(out, DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = out.flush();
}

fn install_panic_hook() {
    use std::sync::Once;
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            prev(info);
        }));
    });
}
