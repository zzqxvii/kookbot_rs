//! Gateway WebSocket 客户端
//!
//! 管理 WebSocket 连接，处理身份验证、心跳和事件分发

use crate::error::{BotError, Result};
use crate::gateway::events::{Event, EventHandler};
use crate::gateway::heartbeat::HeartbeatManager;
use crate::gateway::protocol::{GatewayOp, GatewayPayload, Intents, SessionInfo};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, trace, warn};


/// Gateway 客户端
pub struct GatewayClient {
    /// Bot Token
    token: String,
    /// 订阅的意图
    intents: u32,
    /// WebSocket 连接
    ws_stream: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    /// 会话信息
    session_info: Arc<RwLock<SessionInfo>>,
    /// 心跳管理器
    heartbeat_manager: Arc<RwLock<Option<HeartbeatManager>>>,
    /// 事件处理器
    event_handler: Arc<RwLock<Option<Box<dyn EventHandler>>>>,
    /// 心跳发送通道
    heartbeat_tx: Arc<RwLock<Option<mpsc::Sender<GatewayPayload>>>>,
    /// 运行状态
    running: Arc<RwLock<bool>>,
    /// 消息发送通道
    message_tx: Arc<RwLock<Option<mpsc::UnboundedSender<Message>>>>,
}

impl GatewayClient {
    /// 创建新的 Gateway 客户端
    pub fn new(token: impl Into<String>, intents: u32) -> Self {
        Self {
            token: token.into(),
            intents,
            ws_stream: Arc::new(RwLock::new(None)),
            session_info: Arc::new(RwLock::new(SessionInfo::default())),
            heartbeat_manager: Arc::new(RwLock::new(None)),
            event_handler: Arc::new(RwLock::new(None)),
            heartbeat_tx: Arc::new(RwLock::new(None)),
            running: Arc::new(RwLock::new(false)),
            message_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// 使用常用意图创建客户端
    pub fn with_basic_intents(token: impl Into<String>) -> Self {
        Self::new(token, Intents::BASIC)
    }

    /// 使用所有意图创建客户端
    pub fn with_all_intents(token: impl Into<String>) -> Self {
        Self::new(token, Intents::ALL)
    }

    /// 设置事件处理器
    pub async fn set_event_handler(&self, handler: Box<dyn EventHandler>) {
        let mut h = self.event_handler.write().await;
        *h = Some(handler);
    }

    /// 连接到 Gateway
    pub async fn connect(&self, gateway_url: &str) -> Result<()> {
        info!("正在连接到 Kook Gateway: {}", gateway_url);

        // 建立 WebSocket 连接
        let (ws_stream, _) = connect_async(gateway_url)
            .await
            .map_err(|e| BotError::GatewayError(format!("连接失败: {}", e)))?;

        info!("WebSocket 连接已建立");

        // 保存连接
        {
            let mut stream = self.ws_stream.write().await;
            *stream = Some(ws_stream);
        }

        // 设置运行状态
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        // 启动消息处理循环
        self.start_message_loop().await;

        Ok(())
    }

    /// 启动消息处理循环
    async fn start_message_loop(&self) {
        info!("启动消息处理循环");

        loop {
            // 检查是否还在运行
            if !*self.running.read().await {
                info!("消息处理循环停止");
                break;
            }

            // 尝试读取消息
            let message_text = {
                let mut stream_guard = self.ws_stream.write().await;
                if let Some(ref mut stream) = *stream_guard {
                    // 使用 timeout 避免永久阻塞
                    match tokio::time::timeout(
                        Duration::from_secs(1),
                        stream.next()
                    ).await {
                        Ok(Some(Ok(Message::Text(text)))) => Some(text),
                        Ok(Some(Ok(Message::Binary(data)))) => {
                            debug!("收到二进制消息，长度: {}", data.len());
                            None
                        }
                        Ok(Some(Ok(Message::Close(frame)))) => {
                            info!("收到关闭帧: {:?}", frame);
                            let mut running = self.running.write().await;
                            *running = false;
                            None
                        }
                        Ok(Some(Ok(Message::Ping(data)))) => {
                            // 自动回复 Pong
                            let _ = stream.send(Message::Pong(data)).await;
                            None
                        }
                        Ok(Some(Ok(Message::Pong(_)))) => {
                            debug!("收到 Pong");
                            None
                        }
                        Ok(Some(Ok(Message::Frame(_)))) => {
                            debug!("收到 Frame");
                            None
                        }
                        Ok(Some(Err(e))) => {
                            error!("WebSocket 错误: {}", e);
                            None
                        }
                        Ok(None) => {
                            info!("WebSocket 连接已关闭");
                            let mut running = self.running.write().await;
                            *running = false;
                            None
                        }
                        Err(_) => None, // 超时，继续循环
                    }
                } else {
                    None
                }
            };

            // 处理消息
            if let Some(text) = message_text {
                self.handle_message(Message::Text(text)).await;
            }
        }
    }

    /// 处理单个 WebSocket 消息
    async fn handle_message(&self, msg: Message) {
        match msg {
            Message::Text(text) => {
                debug!("收到文本消息: {}", text);
                // 解析 GatewayPayload
                match serde_json::from_str::<GatewayPayload>(&text) {
                    Ok(payload) => self.handle_payload(payload).await,
                    Err(e) => error!("解析消息失败: {}", e),
                }
            }
            Message::Binary(data) => {
                debug!("收到二进制消息，长度: {}", data.len());
                // 处理压缩数据（如果需要）
            }
            Message::Close(frame) => {
                info!("收到关闭帧: {:?}", frame);
                // 设置运行状态为 false
                let mut running = self.running.write().await;
                *running = false;
            }
            Message::Ping(data) => {
                debug!("收到 Ping");
                // 自动回复 Pong
                let mut stream_guard = self.ws_stream.write().await;
                if let Some(ref mut stream) = *stream_guard {
                    let _ = stream.send(Message::Pong(data)).await;
                }
            }
            Message::Pong(_) => {
                debug!("收到 Pong");
            }
            _ => {}
        }
    }

    /// 处理 GatewayPayload
    async fn handle_payload(&self, payload: GatewayPayload) {
        match payload.op {
            0 => {
                // 事件 (Dispatch)
                if let Some(t) = &payload.t {
                    debug!("收到事件: {}", t);
                    // 生成对应的事件并调用处理器
                    if let Some(event) = self.create_event(&payload).await {
                        self.dispatch_event(event).await;
                    }
                }
            }
            1 => {
                // 心跳
                debug!("收到心跳请求");
            }
            10 => {
                // Hello
                info!("收到 Hello，准备发送 Identify");
                // 发送 Identity 消息
                let identify = GatewayPayload::identify(&self.token, self.intents);
                self.send_payload(identify).await;
            }
            11 => {
                // 心跳确认
                debug!("收到心跳确认");
            }
            _ => {
                debug!("收到未知操作码: {}", payload.op);
            }
        }
    }

    /// 从 GatewayPayload 创建 Event
    async fn create_event(&self, payload: &GatewayPayload) -> Option<crate::gateway::events::Event> {
        use crate::gateway::events::*;

        let event_type = payload.t.as_deref()?;
        let data = payload.d.as_ref()?;

        match event_type {
            "READY" => {
                if let Ok(ready) = serde_json::from_value::<ReadyEvent>(data.clone()) {
                    return Some(Event::Ready(ready));
                }
            }
            "MESSAGE_CREATE" => {
                if let Ok(msg) = serde_json::from_value::<MessageCreateEvent>(data.clone()) {
                    return Some(Event::MessageCreate(msg));
                }
            }
            "GUILD_MEMBER_ADD" => {
                if let Ok(member) = serde_json::from_value::<GuildMemberAddEvent>(data.clone()) {
                    return Some(Event::GuildMemberAdd(member));
                }
            }
            // 可以添加更多事件类型
            _ => {
                debug!("未处理的事件类型: {}", event_type);
                // 返回未知事件
                if let Ok(unknown) = serde_json::from_value::<UnknownEvent>(data.clone()) {
                    return Some(Event::Unknown(unknown));
                }
            }
        }

        None
    }

    /// 分发事件到处理器
    async fn dispatch_event(&self, event: crate::gateway::events::Event) {
        let handler_opt = self.event_handler.read().await;
        if let Some(handler) = handler_opt.as_ref() {
            handler.on_event(event).await;
        }
    }

    /// 发送 GatewayPayload
    async fn send_payload(&self, payload: GatewayPayload) {
        let text = match serde_json::to_string(&payload) {
            Ok(json) => json,
            Err(e) => {
                error!("序列化消息失败: {}", e);
                return;
            }
        };

        let mut stream_guard = self.ws_stream.write().await;
        if let Some(ref mut stream) = *stream_guard {
            if let Err(e) = stream.send(Message::Text(text)).await {
                error!("发送消息失败: {}", e);
            } else {
                debug!("发送消息成功");
            }
        }
    }

    /// 断开连接
    pub async fn disconnect(&self) {
        info!("断开 Gateway 连接...");

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // 停止心跳
        {
            let mut hbm = self.heartbeat_manager.write().await;
            if let Some(ref mut hbm) = *hbm {
                hbm.stop().await;
            }
        }

        // 关闭 WebSocket 连接
        {
            let mut stream = self.ws_stream.write().await;
            if let Some(stream) = stream.take() {
                let (mut write, _) = stream.split();
                let _ = write.close().await;
            }
        }

        info!("Gateway 连接已断开");
    }

    /// 检查是否已连接
    pub async fn is_connected(&self) -> bool {
        *self.running.read().await
    }

    /// 获取会话信息
    pub async fn session_info(&self) -> SessionInfo {
        self.session_info.read().await.clone()
    }

    /// 运行 Gateway 客户端（阻塞直到连接断开）
    pub async fn run(&self) -> Result<()> {
        info!("Gateway 客户端开始运行");

        loop {
            // 检查是否还在运行
            if !self.is_connected().await {
                info!("Gateway 连接已断开");
                break;
            }

            // 等待一段时间再检查
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        info!("Gateway 客户端停止运行");
        Ok(())
    }
}

impl Drop for GatewayClient {
    fn drop(&mut self) {
        // 尝试优雅地关闭连接
        // 注意：这里不能阻塞，所以不能使用 async
    }
}
