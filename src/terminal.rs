use anyhow::{Context, Result};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use std::io::{Write, stdout};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether we successfully pushed keyboard enhancement flags and therefore
/// need to pop them on restore. Used by both Drop and the panic hook.
static KEYBOARD_FLAGS_PUSHED: AtomicBool = AtomicBool::new(false);

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
        // Opt into the kitty keyboard protocol where supported so we get real
        // Press/Release/Repeat events on Unix. Without this, terminals only
        // emit press events and held-key tracking has to lean on a decay
        // timer plus OS auto-repeat — which leaves a dead zone between them.
        if supports_keyboard_enhancement().unwrap_or(false) {
            if execute!(
                out,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
                )
            )
            .is_ok()
            {
                KEYBOARD_FLAGS_PUSHED.store(true, Ordering::SeqCst);
            }
        }
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
    if KEYBOARD_FLAGS_PUSHED.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
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
