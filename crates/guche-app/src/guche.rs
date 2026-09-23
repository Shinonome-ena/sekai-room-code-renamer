use guche_core::Cooldown;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::config::{AppConfig, GroupEntry};
use crate::stats::{RenameRecord, StatsManager};
use crate::utils::LogEntry;
use crate::utils::lg;

#[derive(Debug)]
pub enum Action {
    GetGroupInfo { group_id: i64, code: String },
    SetGroupName { group_id: i64, new_name: String },
    SendGroupMsg { group_id: i64, reply_id: i64, text: String },
}

pub struct GucheEngine {
    pub config: Arc<Mutex<AppConfig>>,
    pub cooldown: Arc<Mutex<Cooldown>>,
    pub stats: Arc<StatsManager>,
    pub log_tx: mpsc::UnboundedSender<LogEntry>,
    pub update_tx: tokio::sync::broadcast::Sender<()>,
}

impl GucheEngine {
    pub async fn new(
        config: Arc<Mutex<AppConfig>>,
        log_tx: mpsc::UnboundedSender<LogEntry>,
        data_dir: std::path::PathBuf,
        update_tx: tokio::sync::broadcast::Sender<()>,
    ) -> Self {
        let stats = StatsManager::new(data_dir.clone());
        stats.migrate_from_legacy(&data_dir.join("stats.json")).await;
        Self {
            config,
            cooldown: Arc::new(Mutex::new(Cooldown::new())),
            stats: Arc::new(stats),
            log_tx,
            update_tx,
        }
    }

    /// 只改闭包碰到的字段；有变化才写 config
    pub async fn edit_group(&self, gid: i64, f: impl FnOnce(&mut GroupEntry)) -> bool {
        let mut cfg = self.config.lock().await;
        let Some(e) = cfg.guche.groups.get_mut(&gid) else { return false };
        let before = e.clone();
        f(e);
        if *e == before { return false; }
        cfg.save(&crate::config::config_path()).await;
        true
    }

    pub async fn handle_event(&self, event: &Value) -> Vec<Action> {
        if event.get("post_type").and_then(|v| v.as_str()) != Some("message") {
            return vec![];
        }
        if event.get("message_type").and_then(|v| v.as_str()) != Some("group") {
            return vec![];
        }

        let gid = event.get("group_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let uid = event.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let mid = event.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0);

        let text = extract_text(event);
        let _ = self.log_tx.send(lg!("RECV", "收到消息: 群{} 用户{} \"{}\"", gid, uid, text));

        let (cmd_en, cmd_dis, cmd_help, cmd_mode, cd) = {
            let c = self.config.lock().await;
            (
                c.commands.guche_enable.trim().to_string(),
                c.commands.guche_disable.trim().to_string(),
                c.commands.guche_help.trim().to_string(),
                c.commands.guche_mode.trim().to_string(),
                c.guche.cooldown_seconds,
            )
        };
        let trimmed = text.trim();

        if trimmed == cmd_en { return self.enable(gid, uid, mid).await; }
        if trimmed == cmd_dis { return self.disable(gid, uid, mid).await; }
        if trimmed == cmd_help {
            return vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: crate::help::generate().to_string() }];
        }
        if !cmd_mode.is_empty() && trimmed.starts_with(&cmd_mode) {
            return self.mode_cmd(trimmed, &cmd_mode, gid, uid, mid).await;
        }

        // 未启用忽略；取 fuzzy（pos 改名时再取）
        let fuzzy = {
            let cfg = self.config.lock().await;
            match cfg.guche.groups.get(&gid) {
                Some(e) => e.mode.unwrap_or(cfg.guche.fuzzy),
                None => return vec![],
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        if !self.cooldown.lock().await.check(gid, now, cd) {
            let _ = self.log_tx.send(lg!("RECV", "冷却中: 群{} 用户{}", gid, uid));
            return vec![];
        }

        let code = match guche_core::match_message(trimmed, fuzzy) {
            Some(c) => c,
            None => return vec![],
        };

        self.cooldown.lock().await.update(gid, now);
        let _ = self.log_tx.send(lg!("MATCH", "匹配成功: 房号={}", code));
        vec![Action::GetGroupInfo { group_id: gid, code }]
    }

    pub async fn handle_rename(&self, gid: i64, old: &str, code: &str, uid: i64, mid: i64) -> Vec<Action> {
        let (pos_end, max) = {
            let cfg = self.config.lock().await;
            let pos_end = cfg.guche.groups.get(&gid)
                .map(|e| e.pos.unwrap_or(cfg.guche.pos_end))
                .unwrap_or(cfg.guche.pos_end);
            (pos_end, cfg.stats.max_records_per_group)
        };

        let (new_name, truncated) = guche_core::compute_new_name(old, code, pos_end);
        let _ = self.log_tx.send(lg!("EXEC", "改名: 群{} \"{}\" → \"{}\"", gid, old, new_name));

        let name = new_name.clone();
        self.edit_group(gid, move |e| e.name = Some(name)).await;

        self.stats.add_record(gid, RenameRecord {
            old_name: old.to_string(),
            new_name: new_name.clone(),
            user_id: uid,
            time: crate::utils::now_datetime(),
            code: code.to_string(),
        }, max).await;

        let reply = if truncated {
            format!("已将群聊名称从'{}'改为'{}'（群名过长已截断）", old, new_name)
        } else {
            format!("已将群聊名称从'{}'改为'{}'", old, new_name)
        };
        let _ = self.log_tx.send(lg!("REPLY", "{}", reply));
        vec![
            Action::SetGroupName { group_id: gid, new_name },
            Action::SendGroupMsg { group_id: gid, reply_id: mid, text: reply },
        ]
    }

    async fn reply_no_perm(&self, gid: i64, uid: i64, mid: i64) -> Option<Vec<Action>> {
        let admins = self.config.lock().await.admin_users.clone();
        (!guche_core::is_superuser(uid, &admins)).then(|| {
            vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: "权限不足".into() }]
        })
    }

    async fn enable(&self, gid: i64, uid: i64, mid: i64) -> Vec<Action> {
        if let Some(a) = self.reply_no_perm(gid, uid, mid).await { return a; }
        let mut cfg = self.config.lock().await;
        if cfg.guche.groups.contains_key(&gid) {
            return vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: "本群已启用固车助手功能".into() }];
        }
        cfg.guche.groups.insert(gid, GroupEntry::default());
        cfg.save(&crate::config::config_path()).await;
        drop(cfg);
        let _ = self.update_tx.send(());
        vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: "已启动本群的自动拾取车牌功能".into() }]
    }

    async fn disable(&self, gid: i64, uid: i64, mid: i64) -> Vec<Action> {
        if let Some(a) = self.reply_no_perm(gid, uid, mid).await { return a; }
        let mut cfg = self.config.lock().await;
        if cfg.guche.groups.remove(&gid).is_none() {
            return vec![]; // 本来就没开，静默
        }
        cfg.save(&crate::config::config_path()).await;
        drop(cfg);
        let _ = self.update_tx.send(());
        vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: "固车助手功能已关闭，下一次冲榜见~".into() }]
    }

    async fn mode_cmd(&self, text: &str, prefix: &str, gid: i64, uid: i64, mid: i64) -> Vec<Action> {
        if let Some(a) = self.reply_no_perm(gid, uid, mid).await { return a; }
        {
            let cfg = self.config.lock().await;
            if !cfg.guche.groups.contains_key(&gid) {
                return vec![]; // 未启用不创建
            }
        }

        let args = text.strip_prefix(prefix).unwrap_or("").trim();

        if args.is_empty() {
            let (pos_end, fuzzy) = {
                let cfg = self.config.lock().await;
                let e = cfg.guche.groups.get(&gid).cloned().unwrap_or_default();
                (e.pos.unwrap_or(cfg.guche.pos_end), e.mode.unwrap_or(cfg.guche.fuzzy))
            };
            return vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: format!(
                "固车模式设置\n━━━━━━━━━━━━━━━━\n\n\
                 {} 前缀    - 车牌放在群名前面\n\
                 {} 后缀    - 车牌放在群名后面\n\
                 {} 严格    - 只识别纯5位数字\n\
                 {} 宽松    - 消息开头提取5位数字\n\
                 {} 前缀 严格  - 同时设置两个\n\n\
                 当前模式：{} + {}",
                prefix, prefix, prefix, prefix, prefix,
                if !pos_end { "前缀" } else { "后缀" },
                if !fuzzy { "严格" } else { "宽松" }
            )}];
        }

        let mut pos = None;
        let mut mode = None;
        for part in args.split_whitespace() {
            match part {
                "前缀" => pos = Some(false),
                "后缀" => pos = Some(true),
                "严格" => mode = Some(false),
                "宽松" => mode = Some(true),
                _ => return vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: format!("未知参数: {}", part) }],
            }
        }

        let mut labels = Vec::new();
        let changed = self.edit_group(gid, |e| {
            if let Some(p) = pos {
                if e.pos != Some(p) {
                    e.pos = Some(p);
                    labels.push(if p { "后缀" } else { "前缀" });
                }
            }
            if let Some(m) = mode {
                if e.mode != Some(m) {
                    e.mode = Some(m);
                    labels.push(if m { "宽松" } else { "严格" });
                }
            }
        }).await;

        if !changed {
            return vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: "设置未变化".into() }];
        }
        let _ = self.update_tx.send(());
        vec![Action::SendGroupMsg { group_id: gid, reply_id: mid, text: format!("已切换为: {}", labels.join(" + ")) }]
    }
}

pub fn extract_text(event: &Value) -> String {
    let msg = match event.get("message") { Some(m) => m, None => return String::new() };
    if let Some(arr) = msg.as_array() {
        return arr.iter().filter_map(|seg| {
            if seg.get("type")?.as_str()? == "text" {
                seg.get("data")?.get("text")?.as_str().map(String::from)
            } else { None }
        }).collect::<Vec<_>>().join("");
    }
    if let Some(s) = msg.as_str() { return s.to_string(); }
    String::new()
}

pub fn build_send_group_msg(gid: i64, rid: i64, text: &str) -> (String, Value) {
    ("send_group_msg".into(), json!({
        "group_id": gid,
        "message": [{"type": "reply", "data": {"id": rid.to_string()}}, {"type": "text", "data": {"text": text}}]
    }))
}

pub fn build_set_group_name(gid: i64, name: &str) -> (String, Value) {
    ("set_group_name".into(), json!({ "group_id": gid, "group_name": name }))
}

pub fn build_get_group_info(gid: i64) -> (String, Value) {
    ("get_group_info".into(), json!({ "group_id": gid }))
}
