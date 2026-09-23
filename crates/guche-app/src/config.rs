use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.json")
}

/// Strip `//` line comments from JSON text (standard JSON doesn't support comments).
pub fn strip_json_comments(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            if let Some(pos) = line.find("//") {
                let before = &line[..pos];
                if before.chars().filter(|c| *c == '"').count() % 2 == 0 {
                    before
                } else {
                    line
                }
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OneBotConfig {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
}
impl Default for OneBotConfig {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 8901, token: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandsConfig {
    pub guche_enable: String,
    pub guche_disable: String,
    pub guche_help: String,
    pub guche_mode: String,
}
impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            guche_enable: "/开启固车拾取".into(),
            guche_disable: "/关闭固车拾取".into(),
            guche_help: "/固车拾取 使用方法".into(),
            guche_mode: "/固车模式".into(),
        }
    }
}

/// 单群：None=跟全局。pos: false=前缀,true=后缀；mode: false=严格,true=宽松
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GroupEntry {
    pub pos: Option<bool>,
    pub mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GucheConfig {
    pub fuzzy: bool,
    pub pos_end: bool,
    pub cooldown_seconds: f64,
    /// 键存在 = 已启用
    pub groups: HashMap<i64, GroupEntry>,
}
impl Default for GucheConfig {
    fn default() -> Self {
        Self { fuzzy: false, pos_end: false, cooldown_seconds: 5.0, groups: HashMap::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsConfig {
    pub max_records_per_group: usize,
}
impl Default for StatsConfig {
    fn default() -> Self {
        Self { max_records_per_group: 1024 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebUiConfig {
    pub enabled: bool,
    pub port: u16,
}
impl Default for WebUiConfig {
    fn default() -> Self {
        Self { enabled: true, port: 8080 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BuffersConfig {
    pub log_buffer_size: usize,
    pub message_queue_size: usize,
    pub connection_pool_size: usize,
}
impl Default for BuffersConfig {
    fn default() -> Self {
        Self { log_buffer_size: 1024, message_queue_size: 512, connection_pool_size: 10 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub onebot: OneBotConfig,
    pub admin_users: Vec<i64>,
    pub commands: CommandsConfig,
    pub guche: GucheConfig,
    pub stats: StatsConfig,
    pub webui: WebUiConfig,
    pub buffers: BuffersConfig,
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            log::info!("config.json 不存在，使用内置默认配置");
            return Self::default();
        }
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                log::error!("读取 config.json 失败: {}，使用默认配置", e);
                return Self::default();
            }
        };
        match serde_json::from_str::<Self>(&strip_json_comments(&raw)) {
            Ok(c) => c,
            Err(e) => {
                log::error!("config.json 解析失败: {}，使用默认配置（原文件已保留，未覆盖）", e);
                Self::default()
            }
        }
    }

    pub async fn save(&self, path: &Path) {
        match serde_json::to_string_pretty(self) {
            Ok(json) => match tokio::fs::write(path, &json).await {
                Ok(_) => log::info!("配置已保存到: {:?}", path),
                Err(e) => log::error!("保存配置文件失败: {} (路径: {:?})", e, path),
            },
            Err(e) => log::error!("序列化配置失败: {}", e),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.onebot.port == 0 {
            return Err("onebot port cannot be 0".into());
        }
        if self.guche.cooldown_seconds < 0.0 {
            return Err("cooldown_seconds must be non-negative".into());
        }
        if self.webui.port == 0 {
            return Err("webui port cannot be 0".into());
        }
        Ok(())
    }
}
