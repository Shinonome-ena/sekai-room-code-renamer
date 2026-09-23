mod config;
mod guche;
mod help;
mod onebot;
mod stats;
mod utils;
mod webui;

use config::AppConfig;
use guche::{Action, GucheEngine};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

// API 通道类型别名，简化签名
type ApiTx = webui::ApiTx;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetConsoleTitleW(lpConsoleTitle: *const u16) -> i32;
}

fn base_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn data_dir() -> PathBuf {
    base_dir().join("data")
}

fn data_path(name: &str) -> PathBuf {
    data_dir().join(name)
}

#[tokio::main]
async fn main() {
    // Windows 设置控制台标题
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let title = OsStr::new("世界计划固车助手").encode_wide().chain(Some(0)).collect::<Vec<u16>>();
        unsafe { SetConsoleTitleW(title.as_ptr()); }
    }
    
    // 创建数据目录
    let _ = std::fs::create_dir_all(data_dir());

    // 加载配置
    let cfg_path = config::config_path();
    let config = AppConfig::load(&cfg_path);
    let buf_size = config.buffers.log_buffer_size;

    // 日志器：stderr + 文件 + WebUI 缓冲
    let log_path = data_path("guche.log");
    let log_file = std::sync::Mutex::new(
        std::fs::OpenOptions::new().create(true).append(true).open(&log_path).ok()
    );
    let log_buf = Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(buf_size)));
    let logger_buf = log_buf.clone();

    struct DualLogger {
        file: std::sync::Mutex<Option<std::fs::File>>,
        buffer: Arc<std::sync::Mutex<VecDeque<String>>>,
        max: usize,
    }
    impl log::Log for DualLogger {
        fn enabled(&self, _: &log::Metadata) -> bool { true }
        fn log(&self, record: &log::Record) {
            let t = utils::now_datetime();
            let line = format!("[{} {} {}] {}", t, record.level(), record.target(), record.args());
            eprintln!("{}", line);
            if let Ok(mut f) = self.file.lock() {
                if let Some(ref mut f) = *f {
                    use std::io::Write;
                    let _ = writeln!(f, "{}", line);
                }
            }
            if let Ok(mut buf) = self.buffer.lock() {
                buf.push_back(line);
                if buf.len() > self.max { buf.pop_front(); }
            }
        }
        fn flush(&self) {}
    }
    log::set_logger(Box::leak(Box::new(DualLogger {
        file: log_file,
        buffer: logger_buf,
        max: buf_size,
    }))).ok();
    log::set_max_level(log::LevelFilter::Info);

    let port = config.onebot.port;
    let host = config.onebot.host.clone();
    let queue_size = config.buffers.message_queue_size;
    let cfg = Arc::new(tokio::sync::Mutex::new(config));
    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    let (update_tx, _) = tokio::sync::broadcast::channel(16);
    let engine = Arc::new(GucheEngine::new(cfg.clone(), log_tx, data_dir(), update_tx.clone()).await);

    // 共享状态
    let ws_on = Arc::new(tokio::sync::RwLock::new(false));
    // API 通道（WS 连接建立后更新；WebUI 同步群名也用）
    let api_tx: Arc<Mutex<Option<ApiTx>>> = Arc::new(Mutex::new(None));

    // 日志桥接
    tokio::spawn(async move {
        let mut rx = log_rx;
        while let Some(e) = rx.recv().await {
            log::info!("[{}] {}", e.tag, e.message);
        }
    });

    // 迁移旧 enabled_groups.json → config.guche.groups（一份存储）
    let eg_path = data_path("enabled_groups.json");
    if eg_path.exists() {
        if let Ok(data) = std::fs::read_to_string(&eg_path) {
            let mut migrated = 0usize;
            {
                let mut c = cfg.lock().await;
                if let Ok(map) = serde_json::from_str::<std::collections::HashMap<i64, config::GroupEntry>>(&data) {
                    for (gid, e) in map {
                        c.guche.groups.entry(gid).or_insert(e);
                        migrated += 1;
                    }
                } else if let Ok(ids) = serde_json::from_str::<Vec<i64>>(&data) {
                    for gid in ids {
                        c.guche.groups.entry(gid).or_default();
                        migrated += 1;
                    }
                }
                if migrated > 0 {
                    c.save(&cfg_path).await;
                }
            }
            if migrated > 0 {
                let _ = std::fs::rename(&eg_path, eg_path.with_extension("json.bak"));
                log::info!("已迁移 {} 个群到 config.guche.groups", migrated);
            }
        }
    }
    {
        let c = cfg.lock().await;
        log::info!("启用群: {}", c.guche.groups.len());
    }

    log::info!("世界计划固车助手 Sekai Guche Assistant v0.1.0");
    log::info!("启动, 监听 {}:{}", host, port);
    log::info!("添加反向 WS: ws://{}:{}/ws", host, port);

    // WS 重启信号
    let (restart_tx, mut restart_rx) = tokio::sync::watch::channel(0u32);
    let restart_tx2 = restart_tx.clone();

    // 启动 WebUI
    {
        let c = cfg.lock().await;
        let enabled = c.webui.enabled;
        let wport = c.webui.port;
        drop(c);
        if enabled {
            log::info!("管理界面已启用，端口: {}", wport);
            let cfg2 = cfg.clone();
            let buf2 = log_buf.clone();
            let stats2 = engine.stats.clone();
            let ws2 = ws_on.clone();
            let update_tx2 = update_tx.clone();
            let api2 = api_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = webui::run_webui(cfg2, buf2, stats2, restart_tx2, ws2, update_tx2, api2).await {
                    log::error!("管理界面启动失败: {}", e);
                }
            });
        }
    }

    // 消息队列
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Value>(queue_size);

    // WS 服务（支持重启）
    let ws_cfg = cfg.clone();
    let ws_event_tx = event_tx.clone();
    let ws_api_tx = api_tx.clone();
    let ws_on2 = ws_on.clone();
    tokio::spawn(async move {
        let mut shutdown: Option<tokio::sync::oneshot::Sender<()>> = None;
        let mut count = 0u32;
        
        // 启动 WS 服务
        let start = |c: &AppConfig, evt_tx: tokio::sync::mpsc::Sender<Value>, api: Arc<Mutex<Option<ApiTx>>>, shut: &mut Option<_>, on: Arc<tokio::sync::RwLock<bool>>, pool_size: usize| {
            let host = c.onebot.host.clone();
            let port = c.onebot.port;
            let token = c.onebot.token.clone();
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            *shut = Some(tx);
            
            let api2 = api.clone();
            let on2 = on.clone();
            tokio::spawn(async move {
                tokio::select! {
                    r = onebot::run_server(&host, port, token, api2, evt_tx, on2, pool_size) => {
                        if let Err(e) = r {
                            log::error!("WS 服务错误: {}", e);
                        }
                    }
                    _ = rx => log::info!("WS 服务已停止（配置变更）"),
                }
            });
        };
        
        // 首次启动
        {
            let c = ws_cfg.lock().await;
            start(&c, ws_event_tx.clone(), ws_api_tx.clone(), &mut shutdown, ws_on2.clone(), c.buffers.connection_pool_size);
        }
        
        // 等待重启信号
        while restart_rx.changed().await.is_ok() {
            count += 1;
            log::info!("收到配置变更信号，重启WS服务... (第{}次)", count);
            if let Some(tx) = shutdown.take() {
                let _ = tx.send(());
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            *ws_on2.write().await = false;
            let c = ws_cfg.lock().await;
            start(&c, ws_event_tx.clone(), ws_api_tx.clone(), &mut shutdown, ws_on2.clone(), c.buffers.connection_pool_size);
        }
    });

    // 事件处理循环
    while let Some(event) = event_rx.recv().await {
        let actions = engine.handle_event(&event).await;
        for action in actions {
            match action {
                Action::GetGroupInfo { group_id, code } => {
                    let (a, p) = guche::build_get_group_info(group_id);
                    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                    if let Some(ref tx) = *api_tx.lock().await { let _ = tx.send((a, p, resp_tx)); }

                    if let Ok(Ok(val)) = tokio::time::timeout(std::time::Duration::from_secs(10), resp_rx).await {
                        if let Some(name) = val.get("data").and_then(|d| d.get("group_name")).and_then(|g| g.as_str()) {
                            let uid = event.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            let mid = event.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            for act in engine.handle_rename(group_id, name, &code, uid, mid).await {
                                if let Some(ref tx) = *api_tx.lock().await { send(tx, &act); }
                            }
                        }
                    }
                }
                _ => {
                    if let Some(ref tx) = *api_tx.lock().await { send(tx, &action); }
                }
            }
        }
    }
    
    log::info!("程序退出");
}

fn send(api_tx: &ApiTx, action: &Action) {
    let (a, p) = match action {
        Action::SetGroupName { group_id, new_name } => guche::build_set_group_name(*group_id, new_name),
        Action::SendGroupMsg { group_id, reply_id, text } => guche::build_send_group_msg(*group_id, *reply_id, text),
        _ => return,
    };
    let name = a.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = api_tx.send((a, p, tx));
    // 异步记录响应
    tokio::spawn(async move {
        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(Ok(val)) => {
                if let Some(code) = val.get("retcode").and_then(|v| v.as_i64()) {
                    if code != 0 {
                        let msg = val.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown");
                        log::warn!("API {} 失败: retcode={} msg={}", name, code, msg);
                    }
                }
            }
            Ok(Err(_)) => log::debug!("API {} 通道已关闭", name),
            Err(_) => log::warn!("API {} 超时", name),
        }
    });
}