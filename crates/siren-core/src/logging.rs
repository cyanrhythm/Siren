//! 日志初始化与文件轮转。
//!
//! 基于 [`log`] 门面实现一个轻量文件日志器，满足以下约定：
//!
//! - **默认位置**：Siren 数据目录下的 `logs/`（`ProjectDirs::data_dir()/logs`）。
//! - **可配置**：环境变量 `SIREN_LOG_DIR` 覆盖输出目录。
//! - **轮转**：单个文件超过 **1 MiB** 即轮转。
//! - **保留**：最多保留 **3** 个历史日志，超出时删除最早的一个。
//! - **命名**：历史日志为 `log-yyyy-MM-dd_hh:mm:ss.log`。
//!   Windows 文件名不允许 `:`，时间分隔符以 `-` 代替（`log-2026-10-13_18-30-05.log`）。
//!
//! 另外支持两个可选环境变量：
//!
//! - `SIREN_LOG_LEVEL`：日志级别（`off`/`error`/`warn`/`info`/`debug`/`trace`），默认 `info`。
//! - `SIREN_LOG_STDERR`：设为 `0`/`false`/`no` 时不再镜像到标准错误，默认镜像。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use log::{LevelFilter, Log, Metadata, Record};

/// 覆盖日志输出目录的环境变量。
pub const LOG_DIR_ENV: &str = "SIREN_LOG_DIR";
/// 覆盖日志级别的环境变量。
pub const LOG_LEVEL_ENV: &str = "SIREN_LOG_LEVEL";
/// 关闭标准错误镜像的环境变量。
pub const LOG_STDERR_ENV: &str = "SIREN_LOG_STDERR";

/// 单个日志文件上限：1 MiB。
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// 历史日志最大保留数量。
const MAX_ROTATED: usize = 3;
/// 当前日志文件名。
const ACTIVE_NAME: &str = "log.log";
/// 历史日志文件名前缀。
const ROTATED_PREFIX: &str = "log-";
/// 历史日志文件名后缀。
const ROTATED_SUFFIX: &str = ".log";

/// 解析日志目录：优先环境变量 `SIREN_LOG_DIR`，其次 Siren 数据目录 `logs/`。
pub fn resolve_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(LOG_DIR_ENV) {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren").map(|d| d.data_dir().join("logs"))
}

/// 初始化全局日志器。失败时静默降级为仅标准错误输出。
pub fn init() {
    let Some(dir) = resolve_dir() else {
        eprintln!("[siren] 无法确定日志目录，日志仅输出到标准错误");
        return;
    };
    match FileLogger::new(dir.clone()) {
        Ok(logger) => {
            if log::set_boxed_logger(Box::new(logger)).is_ok() {
                log::set_max_level(level_from_env());
            }
        }
        Err(err) => eprintln!("[siren] 初始化日志文件失败（{}）：{err}", dir.display()),
    }
}

fn level_from_env() -> LevelFilter {
    match std::env::var(LOG_LEVEL_ENV).map(|v| v.to_ascii_lowercase()) {
        Ok(v) => match v.as_str() {
            "off" => LevelFilter::Off,
            "error" => LevelFilter::Error,
            "warn" | "warning" => LevelFilter::Warn,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            _ => LevelFilter::Info,
        },
        Err(_) => LevelFilter::Info,
    }
}

fn mirror_stderr() -> bool {
    !matches!(
        std::env::var(LOG_STDERR_ENV).as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    )
}

/// 判断写入 `incoming` 字节后是否需要轮转（当前文件非空且将超出上限）。
fn should_rotate(size: u64, incoming: u64) -> bool {
    size > 0 && size.saturating_add(incoming) > MAX_FILE_BYTES
}

/// 历史日志文件名；Windows 下把 `:` 替换为 `-`。
fn rotated_name_at(now: DateTime<Local>) -> String {
    let stamp = now.format("%Y-%m-%d_%H:%M:%S").to_string();
    let stamp = if std::env::consts::OS == "windows" {
        stamp.replace(':', "-")
    } else {
        stamp
    };
    format!("{ROTATED_PREFIX}{stamp}{ROTATED_SUFFIX}")
}

/// 是否为历史日志文件名（不含当前日志 `log.log`）。
fn is_rotated(name: &str) -> bool {
    name.starts_with(ROTATED_PREFIX) && name.ends_with(ROTATED_SUFFIX)
}

/// 保留最新的 [`MAX_ROTATED`] 个历史日志，删除更早的。
///
/// 文件名时间戳零填充，字典序即时间序。
fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_rotated(name))
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(MAX_ROTATED);
    for name in &names[..excess] {
        let _ = fs::remove_file(dir.join(name));
    }
}

fn open_active(path: &Path) -> std::io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    Ok((file, size))
}

struct State {
    file: File,
    size: u64,
}

/// 文件日志器：写入当前日志，超出上限时轮转并保留最近若干份。
pub struct FileLogger {
    dir: PathBuf,
    mirror: bool,
    state: Mutex<State>,
}

impl FileLogger {
    /// 在指定目录创建日志器（目录不存在则创建）。
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let (file, size) = open_active(&dir.join(ACTIVE_NAME))?;
        Ok(Self {
            dir,
            mirror: mirror_stderr(),
            state: Mutex::new(State { file, size }),
        })
    }

    /// 把当前日志归档为带时间戳的文件，并重新打开当前日志。
    fn rotate(&self, state: &mut State) {
        let _ = state.file.flush();
        let archived = self.dir.join(rotated_name_at(Local::now()));
        let _ = fs::rename(self.dir.join(ACTIVE_NAME), &archived);
        prune(&self.dir);
        match open_active(&self.dir.join(ACTIVE_NAME)) {
            Ok((file, size)) => {
                state.file = file;
                state.size = size;
            }
            Err(_) => state.size = 0,
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} [{}] {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args(),
        );
        if self.mirror {
            eprint!("{line}");
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let bytes = line.len() as u64;
        if should_rotate(state.size, bytes) {
            self.rotate(&mut state);
        }
        if state.file.write_all(line.as_bytes()).is_ok() {
            state.size += bytes;
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            let _ = state.file.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 每个测试使用独立的临时目录。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("siren-logging-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rotated_name_uses_timestamp_format() {
        let now = Local.with_ymd_and_hms(2026, 10, 13, 18, 30, 5).unwrap();
        let name = rotated_name_at(now);
        if std::env::consts::OS == "windows" {
            assert_eq!(name, "log-2026-10-13_18-30-05.log");
        } else {
            assert_eq!(name, "log-2026-10-13_18:30:05.log");
        }
    }

    #[test]
    fn active_log_is_not_considered_rotated() {
        assert!(!is_rotated(ACTIVE_NAME));
        assert!(is_rotated("log-2026-10-13_18-30-05.log"));
        assert!(!is_rotated("other.log"));
    }

    #[test]
    fn rotation_triggers_only_after_exceeding_limit() {
        assert!(!should_rotate(0, MAX_FILE_BYTES + 1));
        assert!(!should_rotate(MAX_FILE_BYTES - 1, 1));
        assert!(should_rotate(MAX_FILE_BYTES - 1, 2));
        assert!(should_rotate(MAX_FILE_BYTES, 1));
    }

    #[test]
    fn prune_keeps_newest_three_and_drops_earliest() {
        let dir = temp_dir("prune");
        let names = [
            "log-2026-10-13_18-30-00.log",
            "log-2026-10-13_18-30-01.log",
            "log-2026-10-13_18-30-02.log",
            "log-2026-10-13_18-30-03.log",
            "log-2026-10-13_18-30-04.log",
        ];
        for name in names {
            fs::write(dir.join(name), b"x").unwrap();
        }
        // 当前日志不应被计入或删除
        fs::write(dir.join(ACTIVE_NAME), b"x").unwrap();

        prune(&dir);

        let mut remaining: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().into_string().unwrap())
            .filter(|name| is_rotated(name))
            .collect();
        remaining.sort();

        assert_eq!(remaining.len(), MAX_ROTATED);
        assert!(!remaining.contains(&"log-2026-10-13_18-30-00.log".to_string()));
        assert!(!remaining.contains(&"log-2026-10-13_18-30-01.log".to_string()));
        assert!(remaining.contains(&"log-2026-10-13_18-30-04.log".to_string()));
        assert!(dir.join(ACTIVE_NAME).exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
