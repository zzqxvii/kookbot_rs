//! Bot 核心模块 - 命令路由和事件处理
//! 
//! 这是 Bot 的主逻辑模块，负责：
//! - 命令路由和分发
//! - 事件处理（WebSocket 和 Webhook）
//! - 模块管理
//! 
//! 采用模块化设计，支持动态注册命令处理器。
//! 音乐播放只是其中一个内置模块，可以添加更多功能模块。

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::api::KookClient;
use crate::core::config::BotConfig;
use crate::gateway::{EventHandler, MessageData, SystemMessageData};
use crate::music::NeteaseClient;
use crate::player::VoiceManager;
use async_trait::async_trait;
use serde_json::Value;

pub mod commands;
pub mod music;

use commands::{CommandContext, CommandResult, CommandRouter};
use music::{create_music_commands, HelpCommand, JoinCommand, LeaveCommand};

/// Bot 核心结构体
pub struct Bot {
    /// Bot 配置
    config: BotConfig,
    /// API 客户端
    api_client: Arc<RwLock<Option<KookClient>>>,
    /// 命令路由器
    command_router: CommandRouter,
    /// 网易云客户端
    netease_client: Arc<RwLock<NeteaseClient>>,
    /// 语音管理器
    voice_manager: Arc<Mutex<Option<VoiceManager>>>,
}

impl Bot {
    /// 创建新的 Bot 实例
    pub fn new(config: BotConfig, api_client: KookClient) -> Self {
        // 清理 cookie 格式
        let netease_cookie = config.music.netease_cookie.as_ref()
            .map(|c| Self::clean_cookie(c));
        
        let netease_client = NeteaseClient::with_cookie(
            &config.music.netease_api_url,
            netease_cookie,
        );
        
        if netease_client.has_cookie() {
            info!("已加载网易云登录凭证");
        } else {
            info!("未配置网易云登录凭证，可能只能播放试听版本");
        }
        
        let mut command_router = CommandRouter::new(&config.prefix);
        
        // 注册基础命令
        command_router.register(Arc::new(HelpCommand));
        command_router.register(Arc::new(JoinCommand));
        command_router.register(Arc::new(LeaveCommand));
        
        // 注册音乐模块命令
        let netease_client_arc = Arc::new(RwLock::new(netease_client));
        for cmd in create_music_commands(netease_client_arc.clone(), config.clone()) {
            command_router.register(cmd);
        }
        
        Self {
            config,
            api_client: Arc::new(RwLock::new(Some(api_client))),
            command_router,
            netease_client: netease_client_arc,
            voice_manager: Arc::new(Mutex::new(None)),
        }
    }
    
    /// 清理 cookie 字符串
    fn clean_cookie(raw: &str) -> String {
        raw.split(';')
            .map(|s| s.trim())
            .filter(|s| {
                let s_lower = s.to_lowercase();
                !s_lower.starts_with("max-age")
                    && !s_lower.starts_with("expires")
                    && !s_lower.starts_with("path=")
                    && !s_lower.starts_with("domain=")
                    && !s_lower.starts_with("secure")
                    && !s_lower.starts_with("httponly")
                    && !s_lower.starts_with("samesite")
                    && !s.is_empty()
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
    
    /// 处理消息事件
    async fn handle_message(&self, data: &MessageData) {
        info!("========================================");
        info!("[Bot] 收到消息");
        info!("========================================");
        info!("  作者: {} (ID: {})", data.extra.author.nickname, data.author_id);
        info!("  内容: {}", data.content);
        info!("  频道: {}", data.target_id);
        
        // 使用命令路由器处理
        if let Some(result) = self.command_router.handle_message(
            data,
            self.api_client.clone(),
            &self.config,
            self.netease_client.clone(),
            self.voice_manager.clone(),
        ).await {
            let channel_id = &data.target_id;
            
            match result {
                CommandResult::Ok => {
                    debug!("命令执行成功");
                }
                CommandResult::Error(msg) => {
                    if let Some(client) = self.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(
                            channel_id,
                            &format!("❌ {}", msg)
                        ).await;
                    }
                }
                CommandResult::Reply(msg) => {
                    if let Some(client) = self.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(channel_id, &msg).await;
                    }
                }
            }
        }
    }
    
    /// 获取 API 客户端
    pub fn api_client(&self) -> Arc<RwLock<Option<KookClient>>> {
        self.api_client.clone()
    }
    
    /// 获取配置
    pub fn config(&self) -> &BotConfig {
        &self.config
    }
}

/// WebSocket 事件处理器
pub struct BotEventHandler {
    bot: Arc<Bot>,
}

impl BotEventHandler {
    pub fn new(bot: Arc<Bot>) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl EventHandler for BotEventHandler {
    async fn on_message(&self, data: MessageData) {
        // 只处理文字消息和 KMarkdown 消息
        if data.is_text() || data.is_kmarkdown() {
            self.bot.handle_message(&data).await;
        }
    }
    
    async fn on_system_message(&self, data: SystemMessageData) {
        info!("[Bot] 系统消息: {}", data.extra.event_type);
    }
    
    async fn on_unknown(&self, data: Value) {
        warn!("[Bot] 未知事件类型: {:?}", data.get("type"));
    }
}

/// Webhook 事件处理器
pub struct BotWebhookHandler {
    bot: Arc<Bot>,
}

impl BotWebhookHandler {
    pub fn new(bot: Arc<Bot>) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl crate::webhook::WebhookHandler for BotWebhookHandler {
    async fn handle_event(&self, event_type: u32, data: Value) {
        info!("[Webhook] 收到事件: type={}", event_type);
        
        match event_type {
            0 => {
                info!("[Webhook] 收到验证请求");
            }
            1 => {
                // 解析为 MessageData 并处理
                if let Ok(msg_data) = serde_json::from_value::<MessageData>(data) {
                    if msg_data.is_text() || msg_data.is_kmarkdown() {
                        self.bot.handle_message(&msg_data).await;
                    }
                }
            }
            _ => {
                debug!("[Webhook] 收到未处理的事件类型: {}", event_type);
            }
        }
    }
}

/// 创建 Bot 实例和事件处理器
pub fn create_bot(
    config: BotConfig,
    api_client: KookClient,
) -> (Arc<Bot>, BotEventHandler, BotWebhookHandler) {
    let bot = Arc::new(Bot::new(config, api_client));
    let ws_handler = BotEventHandler::new(bot.clone());
    let webhook_handler = BotWebhookHandler::new(bot.clone());
    
    (bot, ws_handler, webhook_handler)
}
