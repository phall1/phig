use std::{
    io::{self, Stdout, Write},
    panic,
    sync::{
        Once,
        atomic::{AtomicBool, Ordering},
    },
};

use crossterm::{
    cursor::{Hide, MoveTo, MoveToNextLine, Show},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

static RAW_ACTIVE: AtomicBool = AtomicBool::new(false);
static ALT_ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();

pub type PhigTerminal = Terminal<CrosstermBackend<Stdout>>;

pub struct TerminalSession {
    terminal: PhigTerminal,
    raw_active: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
    normal_screen_advanced: bool,
}

impl TerminalSession {
    pub fn enter(no_alt_screen: bool) -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        RAW_ACTIVE.store(true, Ordering::SeqCst);

        let mut stdout = io::stdout();
        let mut alternate_screen = false;
        let mut cursor_hidden = false;

        if !no_alt_screen {
            // Mark ownership before writing so rollback also covers a partial
            // terminal escape that returned an I/O error.
            alternate_screen = true;
            ALT_ACTIVE.store(true, Ordering::SeqCst);
            if let Err(error) = execute!(stdout, EnterAlternateScreen) {
                rollback_setup(&mut stdout, alternate_screen, cursor_hidden);
                return Err(error);
            }
        } else if let Err(error) = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0)) {
            rollback_setup(&mut stdout, alternate_screen, cursor_hidden);
            return Err(error);
        }

        cursor_hidden = true;
        if let Err(error) = execute!(stdout, Hide) {
            rollback_setup(&mut stdout, alternate_screen, cursor_hidden);
            return Err(error);
        }

        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                emergency_restore();
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            raw_active: true,
            alternate_screen,
            cursor_hidden,
            normal_screen_advanced: false,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut PhigTerminal {
        &mut self.terminal
    }

    pub fn force_redraw(&mut self) -> io::Result<()> {
        self.terminal.clear()?;
        self.terminal.backend_mut().flush()
    }

    pub fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;

        if self.cursor_hidden {
            match execute!(self.terminal.backend_mut(), Show) {
                Ok(()) => self.cursor_hidden = false,
                Err(error) => first_error = Some(error),
            }
        }

        if self.alternate_screen {
            match execute!(self.terminal.backend_mut(), LeaveAlternateScreen) {
                Ok(()) => {
                    self.alternate_screen = false;
                    ALT_ACTIVE.store(false, Ordering::SeqCst);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        } else if !self.normal_screen_advanced {
            match execute!(self.terminal.backend_mut(), MoveToNextLine(1)) {
                Ok(()) => self.normal_screen_advanced = true,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }

        if self.raw_active {
            match disable_raw_mode() {
                Ok(()) => {
                    self.raw_active = false;
                    RAW_ACTIVE.store(false, Ordering::SeqCst);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }

        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.restore().is_err() {
            // Last-chance process-global cleanup; failures here cannot be
            // reported but must not prevent attempts for remaining modes.
            emergency_restore();
        }
    }
}

fn rollback_setup(stdout: &mut Stdout, alternate_screen: bool, cursor_hidden: bool) {
    if cursor_hidden {
        let _ = execute!(stdout, Show);
    }
    if alternate_screen && execute!(stdout, LeaveAlternateScreen).is_ok() {
        ALT_ACTIVE.store(false, Ordering::SeqCst);
    }
    if disable_raw_mode().is_ok() {
        RAW_ACTIVE.store(false, Ordering::SeqCst);
    }
    let _ = stdout.flush();
    if ALT_ACTIVE.load(Ordering::SeqCst) || RAW_ACTIVE.load(Ordering::SeqCst) {
        emergency_restore();
    }
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            emergency_restore();
            previous(info);
        }));
    });
}

fn emergency_restore() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, Show);
    if ALT_ACTIVE.load(Ordering::SeqCst) && execute!(stdout, LeaveAlternateScreen).is_ok() {
        ALT_ACTIVE.store(false, Ordering::SeqCst);
    }
    if RAW_ACTIVE.load(Ordering::SeqCst) && disable_raw_mode().is_ok() {
        RAW_ACTIVE.store(false, Ordering::SeqCst);
    }
    let _ = stdout.flush();
}
