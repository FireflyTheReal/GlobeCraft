//! The run log: messages for the user and for bug reports, written to `globecraft.log`
//! in the data folder and echoed to standard error. Release builds on Windows have no
//! console, and on other systems standard error is lost unless GlobeCraft was started from
//! a terminal, so the file is where problems can be found afterwards.
//!
//! Write with [`crate::log_line!`]. The file is opened by [`init`]; until then, and when it
//! cannot be opened, lines go to standard error only.
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, PoisonError},
};

/// A log longer than this at startup is moved to `globecraft.log.1` (replacing the older
/// one), so the two files together stay under twice this size.
const ROTATE_ABOVE_BYTES: u64 = 1024 * 1024;
/// Most bytes one session writes to the file; a message repeated in a loop must not fill the
/// disk. Standard error still gets every line.
const MAX_SESSION_BYTES: u64 = 4 * 1024 * 1024;

struct LogFile {
    file: File,
    written: u64,
}

static LOG: OnceLock<Mutex<Option<LogFile>>> = OnceLock::new();

fn log() -> &'static Mutex<Option<LogFile>> {
    LOG.get_or_init(|| Mutex::new(None))
}

/// Where the run log is written.
pub fn path() -> PathBuf {
    crate::paths::cache_dir().join("globecraft.log")
}

/// Opens the run log for this session, after moving a long one aside. Call once at
/// startup; failures leave logging on standard error only.
pub fn init() {
    let file = open(&path());
    if let Err(error) = &file {
        eprintln!("Run log unavailable ({error}); messages go to standard error only");
    }
    *log().lock().unwrap_or_else(PoisonError::into_inner) =
        file.ok().map(|file| LogFile { file, written: 0 });
    write(format_args!(
        "GlobeCraft {} started ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
}

fn open(path: &Path) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > ROTATE_ABOVE_BYTES) {
        let mut older = path.as_os_str().to_owned();
        older.push(".1");
        std::fs::rename(path, older)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Writes one line, with the time (UTC), to the run log and standard error.
pub fn write(message: std::fmt::Arguments<'_>) {
    let line = format!(
        "{} {message}\n",
        globecraft_core::time::format_utc(globecraft_core::math::now_unix())
    );
    eprint!("{line}");
    let mut state = log().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(open) = state.as_mut()
        && open.written < MAX_SESSION_BYTES
    {
        // A failed write cannot be reported anywhere better than standard error, which
        // already has the line.
        if open.file.write_all(line.as_bytes()).is_ok() {
            open.written += line.len() as u64;
        }
    }
}

/// Logs every panic, with its backtrace, before the default handler runs. Worker threads
/// catch their panics so the app keeps running; the log is where those show up.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        write(format_args!(
            "panic in thread {}: {info}\n{}",
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        ));
        default(info);
    }));
}

/// Writes a line to the run log (see [`run_log`](crate::run_log)), formatted like
/// `println!`.
#[macro_export]
macro_rules! log_line {
    ($($arg:tt)*) => {
        $crate::run_log::write(format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_log_is_moved_aside_and_a_new_one_appended() {
        let root = std::env::temp_dir().join(format!("globecraft-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("globecraft.log");
        drop(open(&path).unwrap());
        std::fs::write(&path, vec![b'x'; ROTATE_ABOVE_BYTES as usize + 1]).unwrap();
        let mut file = open(&path).unwrap();
        file.write_all(b"new\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new\n");
        assert_eq!(
            std::fs::metadata(root.join("globecraft.log.1"))
                .unwrap()
                .len(),
            ROTATE_ABOVE_BYTES + 1
        );
        // A short log is kept and appended to.
        drop(file);
        open(&path).unwrap().write_all(b"more\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new\nmore\n");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
