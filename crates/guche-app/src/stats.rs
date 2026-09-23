use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRecord {
    pub old_name: String,
    pub new_name: String,
    pub user_id: i64,
    pub time: String,
    pub code: String,
}

/// Record times are "YYYY-MM-DD HH:MM:SS". A date-only end bound must cover the whole day.
fn range_end(end_date: &str) -> String {
    if end_date.len() == 10 {
        format!("{} 23:59:59", end_date)
    } else {
        end_date.to_string()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GroupStats {
    pub group_id: i64,
    pub records: Vec<RenameRecord>,
}

impl GroupStats {
    pub async fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        let data = tokio::fs::read_to_string(path).await.unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    }

    pub async fn save(&self, path: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = tokio::fs::write(path, json).await;
        }
    }

    pub fn add_record(&mut self, record: RenameRecord, max_records: usize) {
        self.records.push(record);
        if self.records.len() > max_records {
            self.records.drain(0..self.records.len() - max_records);
        }
    }

    pub fn count_today(&self) -> usize {
        let today = crate::utils::now_date();
        self.records.iter().filter(|r| r.time.starts_with(&today)).count()
    }

    pub fn count_in_range(&self, start_date: &str, end_date: &str) -> usize {
        let end = range_end(end_date);
        self.records.iter().filter(|r| r.time.as_str() >= start_date && r.time.as_str() <= end.as_str()).count()
    }

    pub fn get_records_in_range(&self, start_date: &str, end_date: &str) -> Vec<&RenameRecord> {
        let end = range_end(end_date);
        self.records.iter().filter(|r| r.time.as_str() >= start_date && r.time.as_str() <= end.as_str()).collect()
    }

    pub fn delete_records_in_range(&mut self, start_date: &str, end_date: &str) -> usize {
        let end = range_end(end_date);
        let before = self.records.len();
        self.records.retain(|r| r.time.as_str() < start_date || r.time.as_str() > end.as_str());
        before - self.records.len()
    }

    pub fn clear(&mut self) {
        self.records.clear();
    }
}

#[derive(Debug)]
pub struct StatsManager {
    base_dir: PathBuf,
}

impl StatsManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn group_dir(&self) -> PathBuf {
        self.base_dir.join("stats")
    }

    fn group_path(&self, group_id: i64) -> PathBuf {
        self.group_dir().join(format!("{}.json", group_id))
    }

    pub async fn load_group(&self, group_id: i64) -> GroupStats {
        let path = self.group_path(group_id);
        GroupStats::load(&path).await
    }

    pub async fn save_group(&self, group_id: i64, stats: &GroupStats) {
        let dir = self.group_dir();
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = self.group_path(group_id);
        stats.save(&path).await;
    }

    pub async fn add_record(&self, group_id: i64, record: RenameRecord, max_records: usize) {
        let mut stats = self.load_group(group_id).await;
        stats.add_record(record, max_records);
        self.save_group(group_id, &stats).await;
    }

    pub async fn get_all_groups(&self) -> Vec<i64> {
        let dir = self.group_dir();
        if !dir.exists() {
            return vec![];
        }
        
        let mut entries = tokio::fs::read_dir(&dir).await.ok();
        let mut groups = Vec::new();
        while let Some(entry) = entries.as_mut() {
            if let Ok(e) = entry.next_entry().await {
                if let Some(e) = e {
                    let name = e.file_name().to_string_lossy().to_string();
                    if let Some(gid) = name.strip_suffix(".json").and_then(|s| s.parse::<i64>().ok()) {
                        groups.push(gid);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        groups
    }

    pub async fn total_count(&self) -> usize {
        let groups = self.get_all_groups().await;
        let mut total = 0;
        for gid in groups {
            total += self.load_group(gid).await.records.len();
        }
        total
    }

    pub async fn count_today(&self) -> usize {
        let groups = self.get_all_groups().await;
        let mut total = 0;
        for gid in groups {
            total += self.load_group(gid).await.count_today();
        }
        total
    }

    pub async fn delete_group_stats(&self, group_id: i64) {
        let path = self.group_path(group_id);
        let _ = tokio::fs::remove_file(path).await;
    }

    pub async fn clear_group(&self, group_id: i64) {
        let mut stats = self.load_group(group_id).await;
        stats.clear();
        self.save_group(group_id, &stats).await;
    }

    pub async fn delete_records_in_range(&self, group_id: i64, start_date: &str, end_date: &str) -> usize {
        let mut stats = self.load_group(group_id).await;
        let deleted = stats.delete_records_in_range(start_date, end_date);
        if deleted > 0 {
            self.save_group(group_id, &stats).await;
        }
        deleted
    }

    pub async fn clear_all(&self) {
        let groups = self.get_all_groups().await;
        for gid in groups {
            self.delete_group_stats(gid).await;
        }
    }

    // 兼容旧格式迁移
    pub async fn migrate_from_legacy(&self, legacy_path: &Path) {
        if !legacy_path.exists() {
            return;
        }
        
        let data = tokio::fs::read_to_string(legacy_path).await.unwrap_or_default();
        if let Ok(legacy) = serde_json::from_str::<LegacyStatsStore>(&data) {
            for (gid_str, records) in legacy.records {
                if let Ok(gid) = gid_str.parse::<i64>() {
                    let group_stats = GroupStats {
                        group_id: gid,
                        records: records,
                    };
                    self.save_group(gid, &group_stats).await;
                }
            }
            log::info!("已迁移旧格式统计数据");
        }
    }
}

#[derive(Debug, Deserialize)]
struct LegacyStatsStore {
    records: std::collections::HashMap<String, Vec<RenameRecord>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(time: &str) -> RenameRecord {
        RenameRecord {
            old_name: "a".into(),
            new_name: "b".into(),
            user_id: 1,
            time: time.into(),
            code: "12345".into(),
        }
    }

    #[test]
    fn range_end_date_only_includes_whole_day() {
        let mut s = GroupStats::default();
        s.records.push(rec("2026-09-17 00:00:00"));
        s.records.push(rec("2026-09-17 14:30:00"));
        s.records.push(rec("2026-09-17 23:59:59"));
        s.records.push(rec("2026-09-18 00:00:00"));
        assert_eq!(s.count_in_range("2026-09-17", "2026-09-17"), 3);
        assert_eq!(s.get_records_in_range("2026-09-17", "2026-09-17").len(), 3);
        assert_eq!(s.delete_records_in_range("2026-09-17", "2026-09-17"), 3);
        assert_eq!(s.records.len(), 1);
    }

    #[test]
    fn range_end_explicit_datetime() {
        let mut s = GroupStats::default();
        s.records.push(rec("2026-09-17 10:00:00"));
        s.records.push(rec("2026-09-17 12:00:00"));
        assert_eq!(s.count_in_range("2026-09-17 00:00:00", "2026-09-17 11:00:00"), 1);
    }
}