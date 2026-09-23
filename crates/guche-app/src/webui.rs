use axum::{
    routing::{get, post, delete},
    Router,
    Json,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use futures_util::{SinkExt, StreamExt};


use crate::config::{AppConfig, GroupEntry};
use crate::stats::StatsManager;

pub type ApiTx = tokio::sync::mpsc::UnboundedSender<(String, serde_json::Value, tokio::sync::oneshot::Sender<serde_json::Value>)>;

#[derive(Debug, Clone)]
pub struct WebUiState {
    pub config: Arc<AsyncMutex<AppConfig>>,
    pub log_buffer: Arc<Mutex<VecDeque<String>>>,
    pub stats: Arc<StatsManager>,
    pub restart_tx: tokio::sync::watch::Sender<u32>,
    pub ws_on: Arc<RwLock<bool>>,
    pub update_tx: tokio::sync::broadcast::Sender<()>,
    pub api_tx: Arc<AsyncMutex<Option<ApiTx>>>,
}

impl WebUiState {
    pub fn new(
        config: Arc<AsyncMutex<AppConfig>>,
        log_buffer: Arc<Mutex<VecDeque<String>>>,
        stats: Arc<StatsManager>,
        restart_tx: tokio::sync::watch::Sender<u32>,
        ws_on: Arc<RwLock<bool>>,
        update_tx: tokio::sync::broadcast::Sender<()>,
        api_tx: Arc<AsyncMutex<Option<ApiTx>>>,
    ) -> Self {
        Self { config, log_buffer, stats, restart_tx, ws_on, update_tx, api_tx }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigResponse {
    pub config: AppConfig,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// 一组群：{group_id, pos, mode, name}；pos/mode 为 true/false/null
fn group_json(gid: i64, e: &GroupEntry) -> serde_json::Value {
    serde_json::json!({ "group_id": gid, "pos": e.pos, "mode": e.mode, "name": e.name })
}

pub async fn run_webui(
    config: Arc<AsyncMutex<AppConfig>>,
    log_buffer: Arc<Mutex<VecDeque<String>>>,
    stats: Arc<StatsManager>,
    restart_tx: tokio::sync::watch::Sender<u32>,
    ws_on: Arc<RwLock<bool>>,
    update_tx: tokio::sync::broadcast::Sender<()>,
    api_tx: Arc<AsyncMutex<Option<ApiTx>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = WebUiState::new(config, log_buffer, stats, restart_tx, ws_on, update_tx, api_tx);
    let st = state.clone();

    let app = Router::new()
        .route("/api/config", get(get_config).put(update_config))
        .route("/api/config/reset", post(reset_config))
        .route("/api/groups", get(get_groups).post(add_group))
        .route("/api/groups/sync-names", post(sync_group_names))
        .route("/api/groups/:id", get(get_group_detail).delete(delete_group).patch(patch_group))
        .route("/api/groups/:id/reset", post(reset_group_override))
        .route("/api/stats", get(get_stats).delete(delete_all_stats))
        .route("/api/stats/:group_id", get(get_group_stats).delete(delete_group_stats))
        .route("/api/stats/:group_id/range", delete(delete_group_stats_range))
        .route("/api/stats/export", get(export_stats))
        .route("/api/logs", get(get_logs))
        .route("/api/logs/stream", get(logs_stream))
        .route("/api/status", get(get_status))
        .with_state(state)
        .fallback(static_files);

    // 配置里填什么端口就监听什么端口
    let port = {
        let cfg = st.config.lock().await;
        cfg.webui.port
    };
    let addr = format!("127.0.0.1:{}", port);
    log::info!("管理界面: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        format!("管理界面无法绑定 {}（请确认端口未被占用且配置正确）: {}", addr, e)
    })?;
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    if let Err(e) = server.await { log::error!("管理界面错误: {}", e); }
    log::info!("管理界面已关闭");
    Ok(())
}

// 编译时嵌入 HTML
static INDEX_HTML: &str = include_str!("../../../webui/index.html");

// 静态文件服务（SPA fallback）
async fn static_files(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path();
    if path == "/" || path.is_empty() || !path.contains('.') {
        return serve_index().await;
    }
    serve_index().await
}

async fn serve_index() -> axum::response::Response {
    axum::response::Html(INDEX_HTML).into_response()
}

// 退出信号
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let term = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler").recv().await;
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    log::info!("收到退出信号");
}

async fn get_config(State(state): State<WebUiState>) -> impl IntoResponse {
    let config = state.config.lock().await;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    Json(ConfigResponse {
        config: config.clone(),
        timestamp,
    })
}

async fn update_config(
    State(state): State<WebUiState>,
    Json(update): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut config = state.config.lock().await;
    
    // 合并更新配置
    if let Some(ob) = update.get("onebot") {
        if let Some(host) = ob.get("host").and_then(|v| v.as_str()) {
            config.onebot.host = host.to_string();
        }
        if let Some(port) = ob.get("port").and_then(|v| v.as_u64()) {
            config.onebot.port = port as u16;
        }
        if ob.get("token").is_some() {
            config.onebot.token = ob.get("token").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
    }
    
    if let Some(guche) = update.get("guche") {
        if let Some(v) = guche.get("fuzzy") { config.guche.fuzzy = v.as_bool().unwrap_or(config.guche.fuzzy); }
        if let Some(v) = guche.get("pos_end") { config.guche.pos_end = v.as_bool().unwrap_or(config.guche.pos_end); }
        if let Some(cooldown) = guche.get("cooldown_seconds").and_then(|v| v.as_f64()) {
            config.guche.cooldown_seconds = cooldown;
        }
    }
    
    if let Some(webui) = update.get("webui") {
        if let Some(port) = webui.get("port").and_then(|v| v.as_u64()) {
            config.webui.port = port as u16;
        }
    }
    
    if let Some(admin_users) = update.get("admin_users") {
        if let Some(arr) = admin_users.as_array() {
            config.admin_users = arr.iter()
                .filter_map(|v| v.as_i64())
                .collect();
        }
    }
    
    if let Some(stats) = update.get("stats") {
        if let Some(max_records) = stats.get("max_records_per_group").and_then(|v| v.as_u64()) {
            config.stats.max_records_per_group = max_records as usize;
        }
    }
    
    if let Some(buffers) = update.get("buffers") {
        if let Some(size) = buffers.get("log_buffer_size").and_then(|v| v.as_u64()) {
            config.buffers.log_buffer_size = size as usize;
        }
        if let Some(size) = buffers.get("message_queue_size").and_then(|v| v.as_u64()) {
            config.buffers.message_queue_size = size as usize;
        }
        if let Some(size) = buffers.get("connection_pool_size").and_then(|v| v.as_u64()) {
            config.buffers.connection_pool_size = size as usize;
        }
    }
    
    if let Some(commands) = update.get("commands") {
        if let Some(enable) = commands.get("guche_enable").and_then(|v| v.as_str()) {
            config.commands.guche_enable = enable.to_string();
        }
        if let Some(disable) = commands.get("guche_disable").and_then(|v| v.as_str()) {
            config.commands.guche_disable = disable.to_string();
        }
        if let Some(help) = commands.get("guche_help").and_then(|v| v.as_str()) {
            config.commands.guche_help = help.to_string();
        }
        if let Some(mode) = commands.get("guche_mode").and_then(|v| v.as_str()) {
            config.commands.guche_mode = mode.to_string();
        }
    }
    
    // Validate config
    if let Err(e) = config.validate() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response();
    }
    
    // Save to file
    let path = crate::config::config_path();
    config.save(&path).await;
    
    // Log detailed config changes
    let mut changes = Vec::new();
    if update.get("onebot").is_some() {
        changes.push(format!("onebot: host={}, port={}, token={}", 
            config.onebot.host, config.onebot.port, 
            config.onebot.token.as_deref().unwrap_or("无")));
    }
    if update.get("guche").is_some() {
        changes.push(format!("guche: fuzzy={}, pos_end={}, cooldown={}s",
            config.guche.fuzzy, config.guche.pos_end, config.guche.cooldown_seconds));
    }
    if update.get("webui").is_some() {
        changes.push(format!("webui: port={}", config.webui.port));
    }
    if update.get("stats").is_some() {
        changes.push(format!("stats: max_records={}", config.stats.max_records_per_group));
    }
    if update.get("buffers").is_some() {
        changes.push(format!("buffers: log={}, queue={}, pool={}", 
            config.buffers.log_buffer_size, config.buffers.message_queue_size, config.buffers.connection_pool_size));
    }
    if update.get("commands").is_some() {
        changes.push(format!("commands: enable={}, disable={}, help={}, mode={}", 
            config.commands.guche_enable, config.commands.guche_disable, 
            config.commands.guche_help, config.commands.guche_mode));
    }
    if update.get("admin_users").is_some() {
        changes.push(format!("admin_users: {:?}", config.admin_users));
    }
    
    if changes.is_empty() {
        log::info!("配置已保存（无变更）");
    } else {
        log::info!("配置已保存: {}", changes.join("; "));
    }
    
    // onebot 变更时重启 WS
    if update.get("onebot").is_some() {
        let _ = state.restart_tx.send(1);
        log::info!("WS 服务将重启");
    }
    
    // 通知前端配置已更新
    let _ = state.update_tx.send(());
    
    StatusCode::OK.into_response()
}

async fn reset_config(State(state): State<WebUiState>) -> impl IntoResponse {
    let mut cfg = state.config.lock().await;
    let def = AppConfig::default();
    *cfg = def.clone();
    def.save(&crate::config::config_path()).await;
    let _ = state.restart_tx.send(1);
    log::info!("配置已重置");
    // 通知前端配置已更新
    let _ = state.update_tx.send(());
    StatusCode::OK.into_response()
}

async fn get_groups(State(state): State<WebUiState>) -> impl IntoResponse {
    let cfg = state.config.lock().await;
    Json(cfg.guche.groups.iter().map(|(g, e)| group_json(*g, e)).collect::<Vec<_>>())
}

/// body: {"pos": true|false|null, "mode": true|false|null}，只改出现的键
async fn patch_group(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut cfg = state.config.lock().await;
    let Some(e) = cfg.guche.groups.get_mut(&group_id) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "群未启用"}))).into_response();
    };
    if let Some(v) = body.get("pos") { e.pos = v.as_bool(); }
    if let Some(v) = body.get("mode") { e.mode = v.as_bool(); }
    cfg.save(&crate::config::config_path()).await;
    let _ = state.update_tx.send(());
    StatusCode::OK.into_response()
}

async fn reset_group_override(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    patch_group(State(state), axum::extract::Path(group_id), Json(serde_json::json!({"pos": null, "mode": null}))).await
}

async fn get_group_detail(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let cfg = state.config.lock().await;
    match cfg.guche.groups.get(&group_id) {
        Some(e) => Json(group_json(group_id, e)),
        None => Json(serde_json::json!({ "group_id": group_id, "pos": null, "mode": null, "name": null })),
    }
}

/// 通过 OneBot 拉取群名，只写 name
async fn sync_group_names(State(state): State<WebUiState>) -> impl IntoResponse {
    let api = state.api_tx.lock().await.clone();
    let Some(api) = api else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "OneBot 未连接"}))).into_response();
    };

    let gids: Vec<i64> = state.config.lock().await.guche.groups.keys().cloned().collect();
    let mut updated = 0;
    for gid in gids {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if api.send(("get_group_info".into(), serde_json::json!({ "group_id": gid }), tx)).is_err() {
            continue;
        }
        if let Ok(Ok(val)) = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            if let Some(name) = val.get("data").and_then(|d| d.get("group_name")).and_then(|g| g.as_str()) {
                let mut cfg = state.config.lock().await;
                if let Some(e) = cfg.guche.groups.get_mut(&gid) {
                    if e.name.as_deref() != Some(name) {
                        e.name = Some(name.to_string());
                        updated += 1;
                    }
                }
            }
        }
    }
    if updated > 0 {
        state.config.lock().await.save(&crate::config::config_path()).await;
    }
    let _ = state.update_tx.send(());
    Json(serde_json::json!({ "updated": updated })).into_response()
}

async fn add_group(
    State(state): State<WebUiState>,
    Json(group_id): Json<i64>,
) -> impl IntoResponse {
    let mut cfg = state.config.lock().await;
    cfg.guche.groups.entry(group_id).or_default();
    cfg.save(&crate::config::config_path()).await;
    let _ = state.update_tx.send(());
    StatusCode::OK.into_response()
}

async fn delete_group(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let mut cfg = state.config.lock().await;
    cfg.guche.groups.remove(&group_id);
    cfg.save(&crate::config::config_path()).await;
    let _ = state.update_tx.send(());
    StatusCode::OK.into_response()
}

async fn get_stats(
    State(state): State<WebUiState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let start_date = params.get("start_date").cloned().unwrap_or_default();
    let end_date = params.get("end_date").cloned().unwrap_or_default();
    let group_id = params.get("group_id").and_then(|s| s.parse::<i64>().ok());
    
    let groups = state.stats.get_all_groups().await;
    let mut all_groups_stats = Vec::new();
    let mut total_renames = 0;
    let mut today_renames = 0;
    let mut range_renames = 0;
    
    for &gid in &groups {
        if let Some(filter_gid) = group_id {
            if gid != filter_gid {
                continue;
            }
        }
        
        let group_stats = state.stats.load_group(gid).await;
        total_renames += group_stats.records.len();
        today_renames += group_stats.count_today();
        
        if !start_date.is_empty() && !end_date.is_empty() {
            range_renames += group_stats.count_in_range(&start_date, &end_date);
        }
        
        all_groups_stats.push(serde_json::json!({
            "group_id": gid,
            "rename_count": group_stats.records.len(),
            "today_count": group_stats.count_today(),
            "last_rename": group_stats.records.last().map(|r| r.time.clone()),
        }));
    }
    
    let response = serde_json::json!({
        "total_renames": total_renames,
        "today_renames": today_renames,
        "range_renames": range_renames,
        "start_date": start_date,
        "end_date": end_date,
        "groups": all_groups_stats
    });
    
    Json(response)
}

async fn get_group_stats(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let start_date = params.get("start_date").cloned().unwrap_or_default();
    let end_date = params.get("end_date").cloned().unwrap_or_default();
    
    let group_stats = state.stats.load_group(group_id).await;
    
    let records = if !start_date.is_empty() && !end_date.is_empty() {
        group_stats.get_records_in_range(&start_date, &end_date)
    } else {
        group_stats.records.iter().collect()
    };
    
    Json(serde_json::json!({
        "group_id": group_id,
        "rename_count": group_stats.records.len(),
        "today_count": group_stats.count_today(),
        "last_rename": group_stats.records.last().map(|r| r.time.clone()),
        "records": records,
    }))
}

async fn delete_group_stats(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    state.stats.clear_group(group_id).await;
    StatusCode::OK
}

async fn delete_group_stats_range(
    State(state): State<WebUiState>,
    axum::extract::Path(group_id): axum::extract::Path<i64>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let start_date = params.get("start_date").cloned().unwrap_or_default();
    let end_date = params.get("end_date").cloned().unwrap_or_default();
    
    if start_date.is_empty() || end_date.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "start_date and end_date are required"}))).into_response();
    }
    
    let deleted = state.stats.delete_records_in_range(group_id, &start_date, &end_date).await;
    Json(serde_json::json!({"deleted": deleted})).into_response()
}

async fn delete_all_stats(State(state): State<WebUiState>) -> impl IntoResponse {
    state.stats.clear_all().await;
    StatusCode::OK
}

async fn export_stats(State(state): State<WebUiState>) -> impl IntoResponse {
    let groups = state.stats.get_all_groups().await;
    let mut all_stats = Vec::new();
    
    for &gid in &groups {
        let group_stats = state.stats.load_group(gid).await;
        all_stats.push(serde_json::json!({
            "group_id": gid,
            "records": group_stats.records,
        }));
    }
    
    let export = serde_json::json!({
        "export_time": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "total_groups": groups.len(),
        "groups": all_stats
    });
    
    Json(export)
}

async fn get_logs(
    State(state): State<WebUiState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let level_filter = params.get("level").cloned().unwrap_or_default();
    let target_filter = params.get("target").cloned().unwrap_or_default();
    let keyword_filter = params.get("keyword").cloned().unwrap_or_default();

    let logs: Vec<LogEntry> = {
        let log_buffer = state.log_buffer.lock().unwrap();
        log_buffer
            .iter()
            .filter_map(|line| parse_log_line(line))
            .filter(|entry| {
                (level_filter.is_empty() || entry.level.to_lowercase() == level_filter.to_lowercase())
                && (target_filter.is_empty() || entry.target.to_lowercase().contains(&target_filter.to_lowercase()))
                && (keyword_filter.is_empty() || entry.message.to_lowercase().contains(&keyword_filter.to_lowercase()))
            })
            .collect()
    };
    
    Json(serde_json::json!({
        "total": logs.len(),
        "filters": {
            "level": level_filter,
            "target": target_filter,
            "keyword": keyword_filter
        },
        "logs": logs
    }))
}

async fn logs_stream(
    ws: WebSocketUpgrade,
    State(state): State<WebUiState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, state: WebUiState) {
    let (mut sender, mut receiver) = socket.split();

    let initial_logs = {
        let log_buffer = state.log_buffer.lock().unwrap();
        log_buffer.clone()
    };

    for log_line in initial_logs {
        if let Some(entry) = parse_log_line(&log_line) {
            if let Ok(log_json) = serde_json::to_string(&entry) {
                if sender.send(Message::Text(log_json)).await.is_err() {
                    return;
                }
            }
        }
    }

    let mut last_log_count = {
        let log_buffer = state.log_buffer.lock().unwrap();
        log_buffer.len()
    };
    
    let mut update_rx = state.update_tx.subscribe();
    
    loop {
        tokio::select! {
            // 处理配置更新通知
            result = update_rx.recv() => {
                match result {
                    Ok(()) => {
                        // 发送配置更新消息给前端
                        let update_msg = serde_json::json!({
                            "type": "config_updated",
                            "timestamp": std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        });
                        if sender.send(Message::Text(update_msg.to_string())).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        // 发送错误，继续循环
                    }
                }
            }
            // 处理日志更新
            _ = async {
                let new_logs = {
                    let log_buffer = state.log_buffer.lock().unwrap();
                    if log_buffer.len() > last_log_count {
                        let new_logs: Vec<String> = log_buffer.iter().skip(last_log_count).cloned().collect();
                        last_log_count = log_buffer.len();
                        new_logs
                    } else {
                        Vec::new()
                    }
                };
                
                for log_line in new_logs {
                    if let Some(entry) = parse_log_line(&log_line) {
                        if let Ok(log_json) = serde_json::to_string(&entry) {
                            if sender.send(Message::Text(log_json)).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                
                // 处理WebSocket消息
                if let Some(msg) = receiver.next().await {
                    match msg {
                        Ok(Message::Ping(_)) => {
                            if sender.send(Message::Pong(vec![])).await.is_err() {
                                return;
                            }
                        }
                        Ok(Message::Close(_)) | Err(_) => {
                            return;
                        }
                        _ => {}
                    }
                }
                
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            } => {}
        }
    }
}

fn parse_log_line(line: &str) -> Option<LogEntry> {
    let end_bracket = line.find(']')?;
    let header = &line[1..end_bracket];
    let message = line.get(end_bracket + 2..).unwrap_or("");

    // header: "YYYY-MM-DD HH:MM:SS LEVEL target"（兼容旧的 "HH:MM:SS LEVEL target"）
    let parts: Vec<&str> = header.split_whitespace().collect();
    let (timestamp, level, target) = if parts.len() >= 4 && parts[1].contains(':') {
        (format!("{} {}", parts[0], parts[1]), parts[2].to_string(), parts[3].to_string())
    } else if parts.len() >= 3 {
        (parts[0].to_string(), parts[1].to_string(), parts[2].to_string())
    } else {
        return None;
    };

    Some(LogEntry { timestamp, level, target, message: message.to_string() })
}

async fn get_status(State(state): State<WebUiState>) -> impl IntoResponse {
    let cfg = state.config.lock().await;
    let groups_len = cfg.guche.groups.len();
    let log_len = state.log_buffer.lock().map(|b| b.len()).unwrap_or(0);
    let ws = state.ws_on.read().await;

    Json(serde_json::json!({
        "websocket_connected": *ws,
        "enabled_groups": groups_len,
        "enabled_groups_count": groups_len,
        "log_buffer": log_len,
        "log_capacity": cfg.buffers.log_buffer_size,
        "queue_size": cfg.buffers.message_queue_size,
        "pool_size": cfg.buffers.connection_pool_size,
        "today_renames": state.stats.count_today().await,
        "total_renames": state.stats.total_count().await,
        "webui_port": cfg.webui.port,
        "fuzzy": cfg.guche.fuzzy,
        "pos_end": cfg.guche.pos_end,
        "cooldown": cfg.guche.cooldown_seconds,
        "admin_count": cfg.admin_users.len(),
        "onebot_host": cfg.onebot.host,
        "onebot_port": cfg.onebot.port,
    }))
}