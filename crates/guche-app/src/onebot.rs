use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

/// Connection pool with automatic cleanup
pub struct ConnectionPool {
    connections: HashMap<String, ConnectionInfo>,
    max_connections: usize,
    timeout: Duration,
}

struct ConnectionInfo {
    last_activity: Instant,
}

impl ConnectionPool {
    fn new(max_connections: usize, timeout: Duration) -> Self {
        Self {
            connections: HashMap::new(),
            max_connections,
            timeout,
        }
    }
    
    fn add_connection(&mut self, peer: &str) -> bool {
        if self.connections.len() >= self.max_connections {
            return false;
        }

        self.connections.insert(peer.to_string(), ConnectionInfo {
            last_activity: Instant::now(),
        });

        true
    }
    
    fn remove_connection(&mut self, peer: &str) {
        self.connections.remove(peer);
    }
    
    fn update_activity(&mut self, peer: &str) {
        if let Some(info) = self.connections.get_mut(peer) {
            info.last_activity = Instant::now();
        }
    }
    
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let expired: Vec<String> = self.connections.iter()
            .filter(|(_, info)| now.duration_since(info.last_activity) > self.timeout)
            .map(|(peer, _)| peer.clone())
            .collect();
        
        for peer in expired {
            self.connections.remove(&peer);
            log::info!("连接超时，移除: {}", peer);
        }
    }
    
    fn connection_count(&self) -> usize {
        self.connections.len()
    }
}

/// Run the WS server on the configured port (no silent fallback).
/// Validates token if configured. Forwards API calls over OneBot V11.
pub async fn run_server(
    host: &str,
    port: u16,
    token: Option<String>,
    api_tx: Arc<Mutex<Option<mpsc::UnboundedSender<(String, Value, oneshot::Sender<Value>)>>>>,
    event_tx: mpsc::Sender<Value>,
    ws_connected: Arc<tokio::sync::RwLock<bool>>,
    connection_pool_size: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
        format!("WS 服务无法绑定 {}（请确认端口未被占用且配置正确）: {}", bind_addr, e)
    })?;
    log::info!("OneBot V11 反向 WS 服务已启动，监听 {}", bind_addr);
    log::info!("请在 OneBot 客户端中添加反向 WS 连接: ws://{}/ws", bind_addr);
    log::info!("（任意符合 OneBot V11 的客户端均可连接）");

    // Create connection pool
    let pool = Arc::new(Mutex::new(ConnectionPool::new(
        connection_pool_size,
        Duration::from_secs(300), // 5 minute timeout
    )));
    
    // Spawn cleanup task
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut pool = pool_clone.lock().await;
            pool.cleanup_expired();
            log::debug!("连接池清理完成，当前连接数: {}", pool.connection_count());
        }
    });
    
    loop {
        let (stream, peer) = listener.accept().await?;
        log::info!("连接来自 {}", peer);
        
        // Check connection pool
        {
            let mut pool = pool.lock().await;
            if !pool.add_connection(&peer.to_string()) {
                log::warn!("连接池已满，拒绝连接: {}", peer);
                continue;
            }
        }
        
        // Validate token during WS handshake
        let expected_token = token.clone();
        let bind_addr_clone = bind_addr.clone();
        let callback = |req: &Request, response: Response| -> Result<Response, http::Response<Option<String>>> {
            // 检查是否为 WebSocket 升级请求
            let is_ws_upgrade = req.headers()
                .get("upgrade")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.eq_ignore_ascii_case("websocket"))
                .unwrap_or(false)
                && req.headers()
                    .get("connection")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.to_lowercase().contains("upgrade"))
                    .unwrap_or(false);

            if !is_ws_upgrade {
                log::warn!("收到非 WebSocket 请求 (来自 {}): {} {}", peer, req.method(), req.uri());
                let body = format!(
                    "此端口仅支持 WebSocket 连接。\n\n\
                    请在 OneBot 客户端中添加反向 WS 地址：\n\
                    ws://{}/ws\n\n\
                    当前请求：{} {}",
                    bind_addr_clone, req.method(), req.uri()
                );
                let mut resp = http::Response::new(Some(body));
                *resp.status_mut() = http::StatusCode::BAD_REQUEST;
                resp.headers_mut().insert("content-type", "text/plain; charset=utf-8".parse().unwrap());
                return Err(resp);
            }

            if let Some(ref expected) = expected_token {
                let auth_ok = req.headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v == format!("Bearer {}", expected))
                    .unwrap_or(false);

                let query_ok = req.uri()
                    .query()
                    .and_then(|q| {
                        q.split('&')
                            .find(|p| p.starts_with("access_token="))
                            .and_then(|p| p.strip_prefix("access_token="))
                            .map(|v| v == *expected)
                    })
                    .unwrap_or(false);

                if !auth_ok && !query_ok {
                    log::warn!("连接被拒绝: token 验证失败 (来自 {})", peer);
                    let mut resp = http::Response::new(Some("Unauthorized".into()));
                    *resp.status_mut() = http::StatusCode::UNAUTHORIZED;
                    return Err(resp);
                }
            }
            Ok(response)
        };

        let ws_stream = match accept_hdr_async(stream, callback).await {
            Ok(ws) => ws,
            Err(e) => {
                log::warn!("WS 握手失败: {}", e);
                let mut pool = pool.lock().await;
                pool.remove_connection(&peer.to_string());
                continue;
            }
        };

        log::info!("WS 连接建立: {}", peer);
        
        // Update connection status
        {
            let mut connected = ws_connected.write().await;
            *connected = true;
        }
        
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        let mut pending: HashMap<String, (oneshot::Sender<Value>, Instant)> = HashMap::new();
        let mut echo_counter: u64 = 0;
        
        // Clone for the connection task
        let pool_clone = pool.clone();
        let peer_clone = peer.to_string();
        let event_tx_clone = event_tx.clone();
        let ws_connected_clone = ws_connected.clone();
        let api_tx_clone = api_tx.clone();
        // 每个连接创建独立 channel，更新共享 sender
        let (conn_api_tx, mut conn_api_rx) = mpsc::unbounded_channel();
        *api_tx.lock().await = Some(conn_api_tx);
        
        // Spawn connection handler
        tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = cleanup_interval.tick() => {
                        // 清理超时的 pending 请求（30秒未收到回复）
                        let now = Instant::now();
                        let stale: Vec<String> = pending.iter()
                            .filter(|(_, (_, ts))| now.duration_since(*ts) > Duration::from_secs(30))
                            .map(|(echo, _)| echo.clone())
                            .collect();
                        for echo in stale {
                            if let Some((tx, _)) = pending.remove(&echo) {
                                log::debug!("API 请求超时，清理: {}", echo);
                                let _ = tx.send(json!({"error": "timeout"}));
                            }
                        }
                    }
                    msg = ws_receiver.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                let text_str: &str = &text;
                                if let Ok(val) = serde_json::from_str::<Value>(text_str) {
                                    // Update connection activity
                                    {
                                        let mut pool = pool_clone.lock().await;
                                        pool.update_activity(&peer_clone);
                                    }
                                    
                                    if let Some(echo) = val.get("echo").and_then(|e| e.as_str()) {
                                        if let Some((tx, _)) = pending.remove(echo) {
                                            let _ = tx.send(val);
                                        }
                                    } else {
                                        // Use try_send to avoid blocking on bounded channel
                                        if event_tx_clone.try_send(val).is_err() {
                                            log::warn!("消息队列已满，丢弃旧消息");
                                        }
                                    }
                                }
                            }
                            Some(Ok(Message::Ping(data))) => {
                                let _ = ws_sender.send(Message::Pong(data)).await;
                            }
                            Some(Ok(Message::Close(_))) => {
                                log::info!("WS 连接关闭: {}", peer_clone);
                                break;
                            }
                            Some(Err(e)) => {
                                log::error!("WS 错误: {}", e);
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }
                    api_req = conn_api_rx.recv() => {
                        match api_req {
                            Some((action, params, resp_tx)) => {
                                echo_counter += 1;
                                let echo = format!("guche_{}", echo_counter);
                                pending.insert(echo.clone(), (resp_tx, Instant::now()));

                                let msg = json!({
                                    "action": action,
                                    "params": params,
                                    "echo": echo
                                });
                                match serde_json::to_string(&msg) {
                                    Ok(json_str) => {
                                        if ws_sender.send(Message::Text(json_str.into())).await.is_err() {
                                            log::error!("发送 API 请求失败");
                                            if let Some((_, (tx, _))) = pending.remove_entry(&echo) {
                                                let _ = tx.send(json!({"error": "send failed"}));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("JSON序列化失败: {}", e);
                                        if let Some((_, (tx, _))) = pending.remove_entry(&echo) {
                                            let _ = tx.send(json!({"error": "json serialization failed"}));
                                        }
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            
            // Remove connection from pool and update status
            let mut pool = pool_clone.lock().await;
            pool.remove_connection(&peer_clone);
            let connection_count = pool.connection_count();
            drop(pool);
            
            // Update connection status based on remaining connections
            {
                let mut connected = ws_connected_clone.write().await;
                *connected = connection_count > 0;
            }
            *api_tx_clone.lock().await = None;
            
            log::info!("连接已从连接池移除: {}", peer_clone);
        });
    }
}