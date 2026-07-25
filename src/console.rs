use log::Level;
use parking_lot::Mutex;

use crate::format::{Color, Component, ComponentType};
use crate::settings::SettingStore;
use crate::{paths, ui};
use crate::{render, StringSetting};

use std::backtrace::Backtrace;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// Global handle to the log file so the panic hook can write to it
/// even when the Console mutex is held by the panicking thread.
static LOG_FILE: OnceLock<Mutex<fs::File>> = OnceLock::new();

/// Records the absolute path of the active log file so we can surface
/// it to the user (printed on startup, included in panic headers).
static LOG_FILE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set to true once the panic hook has been installed, so we never
/// double-install it if Console::new() is called more than once.
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

pub struct Console {
    history: Vec<Component>,
    dirty: bool,
    logfile: fs::File,
    log_level_term: log::Level,
    log_level_file: log::Level,

    elements: Option<ConsoleElements>,
    active: bool,
    position: f64,
}

struct ConsoleElements {
    background: ui::ImageRef,
    lines: Vec<ui::FormattedRef>,
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    pub fn new() -> Console {
        Self::with_log_file(None)
    }

    /// Opens (or creates) the log file. If `custom_path` is `None`, the
    /// default location is `<exe_dir>/log/client.log` — i.e. a `log/`
    /// folder next to the running .exe. This makes the portable
    /// .zip distribution work out of the box: extract anywhere, run,
    /// and the log lands in the bundled `log/` folder automatically.
    /// If that location isn't writable, falls back to the user cache
    /// directory (`<cache_dir>/leafish/client.log`).
    pub fn with_log_file(custom_path: Option<PathBuf>) -> Console {
        let preferred_path = custom_path.unwrap_or_else(default_log_path);

        // Make sure the parent directory exists so we don't panic if the
        // user pointed us at a fresh path. This is what makes the
        // portable .zip distribution work — the bundled `log/` folder
        // may or may not already exist on disk, but either way we
        // (re)create it here so the file open below always succeeds.
        if let Some(parent) = preferred_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Truncate per run so the log reflects the current session — much
        // easier to read when dissecting a crash.
        //
        // If the preferred location isn't writable (e.g. the .exe was
        // extracted into Program Files or another read-only folder),
        // fall back to the user's cache directory so we still get a
        // log somewhere usable.
        let (logfile, log_path): (fs::File, PathBuf) =
            match OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&preferred_path)
            {
                Ok(f) => (f, preferred_path),
                Err(e) => {
                    let fallback = paths::get_cache_dir().join("client.log");
                    let _ = fs::create_dir_all(
                        fallback.parent().unwrap_or(std::path::Path::new(".")),
                    );
                    eprintln!(
                        "warning: could not open log file at {} ({}); falling back to {}",
                        preferred_path.display(),
                        e,
                        fallback.display()
                    );
                    let f = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(&fallback)
                        .expect("failed to open log file in fallback location");
                    (f, fallback)
                }
            };

        // Stash global handles for the panic hook.
        let _ = LOG_FILE.set(Mutex::new(logfile.try_clone().expect("clone log file")));
        let _ = LOG_FILE_PATH.set(log_path.clone());

        install_panic_hook();

        Console {
            history: vec![Component::new(ComponentType::new("", None)); 200],
            dirty: false,
            logfile,
            log_level_term: log::Level::Info,
            log_level_file: log::Level::Trace,

            elements: None,
            active: false,
            position: -220.0,
        }
    }

    /// Returns the absolute path of the active log file, if the Console
    /// has been initialised.
    pub fn log_file_path() -> Option<&'static PathBuf> {
        LOG_FILE_PATH.get()
    }

    /// Force-flush the global log file. Cheap: just calls `sync_data()`
    /// on the underlying `File` under the global mutex.
    ///
    /// This is used by diagnostic code that wants to guarantee a log
    /// line is physically on disk before running a potentially-fatal
    /// operation (e.g. before entering a packet handler that has
    /// crashed before).
    pub fn flush_logs() {
        if let Some(global) = LOG_FILE.get() {
            let f = global.lock();
            let _ = f.sync_data();
        }
    }

    fn log_level_from_env(name: &str) -> Option<log::Level> {
        let variable_string = std::env::var(name).ok()?;
        log::Level::from_str(&variable_string).ok()
    }

    pub fn configure(&mut self, settings: &SettingStore) {
        self.log_level_term = term_log_level(settings).unwrap_or(Level::Info);
        self.log_level_file = file_log_level(settings).unwrap_or(Level::Debug);

        for name in ["RUST_LOG", "LOG_LEVEL"].iter() {
            if let Some(level) = Console::log_level_from_env(name) {
                self.log_level_term = level;
                self.log_level_file = level;
            }
        }
        if let Some(level) = Console::log_level_from_env("RUST_LOG") {
            self.log_level_term = level;
        }
        if let Some(level) = Console::log_level_from_env("LOG_LEVEL_FILE") {
            self.log_level_file = level;
        }

        // IMPORTANT: do NOT call `log::info!` / `log::debug!` / etc. from
        // inside this method. `configure()` is always invoked as
        // `con.lock().configure(...)` from main.rs, so the Console mutex
        // is already held. Every log call goes through `ConsoleProxy::log`
        // which tries to `self.console.lock()` again — and
        // `parking_lot::Mutex` is NOT reentrant, so doing so deadlocks
        // the calling thread instantly. (This was the bug that caused
        // the game to hang at "settings all loaded!" right after startup.)
        //
        // Instead, toggle the network-debug flag here (it's just an
        // atomic store, no logging), and let the caller log a status
        // line AFTER releasing the lock. See `configure_and_report`
        // below for the public API.
        if self.log_level_file >= log::Level::Debug {
            leafish_protocol::protocol::enable_network_debug();
        }
    }

    /// Convenience wrapper around `configure` that returns enough info
    /// for the caller to log the network-debug status line itself, AFTER
    /// the Console mutex has been released. This is the safe way to use
    /// `configure` from main — it avoids the re-entrant-lock deadlock
    /// described in the comment inside `configure`.
    ///
    /// Returns `true` if network packet logging was enabled, `false`
    /// otherwise. The caller should log this itself; `configure` no
    /// longer logs anything on its own.
    pub fn configure_and_report(&mut self, settings: &SettingStore) -> bool {
        self.configure(settings);
        self.log_level_file >= log::Level::Debug
    }

    pub fn _is_active(&self) -> bool {
        self.active
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
    }

    pub fn _activate(&mut self) {
        self.active = true;
    }

    pub fn tick(
        &mut self,
        ui_container: &mut ui::Container,
        renderer: Arc<render::Renderer>,
        delta: f64,
        width: f64,
    ) {
        if !self.active && self.position <= -220.0 {
            self.elements = None;
            return;
        }
        if self.active {
            if self.position < 0.0 {
                self.position += delta * 4.0;
            } else {
                self.position = 0.0;
            }
        } else if self.position > -220.0 {
            self.position -= delta * 4.0;
        } else {
            self.position = -220.0;
        }

        let w = match ui_container.mode {
            ui::Mode::Scaled => width,
            ui::Mode::Unscaled(scale) => 854.0 / scale,
        };
        if self.elements.is_none() {
            let background = ui::ImageBuilder::new()
                .texture("leafish:solid")
                .position(0.0, self.position)
                .size(w, 220.0)
                .colour((0, 0, 0, 180))
                .draw_index(500)
                .create(ui_container);
            self.elements = Some(ConsoleElements {
                background,
                lines: vec![],
            });
            self.dirty = true;
        }
        let elements = self.elements.as_mut().unwrap();
        let mut background = elements.background.borrow_mut();
        background.y = self.position;
        background.width = w;

        if self.dirty {
            self.dirty = false;
            elements.lines.clear();

            let mut offset = 0.0;
            for line in self.history.iter().rev() {
                if offset >= 210.0 {
                    break;
                }
                let (_, height) =
                    ui::Formatted::compute_size(renderer.clone(), line, w - 10.0, 1.0, 1.0, 1.0);
                elements.lines.push(
                    ui::FormattedBuilder::new()
                        .text(line.clone())
                        .position(5.0, 5.0 + offset)
                        .max_width(w - 10.0)
                        .alignment(ui::VAttach::Bottom, ui::HAttach::Left)
                        .create(&mut *background),
                );
                offset += height;
            }
        }
    }

    fn log(&mut self, record: &log::Record) {
        for filtered in FILTERED_CRATES {
            if record.module_path().unwrap_or("").starts_with(filtered) {
                return;
            }
        }

        let mut file = &record.file().unwrap_or("").replace('\\', "/")[..];
        if let Some(pos) = file.rfind("src/") {
            file = &file[pos + 4..];
        }

        let line = format!(
            "[{}:{}][{}] {}",
            file,
            record.line().unwrap_or(0),
            record.level(),
            record.args()
        );

        if record.level() <= self.log_level_file {
            // Prefer the global handle: that way the panic hook (which
            // only has access to the global, not the Console mutex) can
            // see every line that's been written so far. Fall back to
            // the in-Console copy if the global isn't initialised
            // (shouldn't happen, but be defensive).
            let target: &mut fs::File = if let Some(global) = LOG_FILE.get() {
                &mut *global.lock()
            } else {
                &mut self.logfile
            };
            let _ = target.write_all(line.as_bytes());
            let _ = target.write_all(b"\n");
            // Push to OS buffers ASAP so a hard crash doesn't eat the
            // last few lines. sync_data is the cheap variant (no
            // metadata sync) and is plenty for our purposes.
            let _ = target.sync_data();
        }

        if record.level() <= self.log_level_term {
            println!("{}", line);

            self.history.remove(0);
            let component = Component {
                list: vec![
                    ComponentType::new("[", None),
                    ComponentType::new(file, Some(Color::Green)),
                    ComponentType::new(":", None),
                    ComponentType::new(
                        &format!("{}", record.line().unwrap_or(0)),
                        Some(Color::Aqua),
                    ),
                    ComponentType::new("]", None),
                    ComponentType::new("[", None),
                    ComponentType::new(
                        &format!("{}", record.level()),
                        Some(match record.level() {
                            log::Level::Debug => Color::Green,
                            log::Level::Error => Color::Red,
                            log::Level::Warn => Color::Yellow,
                            log::Level::Info => Color::Aqua,
                            log::Level::Trace => Color::Blue,
                        }),
                    ),
                    ComponentType::new("] ", None),
                    ComponentType::new(&format!("{}", record.args()), None),
                ],
            };
            self.history.push(component);
            self.dirty = true;
        }
    }
}

fn _log_level_from_str(s: &str) -> Option<log::Level> {
    // TODO: no opposite of FromStr in log crate?
    use log::Level::*;
    match s {
        "trace" => Some(Trace),
        "debug" => Some(Debug),
        "info" => Some(Info),
        "warn" => Some(Warn),
        "error" => Some(Error),
        _ => None,
    }
}

/// Picks the default log file location.
///
/// For the portable RustCraft distribution (a .zip with the .exe and a
/// `log/` folder next to each other), we want the log to land in that
/// `log/` folder automatically — no flags needed. So the default is
/// `<exe_dir>/log/client.log`, i.e. a sibling folder of the running
/// executable.
///
/// If for some reason `current_exe()` fails (very unusual), fall back
/// to the user's cache directory.
fn default_log_path() -> PathBuf {
    if let Some(exe_path) = std::env::current_exe().ok() {
        if let Some(exe_dir) = exe_path.parent() {
            return exe_dir.join("log").join("client.log");
        }
    }
    paths::get_cache_dir().join("client.log")
}

fn term_log_level(store: &SettingStore) -> Option<Level> {
    let val = store.get_string(StringSetting::LogLevelTerm);
    Level::from_str(&val).ok()
}
fn file_log_level(store: &SettingStore) -> Option<Level> {
    let val = store.get_string(StringSetting::LogLevelFile);
    Level::from_str(&val).ok()
}

const FILTERED_CRATES: &[&str] = &[
    //"reqwest", // TODO: needed?
    "mime",
];

pub struct ConsoleProxy {
    console: Arc<Mutex<Console>>,
}

impl ConsoleProxy {
    pub fn new(con: Arc<Mutex<Console>>) -> ConsoleProxy {
        ConsoleProxy { console: con }
    }
}

impl log::Log for ConsoleProxy {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Trace
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.console.lock().log(record);
        }
    }

    fn flush(&self) {}
}

unsafe impl Send for ConsoleProxy {}
unsafe impl Sync for ConsoleProxy {}

/// Installs a panic hook that writes the panic payload, location, and a
/// backtrace into the log file before the process unwinds. This is what
/// makes the .log file useful for diagnosing "joined a server and it
/// crashed" scenarios — without this, the panic message goes to stderr
/// only and is lost the moment the window closes.
///
/// Idempotent: safe to call multiple times.
pub fn install_panic_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    let log_path = LOG_FILE_PATH.get().cloned();

    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());

        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string panic payload>");

        let bt = Backtrace::force_capture();

        // Stamp the build version into the panic header so we can tell
        // EXACTLY which binary panicked. This is critical for diagnosing
        // "stale .zip" reports — if the user reports a panic but the
        // build hash here doesn't match the latest release, we know
        // immediately that they're running an older build.
        let build_hash = env!("LEAFISH_BUILD_GIT_HASH");
        let build_time = env!("LEAFISH_BUILD_TIME");

        let header = format!(
            "\n\n================ PANIC ================\n\
             time    : {}\n\
             build   : {} (built {})\n\
             location: {}\n\
             message : {}\n\
             log file: {}\n\
             ---------------- backtrace ----------------\n{}\n\
             ========================================\n",
            rfc3339_now(),
            build_hash,
            build_time,
            location,
            payload,
            log_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<not initialised>".to_string()),
            bt
        );

        // 1) Write to the global log file handle so we get the panic
        //    recorded even if the Console mutex is poisoned / held.
        if let Some(global) = LOG_FILE.get() {
            let mut gf = global.lock();
            let _ = gf.write_all(header.as_bytes());
            let _ = gf.sync_all();
        }

        // 2) ALSO write to a separate crash-<timestamp>.log file in the
        //    same directory as the main log. This is a belt-and-suspenders
        //    fallback in case the main log file is unwritable for any
        //    reason (file lock contention, file handle closed, etc.).
        //    The crash file has a unique name per panic so multiple
        //    panics don't overwrite each other, and so the user can
        //    easily spot it in the log folder.
        if let Some(main_path) = log_path.as_ref() {
            let dir = main_path.parent().unwrap_or(std::path::Path::new("."));
            let ts = {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            };
            let crash_path = dir.join(format!("crash-{}.log", ts));
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&crash_path)
            {
                let _ = f.write_all(header.as_bytes());
                let _ = f.sync_all();
                // Also try to dump the last 8 KB of the main log file
                // into the crash file so we have packet context along
                // with the backtrace.
                if let Some(global) = LOG_FILE.get() {
                    // try_lock so we don't deadlock if the panic
                    // happened while another thread holds the log mutex.
                    if let Some(mut gf) = global.try_lock() {
                        // Clamp to file size so we don't try to seek to
                        // a negative position on a small log file.
                        let file_len = gf.metadata().map(|m| m.len()).unwrap_or(0);
                        let tail_size = std::cmp::min(8192, file_len);
                        let _ = gf.seek(SeekFrom::End(-(tail_size as i64)));
                        let mut tail = String::new();
                        let _ = gf.read_to_string(&mut tail);
                        let _ = f.write_all(b"\n--- last 8KB of main log ---\n");
                        let _ = f.write_all(tail.as_bytes());
                    }
                }
            }
        }

        // 3) Also try the regular logger so terminal / in-game console
        //    get a chance to see it.
        log::error!("PANIC at {}: {}", location, payload);

        // 4) Print to stderr as a last-resort so the panic is visible
        //    even if every other mechanism fails. On Windows, stderr
        //    goes to the parent terminal (if launched from one) or to
        //    the OS crash dialog (if not).
        eprintln!("{}", header);
    }));
}

fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs = now % 60;
    let mins = (now / 60) % 60;
    let hours = (now / 3600) % 24;
    let days = now / 86400;
    // Good enough for log readability; not a real date lib but stable.
    format!("epoch+{}d {:02}:{:02}:{:02} (utc)", days, hours, mins, secs)
}

// ---------------------------------------------------------------------------
// Module-level accessors for the global log file handle.
//
// These are used by the VEH (Vectored Exception Handler) in `veh.rs`,
// which needs to write to the log file from inside an SEH exception
// callback — WITHOUT going through the `ConsoleProxy` logger (because
// the logger tries to acquire the Console mutex, which may be held by
// the crashing thread, causing a deadlock).
//
// Both accessors are designed for use from exception-handler context:
//   - `global_log_mutex()` returns the raw `&Mutex<File>` so the caller
//     can use `try_lock()` to avoid blocking.
//   - `log_file_path()` is already publicly exposed on `Console`,
//     but we re-export it at module level for symmetry.
// ---------------------------------------------------------------------------

/// Returns a reference to the global log file mutex, if the Console
/// has been initialised.
///
/// Used by the VEH handler to write crash diagnostics directly to the
/// log file without going through the ConsoleProxy logger (which
/// could deadlock if the crashing thread holds the Console mutex).
pub fn global_log_mutex() -> Option<&'static Mutex<fs::File>> {
    LOG_FILE.get()
}

/// Returns the absolute path of the active log file, if the Console
/// has been initialised. Module-level re-export of
/// `Console::log_file_path()` for symmetry with `global_log_mutex()`.
pub fn log_file_path() -> Option<&'static PathBuf> {
    LOG_FILE_PATH.get()
}
