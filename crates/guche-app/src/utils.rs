fn local_time() -> (u64, u64, u64, u64) {
    use time::OffsetDateTime;
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    (now.year() as u64, now.month() as u64, now.day() as u64,
     now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64)
}

pub fn now_hms() -> String {
    let (_, _, _, s) = local_time();
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

pub fn now_date() -> String {
    let (y, m, d, _) = local_time();
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn now_datetime() -> String {
    format!("{} {}", now_date(), now_hms())
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub tag: &'static str,
    pub message: String,
}

macro_rules! lg {
    ($tag:literal, $($arg:tt)*) => {
        $crate::utils::LogEntry { tag: $tag, message: format!($($arg)*) }
    };
}
pub(crate) use lg;