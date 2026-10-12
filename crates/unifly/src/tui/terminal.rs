//! Terminal initialization, restoration, and panic-safe cleanup.
//!
//! Wraps the crossterm + ratatui terminal lifecycle so the rest of the app
//! never has to think about raw mode or alternate screen.

use std::io::{Stdout, Write, stdout};

use color_eyre::eyre::Result;
use crossterm::{
    ExecutableCommand, QueueableCommand, cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{
        self, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type Backend = CrosstermBackend<Stdout>;

/// Terminal wrapper that handles setup, teardown, and panic recovery.
pub struct Tui {
    pub terminal: Terminal<Backend>,
}

impl Tui {
    /// Create a new terminal instance (does NOT enter raw mode yet).
    pub fn new() -> Result<Self> {
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Enter TUI mode: alternate screen, raw mode, mouse capture, hidden cursor.
    pub fn enter(&mut self) -> Result<()> {
        terminal::enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        stdout().execute(EnableMouseCapture)?;
        stdout().execute(cursor::Hide)?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Exit TUI mode: restore terminal to its original state.
    #[allow(clippy::unused_self)]
    pub fn exit(&mut self) {
        // Kitty keeps images per screen, so free the chart images while the
        // alternate screen they live on is still active.
        #[cfg(feature = "tui-graphics")]
        if let Some(deletes) = crate::tui::graphics::take_all_image_deletes() {
            let mut out = stdout();
            let _ = out.write_all(deletes.as_bytes());
            let _ = out.flush();
        }

        // Best-effort restoration — don't bail on partial failures
        let _ = stdout().execute(cursor::Show);
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }

    /// Draw a frame using the provided render closure.
    pub fn draw<F>(&mut self, render: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        let sync = sync_updates_enabled();
        if sync {
            self.terminal.backend_mut().queue(BeginSynchronizedUpdate)?;
        }

        let result = self.terminal.draw(render).map(|_| ());

        // Chart images the cache dropped before this frame cannot appear in
        // it, so the frame has replaced their cells; free them in the same
        // update so the terminal's image store does not fill up.
        #[cfg(feature = "tui-graphics")]
        if result.is_ok()
            && let Some(deletes) = crate::tui::graphics::take_retired_image_deletes()
        {
            self.terminal.backend_mut().write_all(deletes.as_bytes())?;
            if !sync {
                self.terminal.backend_mut().flush()?;
            }
        }

        if sync {
            self.terminal.backend_mut().queue(EndSynchronizedUpdate)?;
            self.terminal.backend_mut().flush()?;
        }

        result?;
        Ok(())
    }

    /// Get terminal size as (width, height).
    pub fn size(&self) -> Result<(u16, u16)> {
        let size = self.terminal.size()?;
        Ok((size.width, size.height))
    }
}

fn sync_updates_enabled() -> bool {
    sync_updates_enabled_with(|key| std::env::var_os(key).is_some())
}

fn sync_updates_enabled_with<F>(env_contains: F) -> bool
where
    F: Fn(&str) -> bool,
{
    !env_contains("UNIFLY_NO_SYNC")
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.exit();
    }
}

/// Install panic and error hooks that restore the terminal before printing.
///
/// Must be called BEFORE entering the terminal, so panics during init
/// also get clean output.
pub fn install_hooks() -> Result<()> {
    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .into_hooks();

    // color-eyre error report hook
    eyre_hook.install()?;

    // Panic hook: restore terminal, then print the panic
    let panic_hook = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restoration
        let _ = stdout().execute(cursor::Show);
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();

        // Now print the panic with full context
        panic_hook(info);
    }));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sync_updates_enabled_with;

    #[test]
    fn sync_updates_default_on() {
        assert!(sync_updates_enabled_with(|_| false));
    }

    #[test]
    fn sync_updates_can_be_disabled() {
        assert!(!sync_updates_enabled_with(|key| key == "UNIFLY_NO_SYNC"));
    }
}
