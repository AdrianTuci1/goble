//! The app's log sinks: stderr (through `env_logger`) and a file under
//! `~/.goble/logs`.
//!
//! A GUI run started from Finder has no terminal, so a `log::error!` that only
//! reaches stderr is lost. The file sink keeps the same records on disk, which
//! is what makes an "LLM request failed" diagnosable after the fact. The file
//! is size-capped: one previous run is kept as `goble.log.1`.
//!
//! Records are scrubbed with the telemetry redactor before they are written.
//! Redaction is a safety net, not a licence: callers still must not log
//! credentials (see `.agents/07-observability/logs.md`).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use goble_telemetry::Redactor;
use log::{LevelFilter, Log, Metadata, Record};

/// The current run's log file, inside the logs directory.
pub const LOG_FILE: &str = "goble.log";
/// The previous run's file, kept across a rotation.
pub const ROTATED_LOG_FILE: &str = "goble.log.1";
/// How large `goble.log` may grow before it is rotated.
pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Longest provider error body written to a single log line.
const MAX_MESSAGE_CHARS: usize = 8_000;

/// A JSON error body larger than any diagnostic needs is cut, not dropped.
fn truncate(message: &str) -> String {
    if message.chars().count() <= MAX_MESSAGE_CHARS {
        return message.to_string();
    }
    let mut cut: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    cut.push_str("…[truncated]");
    cut
}

struct FileState {
    file: File,
    len: u64,
}

/// Appends redacted records to one file, rotating it when it reaches
/// `max_bytes`. A logging failure never propagates: it cannot stop the app.
pub struct FileLogger {
    path: PathBuf,
    state: Mutex<FileState>,
    level: LevelFilter,
    redactor: Redactor,
    max_bytes: u64,
}

impl FileLogger {
    /// Open (and rotate when needed) the log file in `dir`.
    pub fn under(dir: impl AsRef<Path>, max_bytes: u64) -> std::io::Result<Self> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let path = dir.join(LOG_FILE);
        rotate_if_full(&path, max_bytes);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            path,
            state: Mutex::new(FileState { file, len }),
            level: LevelFilter::Info,
            redactor: Redactor::new(),
            max_bytes,
        })
    }

    /// The file this logger appends to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn level(&self) -> LevelFilter {
        self.level
    }

    pub fn set_level(&mut self, level: LevelFilter) {
        self.level = level;
    }

    fn write_record(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = self.redactor.scrub(&record.args().to_string());
        let message = truncate(&message);
        let location = match (record.file(), record.line()) {
            (Some(file), Some(line)) => format!(" {file}:{line}"),
            (Some(file), None) => format!(" {file}"),
            _ => String::new(),
        };
        let line = format!(
            "{} {:<5} {}{location}: {message}\n",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            record.level(),
            record.target(),
        );

        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if state.len > 0 && state.len + line.len() as u64 > self.max_bytes {
            rotate_open(&self.path, &mut state);
        }
        if let Err(err) = state.file.write_all(line.as_bytes()) {
            // A log that cannot be written must not take the app down, and
            // logging the failure would recurse into this same sink.
            eprintln!("goble: could not write {}: {err}", self.path.display());
            return;
        }
        state.len += line.len() as u64;
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        self.write_record(record);
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            let _ = state.file.flush();
        }
    }
}

/// Keep one previous file. A rename that fails leaves the current file to grow
/// past the cap instead of losing records or failing the run.
fn rotate_if_full(path: &Path, max_bytes: u64) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    if metadata.len() == 0 || metadata.len() < max_bytes {
        return;
    }
    let rotated = path.with_file_name(ROTATED_LOG_FILE);
    let _ = std::fs::rename(path, rotated);
}

fn rotate_open(path: &Path, state: &mut FileState) {
    let _ = state.file.flush();
    let rotated = path.with_file_name(ROTATED_LOG_FILE);
    if std::fs::rename(path, &rotated).is_err() {
        return;
    }
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(path) {
        state.file = file;
        state.len = 0;
    }
}

/// `env_logger` for the terminal, plus the file that survives a GUI run.
pub struct TeeLogger {
    stderr: Box<dyn Log>,
    stderr_level: LevelFilter,
    file: Option<FileLogger>,
}

impl TeeLogger {
    /// The most verbose level any sink accepts; this is what the process-wide
    /// max level must be set to, or a record never reaches the sink.
    pub fn max_level(&self) -> LevelFilter {
        let file_level = self
            .file
            .as_ref()
            .map(FileLogger::level)
            .unwrap_or(LevelFilter::Off);
        self.stderr_level.max(file_level)
    }

    /// Where the file sink writes, for diagnostics and tests.
    pub fn file_path(&self) -> Option<&Path> {
        self.file.as_ref().map(FileLogger::path)
    }
}

impl Log for TeeLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.stderr.enabled(metadata)
            || self
                .file
                .as_ref()
                .is_some_and(|file| file.enabled(metadata))
    }

    fn log(&self, record: &Record<'_>) {
        self.stderr.log(record);
        if let Some(file) = &self.file {
            file.log(record);
        }
    }

    fn flush(&self) {
        self.stderr.flush();
        if let Some(file) = &self.file {
            file.flush();
        }
    }
}

/// The app's logger: `env_logger` on stderr, and a file when `~/.goble` can be
/// resolved. The file keeps at least `Info` even when `RUST_LOG` leaves stderr
/// at its default of `Error`, so warnings reach disk during a GUI run.
pub fn app_logger() -> TeeLogger {
    let stderr = env_logger::Builder::from_default_env().build();
    let stderr_level = stderr.filter();
    let file = goble_core::app_home::home_dir()
        .ok()
        .map(|home| home.join("logs"))
        .and_then(|dir| FileLogger::under(dir, DEFAULT_MAX_BYTES).ok())
        .map(|mut file| {
            file.set_level(LevelFilter::Info.max(stderr_level));
            file
        });
    TeeLogger {
        stderr: Box::new(stderr),
        stderr_level,
        file,
    }
}

/// Install [`app_logger`] as the process logger. Used when telemetry cannot
/// start; a logger already installed is left alone.
pub fn install_default() {
    let logger = app_logger();
    let level = logger.max_level();
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::{Level, Record};

    fn write_record(logger: &FileLogger, level: Level, message: &str) {
        logger.log(
            &Record::builder()
                .args(format_args!("{message}"))
                .level(level)
                .target("goble::test")
                .file(Some("test.rs"))
                .line(Some(42))
                .build(),
        );
    }

    #[test]
    fn a_record_reaches_the_file_with_its_level_and_location() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logger = FileLogger::under(dir.path(), DEFAULT_MAX_BYTES).expect("logger");
        write_record(&logger, Level::Error, "LLM request failed: 400 Bad Request");
        logger.flush();

        let text = std::fs::read_to_string(logger.path()).expect("log file");
        assert!(text.contains("ERROR"), "{text}");
        assert!(text.contains("goble::test"), "{text}");
        assert!(
            text.contains("LLM request failed: 400 Bad Request"),
            "{text}"
        );
        assert!(text.contains("test.rs:42"), "{text}");
    }

    #[test]
    fn a_credential_in_a_message_is_scrubbed_before_it_is_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logger = FileLogger::under(dir.path(), DEFAULT_MAX_BYTES).expect("logger");
        write_record(
            &logger,
            Level::Error,
            "Authorization: Bearer abcdef1234567890 rejected",
        );
        logger.flush();

        let text = std::fs::read_to_string(logger.path()).expect("log file");
        assert!(!text.contains("abcdef1234567890"), "{text}");
        assert!(text.contains("[redacted]"), "{text}");
    }

    #[test]
    fn a_full_file_is_rotated_and_the_previous_line_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logger = FileLogger::under(dir.path(), 80).expect("logger");
        write_record(&logger, Level::Error, "first line that fills the file");
        write_record(&logger, Level::Error, "second line forces a rotation");
        logger.flush();

        let current = std::fs::read_to_string(dir.path().join(LOG_FILE)).expect("current log");
        let rotated =
            std::fs::read_to_string(dir.path().join(ROTATED_LOG_FILE)).expect("rotated log");
        assert!(current.contains("second line"), "{current}");
        assert!(rotated.contains("first line"), "{rotated}");
    }

    #[test]
    fn an_over_verbose_message_is_cut_instead_of_written_whole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let logger = FileLogger::under(dir.path(), DEFAULT_MAX_BYTES).expect("logger");
        let body = "provider said no ".repeat(MAX_MESSAGE_CHARS / 8);
        write_record(&logger, Level::Error, &body);
        logger.flush();

        let text = std::fs::read_to_string(logger.path()).expect("log file");
        assert!(text.contains("[truncated]"), "{}", text.len());
        assert!(!text.contains(&body), "the full body must not be written");
    }
}
