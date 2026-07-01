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
use tracing::{debug, info, warn};

use crate::api::KookClient;
use crate::common::play_state::PlayState;
use crate::core::config::BotConfig;
use crate::gateway::{
    ButtonClickData, EventHandler, MessageData, ReactionEventData, SystemMessageData,
    VoiceChannelEventData,
};
use crate::music::NeteaseClient;
use crate::music::QQMusicClient;
use crate::music::BilibiliClient;
use crate::player::VoiceManager;
use async_trait::async_trait;
use serde_json::Value;

pub mod commands;
pub mod music;
pub mod qqmusic;
pub mod bilibili;

use commands::{CommandResult, CommandRouter};
use music::{create_music_commands, HelpCommand, JoinCommand, LeaveCommand, UnifiedSearchCommand};
use qqmusic::create_qqmusic_commands;
use bilibili::create_bilibili_commands;

/// Bot 核心结构体
pub struct Bot {
    /// Bot 配置
    config: BotConfig,
    /// API 客户端
    api_client: Arc<RwLock<Option<KookClient>>>,
    /// 命令路由器
    command_router: CommandRouter,
    /// 播放状态
    play_state: Arc<PlayState>,
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
            .map(|c| Self::clean_cookie(c))
            .filter(|c| !c.is_empty());
        let netease_client = NeteaseClient::with_cookie(
            &config.music.netease_api_url,
            netease_cookie,
        );
        
        if netease_client.has_cookie() {
            info!("已加载网易云登录凭证");
        } else {
            info!("未配置网易云登录凭证，可能只能播放试听版本");
        }

        // 创建 QQ 音乐客户端
        let qqmusic_cookie = config.music.qqmusic_cookie.as_ref()
            .map(|c| Self::clean_cookie(c))
            .filter(|c| !c.is_empty());
        let qqmusic_client = QQMusicClient::with_cookie(
            &config.music.qqmusic_api_url,
            qqmusic_cookie,
        );

        if qqmusic_client.has_cookie() {
            info!("已加载QQ音乐登录凭证");
        } else {
            info!("未配置QQ音乐登录凭证，可能只能播放试听版本");
        }

        // 创建 B站客户端
        let bilibili_cookie = config.music.bilibili_cookie.as_ref()
            .map(|c| Self::clean_cookie(c))
            .filter(|c| !c.is_empty());
        let bilibili_client = BilibiliClient::with_cookie(
            &config.music.bilibili_api_url,
            bilibili_cookie,
        );

        if bilibili_client.has_cookie() {
            info!("已加载B站登录凭证");
        } else {
            info!("未配置B站登录凭证，可能只能播放试听版本");
        }

        let mut command_router = CommandRouter::new(&config.prefix);

        // 注册基础命令
        command_router.register(Arc::new(HelpCommand));
        command_router.register(Arc::new(JoinCommand));
        command_router.register(Arc::new(LeaveCommand));
        // 注册音乐模块命令
        let play_state = Arc::new(PlayState::new());
        let netease_client_arc = Arc::new(RwLock::new(netease_client));
        for cmd in create_music_commands(netease_client_arc.clone(), config.clone(), play_state.clone()) {
            command_router.register(cmd);
        }
        // 注册 QQ 音乐命令
        let qqmusic_client_arc = Arc::new(RwLock::new(qqmusic_client));
        for cmd in create_qqmusic_commands(qqmusic_client_arc.clone(), &config, play_state.clone()) {
            command_router.register(cmd);
        }
        // 注册 B站命令
        let bilibili_client_arc = Arc::new(RwLock::new(bilibili_client));
        for cmd in create_bilibili_commands(bilibili_client_arc.clone(), &config, play_state.clone()) {
            command_router.register(cmd);
        }
        // 注册跨平台搜索命令
        command_router.register(Arc::new(UnifiedSearchCommand::new(
            netease_client_arc.clone(),
            qqmusic_client_arc.clone(),
            bilibili_client_arc.clone(),
        )));

        Self {
            config,
            api_client: Arc::new(RwLock::new(Some(api_client))),
            command_router,
            play_state,
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
        if let Some(result) = self.command_router.handle_message(
            data,
            self.api_client.clone(),
            &self.config,
            &self.play_state,
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
    
    async fn on_button_click(&self, data: ButtonClickData) {
        info!("[Bot] ========== 按钮点击事件 ==========");
        info!("[Bot] channel_type: {}", data.channel_type);
        info!("[Bot] target_id (顶层): {}", data.target_id);
        info!("[Bot] user_id: {}", data.extra.body.user_id);
        info!("[Bot] button value: {}", data.extra.body.value);
        info!("[Bot] msg_id: {}", data.extra.body.msg_id);
        info!("[Bot] body.target_id (频道): {}", data.extra.body.target_id);
        
        let user_id = &data.extra.body.user_id;
        let value = &data.extra.body.value;
        // 使用 body.target_id 作为回复目标（群组频道）
        let channel_id = &data.extra.body.target_id;
        
        // 获取用户显示名称
        let user_display = if let Some(ref user_info) = data.extra.body.user_info {
            if !user_info.nickname.is_empty() {
                user_info.nickname.clone()
            } else if !user_info.username.is_empty() {
                user_info.username.clone()
            } else {
                user_id.clone()
            }
        } else {
            user_id.clone()
        };
        
        // 管理员权限检查（仅当配置了管理员列表时）
        if !self.bot.config.admins.is_empty()
            && !self.bot.config.is_admin(user_id)
            && (value == "stop" || value == "nextMusic")
        {
            info!("[Bot] 非管理员用户 {} 尝试执行 {}", user_id, value);
            if let Some(client) = self.bot.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(
                    channel_id,
                    "⚠️ 仅管理员可执行此操作"
                ).await;
            }
            return;
        }

        match value.as_str() {
            "nextMusic" => {
                info!("[Bot] 处理下一首请求, is_playing={}", self.bot.play_state.is_playing());
                info!("[Bot] 当前 PID: {}", self.bot.play_state.get_pid());
                
                if self.bot.play_state.is_playing() {
                    self.bot.play_state.request_next();
                    let killed = self.bot.play_state.kill_process();
                    info!("[Bot] 进程已终止: {}", killed);
                    
                    if let Some(client) = self.bot.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(
                            channel_id,
                            &format!("⏭️ 用户 **{}** 点击了下一首", user_display)
                        ).await;
                    }
                } else {
                    info!("[Bot] 当前没有正在播放的歌曲");
                    if let Some(client) = self.bot.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(
                            channel_id,
                            "⚠️ 当前没有正在播放的歌曲"
                        ).await;
                    }
                }
            }
            "stop" => {
                info!("[Bot] 处理停止请求, is_playing={}", self.bot.play_state.is_playing());
                info!("[Bot] 当前 PID: {}", self.bot.play_state.get_pid());

                if self.bot.play_state.is_playing() {
                    self.bot.play_state.request_stop();
                    let killed = self.bot.play_state.kill_process();
                    info!("[Bot] 进程已终止: {}", killed);

                    // 删除播放卡片
                    if let Some(client) = self.bot.api_client.read().await.as_ref() {
                        if let Some(old_msg_id) = self.bot.play_state.take_play_msg_id() {
                            let _ = client.delete_message(&old_msg_id).await;
                        }
                    }

                    // 离开语音频道
                    let mut vm = self.bot.voice_manager.lock().await;
                    if let Some(voice_manager) = vm.as_mut() {
                        let _ = voice_manager.leave_channel().await;
                        *vm = None;
                    }

                    // 发送停止确认消息
                    if let Some(client) = self.bot.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(
                            channel_id,
                            &format!("⏹️ 用户 **{}** 停止了播放", user_display)
                        ).await;
                    }
                } else {
                    info!("[Bot] 当前没有正在播放的歌曲");
                    if let Some(client) = self.bot.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(
                            channel_id,
                            "⚠️ 当前没有正在播放的歌曲"
                        ).await;
                    }
                }
            }
            _ => {
                debug!("未处理的按钮值: {}", value);
            }
        }
        info!("[Bot] ========== 按钮事件处理完成 ==========");
    }
    
    async fn on_user_join_voice(&self, data: VoiceChannelEventData) {
        info!(
            "[Bot] 用户 {} 加入语音频道 {}",
            data.extra.body.user_id, data.extra.body.channel_id
        );
    }
    
    async fn on_user_leave_voice(&self, data: VoiceChannelEventData) {
        info!(
            "[Bot] 用户 {} 离开语音频道 {}",
            data.extra.body.user_id, data.extra.body.channel_id
        );
    }
    
    async fn on_user_add_reaction(&self, data: ReactionEventData) {
        info!(
            "[Bot] 用户 {} 对消息 {} 添加表情 {}",
            data.extra.body.user_id, data.extra.body.msg_id, data.extra.body.emoji.id
        );
    }
    
    async fn on_user_remove_reaction(&self, data: ReactionEventData) {
        info!(
            "[Bot] 用户 {} 对消息 {} 删除表情 {}",
            data.extra.body.user_id, data.extra.body.msg_id, data.extra.body.emoji.id
        );
    }
    
    async fn on_unknown(&self, data: Value) {
        let msg_type = data.get("type").and_then(|t| t.as_i64()).unwrap_or(-1);
        let event_type = data.get("extra")
            .and_then(|e| e.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");
        warn!("[Bot] 未知事件: type={}, event_type={}", msg_type, event_type);
        warn!("[Bot] 原始数据: {}", serde_json::to_string(&data).unwrap_or_default());
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
