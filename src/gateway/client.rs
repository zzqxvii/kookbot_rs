//! Gateway WebSocket 客户端

use crate::core::error::{BotError, Result};
use crate::gateway::events::{parse_event, Event, EventHandler};
use crate::gateway::protocol::{GatewayMessage, Intents, SessionInfo, SignalType};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

/// Gateway 客户端
pub struct GatewayClient {
    token: String,
    intents: u32,
    ws_stream: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    session_info: Arc<RwLock<SessionInfo>>,
    event_handler: Arc<RwLock<Option<Box<dyn EventHandler>>>>,
    running: Arc<RwLock<bool>>,
    heartbeat_interval: Arc<RwLock<u64>>,
}

impl GatewayClient {
    pub fn new(token: impl Into<String>, intents: u32) -> Self {
        Self {
            token: token.into(),
            intents,
            ws_stream: Arc::new(RwLock::new(None)),
            session_info: Arc::new(RwLock::new(SessionInfo::default())),
            event_handler: Arc::new(RwLock::new(None)),
            running: Arc::new(RwLock::new(false)),
            heartbeat_interval: Arc::new(RwLock::new(30000)),
        }
    }

    pub fn with_basic_intents(token: impl Into<String>) -> Self {
        info!("创建 Gateway 客户端，使用 BASIC intents: {}", Intents::BASIC);
        Self::new(token, Intents::BASIC)
    }

    pub fn with_all_intents(token: impl Into<String>) -> Self {
        info!("创建 Gateway 客户端，使用 ALL intents: {}", Intents::ALL);
        Self::new(token, Intents::ALL)
    }

    pub async fn set_event_handler(&self, handler: Box<dyn EventHandler>) {
        info!("设置事件处理器");
        *self.event_handler.write().await = Some(handler);
    }

    pub async fn connect(&self, gateway_url: &str) -> Result<()> {
        info!("========================================");
        info!("正在连接到 Kook Gateway");
        info!("URL: {}", gateway_url);
        info!("========================================");

        let (ws_stream, response) = connect_async(gateway_url)
            .await
            .map_err(|e| {
                error!("WebSocket 连接失败: {}", e);
                BotError::GatewayError(format!("连接失败: {}", e))
            })?;

        info!("WebSocket 连接已建立");
        info!("HTTP 响应状态: {:?}", response.status());

        *self.ws_stream.write().await = Some(ws_stream);
        *self.running.write().await = true;

        info!("连接完成，开始监听消息...");
        Ok(())
    }

    pub async fn run(&self) -> Result<()> {
        info!("========================================");
        info!("Gateway 客户端开始运行");
        info!("Token 前8位: {}...", &self.token.chars().take(8).collect::<String>());
        info!("Intents: {}", self.intents);
        info!("========================================");

        // 检查 WebSocket 流是否存在
        let stream_exists = self.ws_stream.read().await.is_some();
        info!("WebSocket 流状态: {}", if stream_exists { "存在" } else { "不存在" });
        
        if !stream_exists {
            error!("WebSocket 流不存在，无法运行");
            return Err(BotError::GatewayError("WebSocket 流不存在".to_string()));
        }

        // 等待第一个消息（Hello），最多等待10秒
        info!("等待服务器 Hello 消息...");
        let mut hello_received = false;
        let hello_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        
        while tokio::time::Instant::now() < hello_deadline {
            if let Some(msg) = self.receive_message().await {
                self.handle_message(msg).await;
                // 检查是否收到了 Hello（heartbeat_interval 会被设置）
                if *self.heartbeat_interval.read().await > 0 {
                    hello_received = true;
                    info!("✓ 收到 Hello 消息，连接正常");
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        if !hello_received {
            error!("等待 Hello 消息超时（10秒），连接可能失败");
            return Err(BotError::GatewayError("未收到 Hello 消息".to_string()));
        }

        let mut heartbeat_tick = interval(Duration::from_secs(30));
        let mut need_heartbeat = true;
        let mut message_count = 0u32;

        loop {
            if !*self.running.read().await {
                warn!("Gateway 连接已断开，退出运行循环");
                break;
            }

            tokio::select! {
                _ = heartbeat_tick.tick() => {
                    if need_heartbeat {
                        self.send_heartbeat().await;
                    }
                }

                message_result = self.receive_message() => {
                    message_count += 1;
                    if let Some(msg) = message_result {
                        self.handle_message(msg).await;
                    }
                }
            }
        }

        info!("Gateway 客户端停止运行，共接收 {} 条消息", message_count);
        Ok(())
    }

    async fn receive_message(&self) -> Option<Message> {
        let mut stream_guard = self.ws_stream.write().await;
        
        if let Some(ref mut stream) = *stream_guard {
            match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Ok(Some(Ok(msg))) => Some(msg),
                Ok(Some(Err(e))) => {
                    error!("WebSocket 读取错误: {}", e);
                    *self.running.write().await = false;
                    None
                }
                Ok(None) => {
                    warn!("WebSocket 连接已关闭");
                    *self.running.write().await = false;
                    None
                }
                Err(_) => None,
            }
        } else {
            None
        }
    }

    async fn handle_message(&self, msg: Message) {
        match msg {
            Message::Text(text) => {
                info!("----------------------------------------");
                info!("收到文本消息，长度: {} 字节", text.len());
                info!("原始消息: {}", if text.len() > 500 { 
                    format!("{}... (截断)", &text[..500]) 
                } else { 
                    text.clone() 
                });
                info!("----------------------------------------");

                match serde_json::from_str::<GatewayMessage>(&text) {
                    Ok(gateway_msg) => {
                        info!("解析成功 - 信令类型 s={}", gateway_msg.s);
                        self.handle_gateway_message(gateway_msg).await;
                    }
                    Err(e) => {
                        error!("解析 GatewayMessage 失败: {}", e);
                        error!("原始内容: {}", text);
                        
                        if let Ok(raw_value) = serde_json::from_str::<serde_json::Value>(&text) {
                            info!("尝试直接解析为 JSON:");
                            if let Some(s) = raw_value.get("s") {
                                info!("  s = {:?}", s);
                            }
                            if let Some(d) = raw_value.get("d") {
                                info!("  d (前200字符) = {:?}", 
                                    serde_json::to_string(d).unwrap_or_default().chars().take(200).collect::<String>());
                            }
                        }
                    }
                }
            }
            Message::Binary(data) => {
                info!("收到二进制消息，长度: {} 字节", data.len());
                
                // 尝试解压缩
                match try_decompress(&data) {
                    Ok(text) => {
                        info!("解压后消息: {}", if text.len() > 500 {
                            format!("{}... (截断)", &text[..500])
                        } else {
                            text.clone()
                        });
                        
                        match serde_json::from_str::<GatewayMessage>(&text) {
                            Ok(gateway_msg) => {
                                self.handle_gateway_message(gateway_msg).await;
                            }
                            Err(e) => {
                                error!("解压后解析失败: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("解压失败: {}", e);
                    }
                }
            }
            Message::Close(frame) => {
                warn!("收到关闭帧: {:?}", frame);
                *self.running.write().await = false;
            }
            Message::Ping(data) => {
                info!("收到 WebSocket Ping，长度: {}", data.len());
                let mut stream = self.ws_stream.write().await;
                if let Some(ref mut s) = *stream {
                    if let Err(e) = s.send(Message::Pong(data)).await {
                        error!("发送 Pong 失败: {}", e);
                    } else {
                        info!("已回复 Pong");
                    }
                }
            }
            Message::Pong(_) => {
                debug!("收到 WebSocket Pong");
            }
            Message::Frame(_) => {
                debug!("收到 Frame 消息");
            }
        }
    }

    async fn handle_gateway_message(&self, msg: GatewayMessage) {
        let signal_type = SignalType::from(msg.s);
        info!("处理信令: {:?}", signal_type);

        match signal_type {
            SignalType::Event => {
                info!(">>> 收到事件消息 <<<");
                if let Some(sn) = msg.sn {
                    info!("  序列号 sn={}", sn);
                    self.session_info.write().await.last_sn = sn;
                }
                
                if let Some(data) = &msg.d {
                    info!("  事件数据 d: {}", 
                        serde_json::to_string(data).unwrap_or_default().chars().take(300).collect::<String>());
                    
                    if let Some(event) = parse_event(data.clone()) {
                        info!("  解析事件成功: {:?}", std::mem::discriminant(&event));
                        self.dispatch_event(event).await;
                    } else {
                        warn!("  解析事件返回 None");
                        warn!("  原始数据: {:?}", data);
                    }
                } else {
                    warn!("  事件数据 d 为空");
                }
            }
            SignalType::Hello => {
                info!("========================================");
                info!("收到 Hello 消息");
                info!("========================================");
                
                if let Some(interval) = msg.heartbeat_interval() {
                    *self.heartbeat_interval.write().await = interval;
                    info!("  心跳间隔: {}ms", interval);
                }
                if let Some(session_id) = msg.session_id() {
                    self.session_info.write().await.session_id = Some(session_id.to_string());
                    info!("  会话ID: {}", session_id);
                }
                
                self.send_identify().await;
            }
            SignalType::Ping => {
                info!("收到 Kook Ping，回复 Pong");
                self.send_pong().await;
            }
            SignalType::Pong => {
                info!("收到 Kook Pong");
            }
            SignalType::Reconnect => {
                warn!("服务器要求重连");
                *self.running.write().await = false;
            }
            SignalType::Resume => {
                info!("收到 Resume");
            }
            SignalType::ResumeAck => {
                info!("Resume 成功");
            }
        }
    }

    async fn send_identify(&self) {
        info!("========================================");
        info!("发送 Identify (s=2)");
        info!("Token: {}...", &self.token.chars().take(8).collect::<String>());
        info!("Intents: {} (二进制: {:b})", self.intents, self.intents);
        info!("========================================");

        let identify = serde_json::json!({
            "s": 2,
            "d": {
                "token": self.token,
                "intents": self.intents,
                "compress": false
            }
        });

        let json_str = identify.to_string();
        info!("发送内容: {}", json_str);

        let mut stream = self.ws_stream.write().await;
        if let Some(ref mut s) = *stream {
            match s.send(Message::Text(json_str)).await {
                Ok(_) => info!("Identify 发送成功"),
                Err(e) => {
                    error!("Identify 发送失败: {}", e);
                    *self.running.write().await = false;
                }
            }
        } else {
            error!("无法发送 Identify: WebSocket 流不存在");
        }
    }

    async fn send_heartbeat(&self) {
        let sn = self.session_info.read().await.last_sn;
        info!("发送心跳 (s=1), sn={}", sn);

        let heartbeat = serde_json::json!({
            "s": 1,
            "sn": sn
        });

        let mut stream = self.ws_stream.write().await;
        if let Some(ref mut s) = *stream {
            if let Err(e) = s.send(Message::Text(heartbeat.to_string())).await {
                error!("发送心跳失败: {}", e);
            }
        }
    }

    async fn send_pong(&self) {
        let pong = GatewayMessage::pong();
        let json_str = serde_json::to_string(&pong).unwrap_or_default();
        info!("发送 Pong: {}", json_str);

        let mut stream = self.ws_stream.write().await;
        if let Some(ref mut s) = *stream {
            if let Err(e) = s.send(Message::Text(json_str)).await {
                error!("发送 Pong 失败: {}", e);
            }
        }
    }

    async fn dispatch_event(&self, event: Event) {
        info!(">>> 分发事件到处理器 <<<");
        if let Some(handler) = self.event_handler.read().await.as_ref() {
            handler.on_event(event).await;
        } else {
            warn!("没有设置事件处理器!");
        }
    }

    pub async fn disconnect(&self) {
        info!("断开 Gateway 连接...");
        *self.running.write().await = false;
        let mut stream = self.ws_stream.write().await;
        if let Some(s) = stream.take() {
            let (mut write, _) = s.split();
            let _ = write.close().await;
        }
        info!("Gateway 连接已断开");
    }

    pub async fn is_connected(&self) -> bool {
        *self.running.read().await
    }
}

fn try_decompress(data: &[u8]) -> std::result::Result<String, String> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = String::new();
    
    decoder.read_to_string(&mut decompressed)
        .map_err(|e| format!("解压失败: {}", e))?;
    
    Ok(decompressed)
}
