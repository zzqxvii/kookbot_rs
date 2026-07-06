//! Gateway WebSocket 客户端

use crate::core::error::{BotError, Result};
use crate::gateway::events::{parse_event, Event, EventHandler};
use crate::gateway::protocol::{GatewayMessage, Intents, SessionInfo, SignalType};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, trace, warn};

type WsWrite = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsRead = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// Gateway 客户端
pub struct GatewayClient {
    token: String,
    intents: u32,
    ws_write: Mutex<Option<WsWrite>>,
    ws_read: Mutex<Option<WsRead>>,
    session_info: RwLock<SessionInfo>,
    /// 用于重连时恢复会话
    resume_info: RwLock<Option<SessionInfo>>,
    event_handler: RwLock<Option<Box<dyn EventHandler>>>,
    running: AtomicBool,
    heartbeat_interval: AtomicU64,
}

impl GatewayClient {
    pub fn new(token: impl Into<String>, intents: u32) -> Self {
        Self {
            token: token.into(),
            intents,
            ws_write: Mutex::new(None),
            ws_read: Mutex::new(None),
            session_info: RwLock::new(SessionInfo::default()),
            resume_info: RwLock::new(None),
            event_handler: RwLock::new(None),
            running: AtomicBool::new(false),
            heartbeat_interval: AtomicU64::new(0),
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

    /// 设置重连用的会话信息
    pub async fn set_resume_info(&self, info: SessionInfo) {
        *self.resume_info.write().await = Some(info);
    }

    /// 获取当前会话信息（用于重连时恢复）
    pub async fn get_session_info(&self) -> SessionInfo {
        self.session_info.read().await.clone()
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

        let (write, read) = ws_stream.split();

        *self.ws_write.lock().await = Some(write);
        *self.ws_read.lock().await = Some(read);
        self.running.store(true, Ordering::Release);

        info!("连接完成，开始监听消息...");
        Ok(())
    }

    pub async fn run(&self) -> Result<()> {
        info!("========================================");
        info!("Gateway 客户端开始运行");
        info!("Token 前8位: {}...", &self.token.chars().take(8).collect::<String>());
        info!("Intents: {}", self.intents);
        info!("========================================");


        // 如果有会话信息，尝试恢复会话
        let resume_info = self.resume_info.write().await.take();
        if let Some(info) = &resume_info {
            if let Some(sid) = &info.session_id {
                let resume_msg = GatewayMessage::resume(sid, info.last_sn);
                let mut write = self.ws_write.lock().await;
                if let Some(ref mut s) = *write {
                    let payload = serde_json::to_string(&resume_msg).unwrap_or_default();
                    info!("[Gateway] 发送 Resume 消息，session={}, sn={}", sid, info.last_sn);
                    if let Err(e) = s.send(Message::Text(payload.into())).await {
                        warn!("发送 Resume 失败: {}", e);
                    }
                }
            }
        }
        // 等待第一个消息（Hello），最多等待10秒
        info!("等待服务器 Hello 消息...");
        let mut hello_received = false;
        let hello_deadline = tokio::time::Instant::now() + Duration::from_secs(10);

        while tokio::time::Instant::now() < hello_deadline {
            if let Some(msg) = self.receive_message().await {
                self.handle_message(msg).await;
                // Kook Hello 不含 heartbeat_interval 字段，用 session_id 检测
                if self.session_info.read().await.session_id.is_some() {
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

        let hb_ms = self.heartbeat_interval.load(Ordering::Relaxed);
        let hb_duration = if hb_ms > 0 {
            Duration::from_millis(hb_ms)
        } else {
            Duration::from_secs(30)
        };
        let mut heartbeat_tick = interval(hb_duration);
        heartbeat_tick.tick().await;
        let mut message_count = 0u32;

        info!("[Gateway] 开始接收事件，心跳间隔 {}ms", hb_duration.as_millis());

        loop {
            if !self.running.load(Ordering::Acquire) {
                warn!("Gateway 连接已断开，退出运行循环");
                break;
            }

            tokio::select! {
                _ = heartbeat_tick.tick() => {
                    self.send_heartbeat().await;
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
        let mut read_guard = self.ws_read.lock().await;
        if let Some(ref mut stream) = *read_guard {
            match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Ok(Some(Ok(msg))) => Some(msg),
                Ok(Some(Err(e))) => {
                    error!("WebSocket 读取错误: {}", e);
                    self.running.store(false, Ordering::Release);
                    None
                }
                Ok(None) => {
                    warn!("WebSocket 连接已关闭");
                    self.running.store(false, Ordering::Release);
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
                trace!("[Gateway] 收到文本消息: {}", text);
                match serde_json::from_str::<GatewayMessage>(&text) {
                    Ok(gateway_msg) => {
                        self.handle_gateway_message(gateway_msg).await;
                    }
                    Err(e) => {
                        warn!("[Gateway] 消息解析失败: {}", e);
                    }
                }
            }
            Message::Binary(data) => {
                info!("[Gateway] 收到二进制消息, 长度: {} bytes", data.len());
                match try_decompress(&data) {
                    Ok(text) => {
                        debug!("[Gateway] 解压后消息: {}", text);
                        if let Ok(gateway_msg) = serde_json::from_str::<GatewayMessage>(&text) {
                            self.handle_gateway_message(gateway_msg).await;
                        }
                    }
                    Err(e) => {
                        warn!("[Gateway] 解压失败: {}", e);
                    }
                }
            }
            Message::Close(frame) => {
                warn!("[Gateway] 收到关闭帧: {:?}", frame);
                self.running.store(false, Ordering::Release);
            }
            Message::Ping(data) => {
                info!("[Gateway] 收到 WebSocket Ping, 长度: {}", data.len());
                let mut write = self.ws_write.lock().await;
                if let Some(ref mut s) = *write {
                    if let Err(e) = s.send(Message::Pong(data)).await {
                        error!("[Gateway] 发送 Pong 失败: {}", e);
                    } else {
                        info!("[Gateway] 已回复 Pong");
                    }
                }
            }
            Message::Pong(_) => {
                debug!("[Gateway] 收到 WebSocket Pong");
            }
            Message::Frame(_) => {
                debug!("[Gateway] 收到 Frame 消息");
            }
        }
    }

    async fn handle_gateway_message(&self, msg: GatewayMessage) {
        let signal_type = SignalType::from(msg.s);

        match signal_type {
            SignalType::Event => {
                if let Some(sn) = msg.sn {
                    self.session_info.write().await.last_sn = sn;
                }

                if let Some(data) = &msg.d {
                    let msg_type = data.get("type").and_then(|t| t.as_i64()).unwrap_or(-1);
                    let event_type = data.get("extra")
                        .and_then(|e| e.get("type"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown");

                    info!("[Gateway] 收到事件: type={}, extra.type={}", msg_type, event_type);

                    if msg_type == 255 {
                        debug!("[Gateway] 系统事件原始数据: {}", serde_json::to_string(data).unwrap_or_default());
                    }

                    if let Some(event) = parse_event(data.clone()) {
                        self.dispatch_event(event).await;
                    } else {
                        warn!("[Gateway] 事件解析返回 None");
                    }
                }
            }
            SignalType::Hello => {
                info!("🔗 收到 HELLO，连接到 Kook Gateway 成功");

                if let Some(interval) = msg.heartbeat_interval() {
                    info!("[Gateway] 心跳间隔: {}ms", interval);
                    self.heartbeat_interval.store(interval, Ordering::Relaxed);
                }
                if let Some(session_id) = msg.session_id() {
                    info!("[Gateway] Session ID: {}", session_id);
                    self.session_info.write().await.session_id = Some(session_id.to_string());
                }
            }
            SignalType::Ping => {
                info!("[Gateway] 收到 PING，发送 PONG");
                self.send_pong().await;
            }
            SignalType::Pong => {
                info!("[Gateway] 收到 PONG，心跳正常");
            }
            SignalType::Reconnect => {
                warn!("⚠️ 服务器要求重连");
                self.running.store(false, Ordering::Release);
            }
            SignalType::Resume => {
                debug!("收到 Resume");
            }
            SignalType::ResumeAck => {
                debug!("Resume 成功");
            }
        }
    }

    async fn send_heartbeat(&self) {
        let sn = self.session_info.read().await.last_sn;
        let heartbeat = serde_json::json!({
            "s": 2,
            "sn": sn
        });

        debug!("[Gateway] 发送心跳 PING, sn={}", sn);
        let mut write = self.ws_write.lock().await;
        if let Some(ref mut s) = *write {
            if let Err(e) = s.send(Message::Text(heartbeat.to_string().into())).await {
                error!("心跳发送失败: {}", e);
            }
        }
    }

    async fn send_pong(&self) {
        let pong = GatewayMessage::pong();
        let mut write = self.ws_write.lock().await;
        if let Some(ref mut s) = *write {
            if let Err(e) = s.send(Message::Text(serde_json::to_string(&pong).unwrap_or_default().into())).await {
                error!("发送 Pong 失败: {}", e);
            }
        }
    }

    async fn dispatch_event(&self, event: Event) {
        let event_type = match &event {
            Event::Message(_) => "Message",
            Event::SystemMessage(_) => "SystemMessage",
            Event::ButtonClick(_) => "ButtonClick",
            Event::UserJoinVoice(_) => "UserJoinVoice",
            Event::UserLeaveVoice(_) => "UserLeaveVoice",
            Event::UserAddReaction(_) => "UserAddReaction",
            Event::UserRemoveReaction(_) => "UserRemoveReaction",
            Event::Unknown(_) => "Unknown",
        };
        info!("[Gateway] 分发事件: {}", event_type);

        if let Some(handler) = self.event_handler.read().await.as_ref() {
            handler.on_event(event).await;
        } else {
            warn!("[Gateway] 没有事件处理器!");
        }
    }

    pub async fn disconnect(&self) {
        self.running.store(false, Ordering::Release);
        let mut write = self.ws_write.lock().await;
        if let Some(mut s) = write.take() {
            let _ = s.close().await;
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.running.load(Ordering::Acquire)
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
