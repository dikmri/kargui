//! 記録ファイル。1 行に、時刻・出来事・引数・結果を書く。
//!
//! 2026-11-20T12:00:00.123Z t_ms=1763640000123 app=egui event=ready args="items=0" result="ok"

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

struct Logger {
    app: &'static str,
    file: Mutex<File>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// UNIX 時刻（ミリ秒）を UTC の ISO 8601 にする。
pub fn format_utc(unix_ms: u64) -> String {
    let ms = unix_ms % 1000;
    let secs = unix_ms / 1000;
    let (h, mi, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    // 1970-01-01 からの日数を年月日にする（H. Hinnant の civil_from_days）
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 記録の 1 行を作る（改行は含まない）。
pub fn line(unix_ms: u64, app: &str, event: &str, args: &str, result: &str) -> String {
    format!(
        "{} t_ms={unix_ms} app={app} event={event} args={} result={}",
        format_utc(unix_ms),
        quote(args),
        quote(result)
    )
}

/// 記録を始める。path が無ければ、今いる場所からの logs/rust-gui/<app>.log に追記する。
pub fn init(app: &'static str, path: Option<&str>) -> std::io::Result<String> {
    let path = path.map(str::to_string).unwrap_or_else(|| format!("logs/rust-gui/{app}.log"));
    if let Some(dir) = std::path::Path::new(&path).parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let _ = LOGGER.set(Logger { app, file: Mutex::new(file) });
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        event("panic", "", &info.to_string());
        prev(info);
    }));
    Ok(path)
}

/// 出来事を 1 行書く。init の前に呼ばれたときは何もしない。
pub fn event(name: &str, args: &str, result: &str) {
    if let Some(l) = LOGGER.get() {
        let text = line(now_ms(), l.app, name, args, result);
        if let Ok(mut f) = l.file.lock() {
            let _ = writeln!(f, "{text}");
            let _ = f.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_format_matches_known_dates() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_utc(1_763_640_000_123), "2025-11-20T12:00:00.123Z");
        assert_eq!(format_utc(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(format_utc(4_102_444_799_999), "2099-12-31T23:59:59.999Z");
    }

    #[test]
    fn line_quotes_args_and_result() {
        assert_eq!(
            line(1_763_640_000_123, "egui", "filter", "q=\"資料\" status=all", "hits=1250\nok"),
            "2025-11-20T12:00:00.123Z t_ms=1763640000123 app=egui event=filter args=\"q=\\\"資料\\\" status=all\" result=\"hits=1250\\nok\""
        );
    }
}
