use std::{
    fs::OpenOptions,
    io::{self, Write},
    panic,
    sync::{
        Once,
        atomic::{AtomicBool, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use crossterm::{
    cursor::{Hide, MoveTo, MoveToNextLine, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
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

pub type PhigTerminal = Terminal<CrosstermBackend<Box<dyn Write>>>;

pub struct TerminalSession {
    terminal: PhigTerminal,
    raw_active: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
    normal_screen_advanced: bool,
    mouse_capture: bool,
}

impl TerminalSession {
    pub fn enter(no_alt_screen: bool) -> io::Result<Self> {
        Self::enter_with(Box::new(io::stdout()), no_alt_screen, false)
    }
    pub fn enter_configured(no_alt_screen: bool, mouse: bool) -> io::Result<Self> {
        Self::enter_with(Box::new(io::stdout()), no_alt_screen, mouse)
    }
    #[cfg(unix)]
    pub fn enter_controlling_tty(no_alt_screen: bool, mouse: bool) -> io::Result<Self> {
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot open controlling terminal /dev/tty: {error}"),
                )
            })?;
        Self::enter_with(Box::new(tty), no_alt_screen, mouse)
    }
    #[cfg(not(unix))]
    pub fn enter_controlling_tty(_no_alt_screen: bool, _mouse: bool) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "selection requires a controlling terminal",
        ))
    }
    fn enter_with(
        mut output: Box<dyn Write>,
        no_alt_screen: bool,
        mouse: bool,
    ) -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        RAW_ACTIVE.store(true, Ordering::SeqCst);
        let mut alternate_screen = false;
        let mut cursor_hidden = false;
        if !no_alt_screen {
            alternate_screen = true;
            ALT_ACTIVE.store(true, Ordering::SeqCst);
            if let Err(error) = execute!(output, EnterAlternateScreen) {
                rollback_setup(&mut output, alternate_screen, cursor_hidden);
                return Err(error);
            }
        } else if let Err(error) = execute!(output, Clear(ClearType::All), MoveTo(0, 0)) {
            rollback_setup(&mut output, alternate_screen, cursor_hidden);
            return Err(error);
        }
        cursor_hidden = true;
        if let Err(error) = execute!(output, Hide) {
            rollback_setup(&mut output, alternate_screen, cursor_hidden);
            return Err(error);
        }
        if mouse {
            if let Err(error) = execute!(output, EnableMouseCapture) {
                rollback_setup(&mut output, alternate_screen, cursor_hidden);
                return Err(error);
            }
        }
        let terminal = match Terminal::new(CrosstermBackend::new(output)) {
            Ok(v) => v,
            Err(e) => {
                emergency_restore();
                return Err(e);
            }
        };
        Ok(Self {
            terminal,
            raw_active: true,
            alternate_screen,
            cursor_hidden,
            normal_screen_advanced: false,
            mouse_capture: mouse,
        })
    }
    pub fn terminal_mut(&mut self) -> &mut PhigTerminal {
        &mut self.terminal
    }
    pub fn force_redraw(&mut self) -> io::Result<()> {
        self.terminal.clear()?;
        self.terminal.backend_mut().flush()
    }
    pub fn copy_osc52(&mut self, value: &str) -> io::Result<()> {
        write!(
            self.terminal.backend_mut(),
            "\u{1b}]52;c;{}\u{7}",
            STANDARD.encode(value.as_bytes())
        )?;
        self.terminal.backend_mut().flush()
    }
    pub fn restore(&mut self) -> io::Result<()> {
        let mut first = None;
        if self.mouse_capture {
            match execute!(self.terminal.backend_mut(), DisableMouseCapture) {
                Ok(()) => self.mouse_capture = false,
                Err(error) => first = Some(error),
            }
        }

        if self.cursor_hidden {
            match execute!(self.terminal.backend_mut(), Show) {
                Ok(()) => self.cursor_hidden = false,
                Err(e) => first = Some(e),
            }
        }
        if self.alternate_screen {
            match execute!(self.terminal.backend_mut(), LeaveAlternateScreen) {
                Ok(()) => {
                    self.alternate_screen = false;
                    ALT_ACTIVE.store(false, Ordering::SeqCst)
                }
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        } else if !self.normal_screen_advanced {
            match execute!(self.terminal.backend_mut(), MoveToNextLine(1)) {
                Ok(()) => self.normal_screen_advanced = true,
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        if self.raw_active {
            match disable_raw_mode() {
                Ok(()) => {
                    self.raw_active = false;
                    RAW_ACTIVE.store(false, Ordering::SeqCst)
                }
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        if let Some(e) = first { Err(e) } else { Ok(()) }
    }
}
impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.restore().is_err() {
            emergency_restore()
        }
    }
}
fn rollback_setup(output: &mut Box<dyn Write>, alternate: bool, hidden: bool) {
    let _ = execute!(output, DisableMouseCapture);
    if hidden {
        let _ = execute!(output, Show);
    }
    if alternate && execute!(output, LeaveAlternateScreen).is_ok() {
        ALT_ACTIVE.store(false, Ordering::SeqCst)
    }
    if disable_raw_mode().is_ok() {
        RAW_ACTIVE.store(false, Ordering::SeqCst)
    }
    let _ = output.flush();
    if ALT_ACTIVE.load(Ordering::SeqCst) || RAW_ACTIVE.load(Ordering::SeqCst) {
        emergency_restore()
    }
}
fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            emergency_restore();
            previous(info)
        }));
    })
}
fn emergency_restore() {
    #[cfg(unix)]
    let output: Box<dyn Write> = OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .map(|f| Box::new(f) as Box<dyn Write>)
        .unwrap_or_else(|_| Box::new(io::stderr()));
    #[cfg(not(unix))]
    let output: Box<dyn Write> = Box::new(io::stderr());
    let mut output = output;
    let _ = execute!(output, DisableMouseCapture, Show);
    if ALT_ACTIVE.load(Ordering::SeqCst) && execute!(output, LeaveAlternateScreen).is_ok() {
        ALT_ACTIVE.store(false, Ordering::SeqCst)
    }
    if RAW_ACTIVE.load(Ordering::SeqCst) && disable_raw_mode().is_ok() {
        RAW_ACTIVE.store(false, Ordering::SeqCst)
    }
    let _ = output.flush();
}
