use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn, Level};
use std::fmt;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use owo_colors::OwoColorize;

use kook_music_bot::api::{Channel, KookClient};
use kook_music_bot::config::{BotConfig, ConnectionMode};
use kook_music_bot::webhook::{WebhookHandler, WebhookServer};
use kook_music_bot::gateway::{EventHandler, GatewayClient, MessageData};
use kook_music_bot::player::{VoiceManager, VoiceStreamingInfo};
use kook_music_bot::music::NeteaseClient;
use kook_music_bot::audio::{FFmpegDirectStreamer, StreamerConfig};
use async_trait::async_trait;
use serde_json::Value;

struct AlignedFormatter;

impl<S, N> FormatEvent<S, N> for AlignedFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        // 级别 (3字符缩写) + 颜色
        let level_str = match *meta.level() {
            tracing::Level::TRACE => "TRC".dimmed().to_string(),
            tracing::Level::DEBUG => "DBG".blue().to_string(),
            tracing::Level::INFO => "INF".green().to_string(),
            tracing::Level::WARN => "WRN".yellow().to_string(),
            tracing::Level::ERROR => "ERR".red().to_string(),
        };
        write!(writer, "{} ", level_str)?;

        // 时间 (日期用/分隔，毫秒1位)
        let timestamp = chrono::Local::now().format("%Y/%m/%d %H:%M:%S");
        write!(writer, "{} ", timestamp)?;

        // 文件和行号 (路径固定宽度，行号紧跟)
        let file = meta.file().unwrap_or("unknown");
        let line = meta.line().unwrap_or(0);
        let location = if file.len() > 18 {
            format!("{}:{}", &file[file.len()-18..], line)
        } else {
            format!("{}:{}", file, line)
        };
        write!(writer, "{:<22} ", location.dimmed())?;

        // 消息
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn update_netease_cookie(config_path: &std::path::Path, cookie: &str) -> anyhow::Result<()> {
    use std::fs;

    let content = fs::read_to_string(config_path)?;
    let mut updated = false;
    let mut new_lines = Vec::new();

    for line in content.lines() {
        if line.starts_with("netease_cookie") {
            new_lines.push(format!("netease_cookie = \"{}\"", cookie));
            updated = true;
        } else if line.starts_with("[music]") && !updated {
            // 如果 [music] section 存在但没有 netease_cookie 行，在后面添加
            new_lines.push(line.to_string());
            new_lines.push(format!("netease_cookie = \"{}\"", cookie));
            updated = true;
            continue;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !updated {
        // 如果没有找到 [music] section，在文件末尾添加
        new_lines.push(String::new());
        new_lines.push("[music]".to_string());
        new_lines.push(format!("netease_cookie = \"{}\"", cookie));
    }

    fs::write(config_path, new_lines.join("\n"))?;
    info!("Cookie 已保存到 {:?}", config_path);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .event_format(AlignedFormatter)
        .init();

    info!("Kook Music Bot - Rust 实现 v0.1.0");

    run_bot(None).await?;
    Ok(())
}

async fn run_bot(config_path: Option<PathBuf>) -> Result<()> {
    info!("========================================");
    info!("启动机器人");
    info!("========================================");

    let config_path = config_path.unwrap_or_else(|| PathBuf::from("config.toml"));
    let config = BotConfig::from_file(&config_path)?;

    info!("✓ 配置加载成功");
    info!("  命令前缀: {}", config.prefix);
    info!("  连接模式: {:?}", config.mode);
    info!("  Token: {}...{}",
        &config.token.chars().take(4).collect::<String>(),
        &config.token.chars().last().unwrap_or('?'));

    // 检查 FFmpeg
    match tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
    {
        Ok(_) => info!("✓ FFmpeg 可用"),
        Err(_) => {
            warn!("✗ FFmpeg 未找到，语音功能可能不可用");
            warn!("  请安装 FFmpeg: https://ffmpeg.org/download.html");
        }
    }

    // 创建 API 客户端并获取启动信息
    let api_client = KookClient::new(&config)?;

    // 获取机器人信息
    info!("========================================");
    info!("获取机器人信息");
    info!("========================================");

    match api_client.get_current_user().await {
        Ok(user) => {
            info!("机器人 ID: {}", user.id);
            info!("机器人名称: {}", user.username);
            if let Some(avatar) = &user.avatar {
                info!("头像: {}", avatar);
            }
        }
        Err(e) => {
            warn!("获取用户信息失败: {}", e);
        }
    }

    // 获取服务器列表
    info!("========================================");
    info!("获取服务器列表");
    info!("========================================");

    match api_client.get_guild_list().await {
        Ok(guilds) => {
            if guilds.is_empty() {
                info!("机器人未加入任何服务器");
            } else {
                info!("已加入 {} 个服务器:", guilds.len());

                for (idx, guild) in guilds.iter().enumerate() {
                    info!("----------------------------------------");
                    info!("[{}] {}", idx + 1, guild.name);
                    info!("    ID: {}", guild.id);
                    if !guild.topic.is_empty() {
                        info!("    主题: {}", guild.topic);
                    }

                    // 获取该服务器的频道列表
                    match api_client.get_channel_list(&guild.id).await {
                        Ok(channels) => {
                            let text_channels: Vec<&Channel> = channels.iter()
                                .filter(|c| c.channel_type == 1 && !c.is_category)
                                .collect();
                            let voice_channels: Vec<&Channel> = channels.iter()
                                .filter(|c| c.channel_type == 2 && !c.is_category)
                                .collect();
                            let categories: Vec<&Channel> = channels.iter()
                                .filter(|c| c.is_category)
                                .collect();

                            info!("    频道: {} 个文字频道, {} 个语音频道, {} 个分类",
                                text_channels.len(), voice_channels.len(), categories.len());

                            if !voice_channels.is_empty() {
                                let vc_names: Vec<&str> = voice_channels.iter()
                                    .map(|c| c.name.as_str())
                                    .collect();
                                info!("    语音频道: {}", vc_names.join(", "));
                            }
                        }
                        Err(e) => {
                            warn!("    获取频道列表失败: {}", e);
                        }
                    }
                }

                info!("----------------------------------------");
            }
        }
        Err(e) => {
            warn!("获取服务器列表失败: {}", e);
        }
    }

    match config.mode {
        ConnectionMode::Webhook => {
            start_webhook_mode(config, api_client).await?;
        }
        ConnectionMode::Websocket => {
            start_websocket_mode(config, api_client).await?;
        }
    }

    Ok(())
}

async fn start_webhook_mode(config: BotConfig, api_client: KookClient) -> Result<()> {
    info!("========================================");
    info!("启动 Webhook 服务器");
    info!("========================================");
    info!("地址: http://{}:{}",
        config.webhook.host,
        config.webhook.port
    );
    info!("路径: {}", config.webhook.path);
    info!("验证令牌: {}...", &config.webhook.verify_token.chars().take(4).collect::<String>());

    let handler = Arc::new(BotWebhookHandler::new(config, api_client));
    let server = WebhookServer::new(handler.config.webhook.clone(), handler);

    info!("✓ Webhook 服务器已启动，等待 KOOK 事件...");
    info!("========================================");

    server.run().await?;

    Ok(())
}

async fn start_websocket_mode(config: BotConfig, api_client: KookClient) -> Result<()> {
    info!("========================================");
    info!("启动 WebSocket 连接");
    info!("========================================");

    info!("正在获取 Gateway 地址...");
    let gateway_url = api_client.get_gateway_url().await?;
    info!("✓ Gateway 地址: {}", gateway_url);

    let token = config.token.clone();
    let client = GatewayClient::with_all_intents(&token);

    let handler = BotEventHandler::new(config, api_client);
    client.set_event_handler(Box::new(handler)).await;

    client.connect(&gateway_url).await?;

    info!("✓ WebSocket 客户端已连接");
    info!("========================================");

    client.run().await?;

    Ok(())
}

struct BotWebhookHandler {
    config: BotConfig,
    api_client: Arc<RwLock<Option<KookClient>>>,
    voice_manager: Arc<Mutex<Option<VoiceManager>>>,
}

impl BotWebhookHandler {
    fn new(config: BotConfig, api_client: KookClient) -> Self {
        Self {
            config,
            api_client: Arc::new(RwLock::new(Some(api_client))),
            voice_manager: Arc::new(Mutex::new(None)),
        }
    }

    async fn handle_message(&self, data: Value) {
        let author_id = data.get("author_id").and_then(|v| v.as_str());
        let content = data.get("content").and_then(|v| v.as_str());
        let channel_id = data.get("target_id").and_then(|v| v.as_str());

        if author_id.is_none() || content.is_none() || channel_id.is_none() {
            return;
        }

        let author_id = author_id.unwrap();
        let content = content.unwrap();
        let channel_id = channel_id.unwrap();

        if data.get("extra")
            .and_then(|e| e.get("author"))
            .and_then(|a| a.get("bot"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false)
        {
            return;
        }

        // 检查消息是否以 prefix 开头
        if content.starts_with(&self.config.prefix) {
            info!("[Webhook] 收到 prefix 消息: {}", content);

            // 回复 hello
            if let Some(client) = self.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(channel_id, "hello").await;
            }
            return;
        }

        if let Some((cmd, args)) = self.parse_command(content) {
            info!("[Webhook] 收到命令: {} (来自: {}, 频道: {})", cmd, author_id, channel_id);

            match cmd {
                "help" | "h" => {
                    self.send_help(channel_id).await;
                }
                "join" | "j" => {
                    if let Some(client) = self.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(channel_id,
                            "⚠️ 加入语音频道功能需要服务器ID，请使用 /join <频道ID> 直接指定频道").await;
                    }
                }
                "leave" | "l" => {
                    let mut vm = self.voice_manager.lock().await;
                    if let Some(ref mut voice_manager) = *vm {
                        match voice_manager.leave_channel().await {
                            Ok(_) => {
                                if let Some(client) = self.api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(channel_id, "✅ 已离开语音频道").await;
                                }
                            }
                            Err(e) => {
                                if let Some(client) = self.api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(channel_id,
                                        &format!("❌ 离开语音频道失败: {}", e)).await;
                                }
                            }
                        }
                    } else {
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id, "⚠️ 当前不在任何语音频道中").await;
                        }
                    }
                }
                "play" | "p" => {
                    if args.is_empty() {
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id,
                                "❌ 请提供搜索关键词或链接\n用法: `!play <关键词>`").await;
                        }
                    } else {
                        let query = args.join(" ");
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id,
                                &format!("🎵 搜索 \"{}\" 功能开发中...", query)).await;
                        }
                    }
                }
                _ => {
                    debug!("未知命令: {}", cmd);
                }
            }
        }
    }

    fn parse_command<'a>(&self, content: &'a str) -> Option<(&'a str, Vec<&'a str>)> {
        if !content.starts_with(&self.config.prefix) {
            return None;
        }

        let content = &content[self.config.prefix.len()..];
        let parts: Vec<&str> = content.split_whitespace().collect();

        if parts.is_empty() {
            return None;
        }

        let cmd = parts[0];
        let args = parts[1..].to_vec();

        Some((cmd, args))
    }

    async fn send_help(&self, channel_id: &str) {
        let content = r#"🎵 **Kook Music Bot** 🎵

**可用命令：**
`{}help` - 显示此帮助
`{}join` - 加入你的语音频道
`{}leave` - 离开语音频道
`{}play <关键词>` - 播放音乐（功能开发中）
`{}search <关键词>` - 搜索音乐（功能开发中）

**注意：** 目前机器人仅支持基础功能，完整功能正在开发中。
"#;

        let content = content.replace("{}", &self.config.prefix);

        if let Some(client) = self.api_client.read().await.as_ref() {
            if let Err(e) = client.send_channel_message(channel_id, &content).await {
                error!("发送帮助消息失败: {}", e);
            }
        }
    }
}

#[async_trait]
impl WebhookHandler for BotWebhookHandler {
    async fn handle_event(&self, event_type: u32, data: Value) {
        info!("[Webhook] 收到事件: type={}", event_type);
        match event_type {
            0 => {
                info!("[Webhook] 收到验证请求");
            }
            1 => {
                self.handle_message(data).await;
            }
            _ => {
                debug!("[Webhook] 收到未处理的事件类型: {}", event_type);
            }
        }
    }
}

struct BotEventHandler {
    config: BotConfig,
    api_client: Arc<RwLock<Option<KookClient>>>,
    netease_client: Arc<RwLock<NeteaseClient>>,
    voice_manager: Arc<Mutex<Option<VoiceManager>>>,
}

impl BotEventHandler {
    fn new(config: BotConfig, api_client: KookClient) -> Self {
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

        Self {
            config,
            api_client: Arc::new(RwLock::new(Some(api_client))),
            netease_client: Arc::new(RwLock::new(netease_client)),
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

    fn parse_command<'a>(&self, content: &'a str) -> Option<(&'a str, Vec<&'a str>)> {
        if !content.starts_with(&self.config.prefix) {
            return None;
        }

        let content = &content[self.config.prefix.len()..];
        let parts: Vec<&str> = content.split_whitespace().collect();

        if parts.is_empty() {
            return None;
        }

        let cmd = parts[0];
        let args = parts[1..].to_vec();

        Some((cmd, args))
    }

    async fn handle_message(&self, data: &MessageData) {
        info!("========================================");
        info!("[EventHandler] 收到消息");
        info!("========================================");
        info!("  消息类型: {:?}", data.msg_type);
        info!("  通道类型: {:?}", data.channel_type);
        info!("  作者: {} (ID: {})", data.extra.author.nickname, data.author_id);
        info!("  内容: {}", data.content);
        info!("  频道: {}", data.target_id);
        info!("  服务器: {}", data.extra.guild_id);

        if data.is_from_bot() {
            info!("  [忽略机器人消息]");
            return;
        }

        if let Some((cmd, args)) = self.parse_command(&data.content) {
            info!("[WebSocket] 收到命令: {}", cmd);

            match cmd {
                "help" | "h" => {
                    self.send_help(&data.target_id).await;
                }
                "play" | "p" => {
                    self.handle_play(&data, args).await;
                }
                "join" | "j" => {
                    self.handle_join(&data).await;
                }
                "leave" | "l" => {
                    self.handle_leave(&data).await;
                }
                "wyy" => {
                    self.handle_wyy(&data, args).await;
                }
                "wyylogin" => {
                    self.handle_wyylogin(&data).await;
                }
                _ => {
                    debug!("[WebSocket] 未知命令: {}", cmd);
                }
            }
        }
    }

    async fn handle_play(&self, data: &MessageData, args: Vec<&str>) {
        let guild_id = &data.extra.guild_id;
        let user_id = &data.author_id;
        let channel_id = &data.target_id;

        // 检查用户是否在语音频道
        let voice_channel = {
            if let Some(client) = self.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        error!("获取用户语音频道失败: {}", e);
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id,
                                &format!("❌ 获取语音频道信息失败: {}", e)).await;
                        }
                        return;
                    }
                }
            } else {
                return;
            }
        };

        match voice_channel {
            Some(vc) => {
                info!("用户 {} 在语音频道: {} ({})", user_id, vc.name, vc.id);

                // 加入语音频道
                if let Some(client) = self.api_client.read().await.as_ref() {
                    match client.join_voice_channel(&vc.id).await {
                        Ok(conn_info) => {
                            info!("成功加入语音频道: {}:{}", conn_info.ip(), conn_info.port());

                            if args.is_empty() {
                                let _ = client.send_channel_message(channel_id,
                                    &format!("✅ 已加入语音频道 **{}**\n请使用 `/play <关键词>` 播放音乐", vc.name)).await;
                            } else {
                                let query = args.join(" ");
                                let _ = client.send_channel_message(channel_id,
                                    &format!("🎵 已加入 **{}**，正在搜索 \"{}\"...", vc.name, query)).await;
                            }
                        }
                        Err(e) => {
                            error!("加入语音频道失败: {}", e);
                            let _ = client.send_channel_message(channel_id,
                                &format!("❌ 加入语音频道失败: {}", e)).await;
                        }
                    }
                }
            }
            None => {
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        "⚠️ 你当前不在任何语音频道中\n请先加入一个语音频道，然后再使用 `/play` 命令").await;
                }
            }
        }
    }

    async fn handle_join(&self, data: &MessageData) {
        let guild_id = &data.extra.guild_id;
        let user_id = &data.author_id;
        let channel_id = &data.target_id;

        // 检查用户是否在语音频道
        let voice_channel = {
            if let Some(client) = self.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        error!("获取用户语音频道失败: {}", e);
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id,
                                &format!("❌ 获取语音频道信息失败: {}", e)).await;
                        }
                        return;
                    }
                }
            } else {
                return;
            }
        };

        match voice_channel {
            Some(vc) => {
                info!("用户 {} 在语音频道: {} ({})", user_id, vc.name, vc.id);

                if let Some(client) = self.api_client.read().await.as_ref() {
                    match client.join_voice_channel(&vc.id).await {
                        Ok(conn_info) => {
                            info!("成功加入语音频道: {}:{}", conn_info.ip(), conn_info.port());
                            let _ = client.send_channel_message(channel_id,
                                &format!("✅ 已加入语音频道 **{}**", vc.name)).await;
                        }
                        Err(e) => {
                            error!("加入语音频道失败: {}", e);
                            let _ = client.send_channel_message(channel_id,
                                &format!("❌ 加入语音频道失败: {}", e)).await;
                        }
                    }
                }
            }
            None => {
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        "⚠️ 你当前不在任何语音频道中\n请先加入一个语音频道，然后再使用 `/join` 命令").await;
                }
            }
        }
    }

    async fn handle_leave(&self, data: &MessageData) {
        let channel_id = &data.target_id;

        let mut vm = self.voice_manager.lock().await;
        if let Some(ref mut voice_manager) = *vm {
            match voice_manager.leave_channel().await {
                Ok(_) => {
                    if let Some(client) = self.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(channel_id, "✅ 已离开语音频道").await;
                    }
                }
                Err(e) => {
                    if let Some(client) = self.api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(channel_id,
                            &format!("❌ 离开语音频道失败: {}", e)).await;
                    }
                }
            }
        } else {
            if let Some(client) = self.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(channel_id, "⚠️ 当前不在任何语音频道中").await;
            }
        }
    }

    async fn handle_wyy(&self, data: &MessageData, args: Vec<&str>) {
        let guild_id = &data.extra.guild_id;
        let user_id = &data.author_id;
        let channel_id = &data.target_id;

        if args.is_empty() {
            if let Some(client) = self.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(channel_id,
                    "❌ 请提供歌曲链接或搜索关键词\n用法: `/wyy <歌曲链接或关键词>`").await;
            }
            return;
        }

        let query = args.join(" ");
        info!("处理 /wyy 命令: {}", query);

        // 检查是否是歌单链接
        if let Some(playlist_id) = NeteaseClient::parse_playlist_id(&query) {
            info!("检测到歌单链接，ID: {}", playlist_id);
            self.handle_wyy_playlist(data, playlist_id).await;
            return;
        }

        // 单曲处理
        self.handle_wyy_single(data, &query).await;
    }

    async fn handle_wyy_playlist(&self, data: &MessageData, playlist_id: u64) {
        let channel_id = &data.target_id;
        let guild_id = &data.extra.guild_id;
        let user_id = &data.author_id;

        // 获取歌单详情
        let netease = self.netease_client.read().await;
        let playlist = match netease.get_playlist_detail(playlist_id).await {
            Ok(p) => p,
            Err(e) => {
                error!("获取歌单失败: {}", e);
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        &format!("❌ 获取歌单失败: {}", e)).await;
                }
                return;
            }
        };

        if playlist.track_ids.is_empty() {
            if let Some(client) = self.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(channel_id, "❌ 歌单为空").await;
            }
            return;
        }

        if let Some(client) = self.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(channel_id,
                &format!("📋 **歌单：{}**\n共 {} 首歌曲，开始播放...", playlist.name, playlist.track_ids.len())).await;
        }

        // 获取用户语音频道
        let voice_channel = {
            if let Some(client) = self.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        error!("获取用户语音频道失败: {}", e);
                        return;
                    }
                }
            } else {
                return;
            }
        };

        let vc = match voice_channel {
            Some(vc) => vc,
            None => {
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        "⚠️ 你当前不在任何语音频道中").await;
                }
                return;
            }
        };

        // 逐首播放歌曲
        for (index, track_id) in playlist.track_ids.iter().enumerate() {
            let netease = self.netease_client.read().await;

            // 获取歌曲详情
            let song = match netease.get_song_detail(*track_id).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("获取歌曲 {} 详情失败: {}", track_id, e);
                    continue;
                }
            };

            let music = netease.to_music(&song);

            // 获取歌曲 URL
            let audio_url = match netease.get_song_url(*track_id).await {
                Ok(Some(url)) => url,
                Ok(None) => {
                    warn!("歌曲 {} 无法获取播放链接", song.name);
                    continue;
                }
                Err(e) => {
                    warn!("获取歌曲 {} URL 失败: {}", song.name, e);
                    continue;
                }
            };

            // 下载歌曲
            let local_file = match netease.download_song(&audio_url, *track_id).await {
                Ok(path) => path,
                Err(e) => {
                    warn!("下载歌曲 {} 失败: {}", song.name, e);
                    continue;
                }
            };

            drop(netease); // 释放锁

            // 发送播放消息
            if let Some(client) = self.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(channel_id,
                    &format!("🎵 [{}/{}] **{}** - {}", index + 1, playlist.track_ids.len(), music.title, music.author)).await;
            }

            // 每首歌播放前重新获取推流地址（Kook 限制：同一端口只能推流一次）
            let (ip, port, streaming_info) = match self.join_voice_for_streaming(&vc.id, channel_id).await {
                Some(info) => info,
                None => break,
            };

            // 播放歌曲
            if !self.play_song(&local_file, &ip, port, &streaming_info).await {
                break;
            }
        }

        if let Some(client) = self.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(channel_id, "✅ 歌单播放完毕").await;
        }
    }

    async fn handle_wyy_single(&self, data: &MessageData, query: &str) {
        let channel_id = &data.target_id;
        let guild_id = &data.extra.guild_id;
        let user_id = &data.author_id;

        let voice_channel = {
            if let Some(client) = self.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        error!("获取用户语音频道失败: {}", e);
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id,
                                &format!("❌ 获取语音频道信息失败: {}", e)).await;
                        }
                        return;
                    }
                }
            } else {
                return;
            }
        };

        let vc = match voice_channel {
            Some(vc) => vc,
            None => {
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        "⚠️ 你当前不在任何语音频道中\n请先加入一个语音频道").await;
                }
                return;
            }
        };

        if let Some(client) = self.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(channel_id,
                &format!("🔍 正在搜索: **{}**", query)).await;
        }

        let netease = self.netease_client.read().await;
        match netease.get_or_search(query).await {
            Ok((song, url)) => {
                let music = netease.to_music(&song);

                if netease.has_cookie() {
                    info!("✅ 使用已登录的网易云账号");
                } else {
                    info!("⚠️ 未登录网易云账号，可能只能播放试听版本");
                }

                match url {
                    Some(audio_url) => {
                        info!("获取到歌曲URL: {}", audio_url);

                        let local_file = match netease.download_song(&audio_url, song.id).await {
                            Ok(path) => {
                                info!("歌曲下载成功: {}", path);
                                path
                            }
                            Err(e) => {
                                error!("下载歌曲失败: {}", e);
                                if let Some(client) = self.api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(channel_id,
                                        &format!("❌ 下载歌曲失败: {}", e)).await;
                                }
                                return;
                            }
                        };

                        // 加入语音频道并播放
                        if let Some((ip, port, streaming_info)) = self.join_voice_for_streaming(&vc.id, channel_id).await {
                            if let Some(client) = self.api_client.read().await.as_ref() {
                                let _ = client.send_channel_message(channel_id,
                                    &format!("🎵 正在播放: **{}** - {}", music.title, music.author)).await;
                            }
                            self.play_song(&local_file, &ip, port, &streaming_info).await;
                        }
                    }
                    None => {
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client.send_channel_message(channel_id,
                                &format!("❌ 无法获取 **{}** 的播放链接\n可能需要 VIP 或歌曲已下架", song.name)).await;
                        }
                    }
                }
            }
            Err(e) => {
                error!("搜索失败: {}", e);
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        &format!("❌ 搜索失败: {}", e)).await;
                }
            }
        }
    }

    /// 加入语音频道并返回流信息
    /// 每次调用都会重新获取推流地址（Kook 限制同一端口只能推流一次）
    async fn join_voice_for_streaming(&self, channel_id: &str, text_channel: &str) -> Option<(String, u16, VoiceStreamingInfo)> {
        if let Some(api_client) = self.api_client.read().await.as_ref() {
            // 先离开频道，确保获取新的推流地址
            let _ = api_client.leave_voice_channel(channel_id).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // 重新加入获取新的推流地址
            let conn_info = match api_client.join_voice_channel(channel_id).await {
                Ok(info) => info,
                Err(e) => {
                    warn!("加入语音失败: {}", e);
                    let _ = api_client.send_channel_message(text_channel,
                        &format!("❌ 加入语音频道失败: {}", e)).await;
                    return None;
                }
            };

            let ip = conn_info.ip.clone().unwrap_or_default();
            let port = conn_info.port.unwrap_or(0);
            info!("获取新推流地址: {}:{}", ip, port);

            let bit_rate = conn_info.bitrate.unwrap_or(self.config.audio.bit_rate);

            let streaming_info = VoiceStreamingInfo {
                ip: ip.clone(),
                port: port as u16,
                rtcp_port: conn_info.rtcp_port.map(|p| p as u16).unwrap_or(port as u16 + 1),
                rtcp_mux: conn_info.rtcp_mux.unwrap_or(true),
                ssrc: conn_info.audio_ssrc.map(|s| s as u32).unwrap_or(1111),
                pt: conn_info.audio_pt.map(|p| p as u8).unwrap_or(111),
                bit_rate,
                sample_rate: 48000,
                channels: 2,
            };

            return Some((ip, port as u16, streaming_info));
        }
        None
    }

    /// 播放歌曲文件
    async fn play_song(&self, file_path: &str, ip: &str, port: u16, streaming_info: &VoiceStreamingInfo) -> bool {
        let mut streamer = match FFmpegDirectStreamer::new(StreamerConfig::from(streaming_info)) {
            Ok(s) => s,
            Err(e) => {
                error!("创建流处理器失败: {}", e);
                return false;
            }
        };

        match streamer.start_stream_url(file_path, ip, port, streaming_info.rtcp_port) {
            Ok(_) => {
                let _ = streamer.wait();
                true
            }
            Err(e) => {
                error!("推流失败: {}", e);
                false
            }
        }
    }

    /// 生成二维码图片并上传到 Kook
    async fn generate_and_upload_qrcode(&self, url: &str) -> Option<String> {
        use qrcode::QrCode;
        use image::Luma;

        let code = match QrCode::new(url) {
            Ok(c) => c,
            Err(e) => {
                warn!("生成二维码失败: {}", e);
                return None;
            }
        };

        let image = code.render::<Luma<u8>>().build();
        let mut buffer = std::io::Cursor::new(Vec::new());

        if let Err(e) = image.write_to(&mut buffer, image::ImageFormat::Png) {
            warn!("编码二维码图片失败: {}", e);
            return None;
        }

        let image_data = buffer.into_inner();

        if let Some(client) = self.api_client.read().await.as_ref() {
            match client.upload_image(&image_data).await {
                Ok(kook_url) => {
                    info!("二维码上传成功: {}", kook_url);
                    return Some(kook_url);
                }
                Err(e) => {
                    warn!("上传二维码图片失败: {}", e);
                }
            }
        }

        None
    }

    async fn handle_wyylogin(&self, data: &MessageData) {
        let channel_id = &data.target_id;

        if let Some(client) = self.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(channel_id,
                "🔑 正在生成网易云登录二维码...").await;
        }

        let netease = self.netease_client.read().await;

        // 获取二维码 key
        let key = match netease.get_qr_key().await {
            Ok(key_data) => key_data.unikey,
            Err(e) => {
                error!("获取二维码key失败: {}", e);
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        &format!("❌ 获取二维码失败: {}", e)).await;
                }
                return;
            }
        };

        // 生成二维码
        let qr_code = match netease.create_qr_code(&key).await {
            Ok(qr) => qr,
            Err(e) => {
                error!("生成二维码失败: {}", e);
                if let Some(client) = self.api_client.read().await.as_ref() {
                    let _ = client.send_channel_message(channel_id,
                        &format!("❌ 生成二维码失败: {}", e)).await;
                }
                return;
            }
        };

        // 本地生成二维码图片并上传到 Kook
        info!("开始生成并上传二维码图片...");
        let image_url = self.generate_and_upload_qrcode(&qr_code.qrurl).await;
        info!("二维码上传结果: {:?}", image_url);

        // 发送二维码
        if let Some(client) = self.api_client.read().await.as_ref() {
            if let Some(ref url) = image_url {
                info!("发送图片消息: {}", url);
                let _ = client.send_image_message(channel_id, url).await;
                let _ = client.send_channel_message(channel_id,
                    "📱 **请扫描上方二维码登录网易云音乐**\n⏰ 二维码有效期 5 分钟").await;
            } else {
                warn!("二维码上传失败，发送链接");
                let _ = client.send_channel_message(channel_id,
                    &format!(
                        "📱 **网易云登录**\n\n点击链接扫码：{}\n\n⏰ 二维码有效期 5 分钟",
                        qr_code.qrurl
                    )).await;
            }
        }

        // 轮询检查登录状态
        let key_clone = key.clone();
        let channel_id_clone = channel_id.to_string();
        let api_client = self.api_client.clone();
        let netease_api_url = self.config.music.netease_api_url.clone();

        tokio::spawn(async move {
            let netease_client = NeteaseClient::new(&netease_api_url);
            let config_path = std::path::PathBuf::from("config.toml");
            let mut attempts = 0;
            let max_attempts = 60;
            let key_str = key_clone.clone();
            info!("启动登录检查任务，key: {}", key_str);

            loop {
                attempts += 1;
                info!("检查登录状态... ({}/{}), key: {}", attempts, max_attempts, key_str);

                if attempts > max_attempts {
                    if let Some(client) = api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(&channel_id_clone,
                            "⏰ 二维码已过期，请重新发送 `/wyylogin`").await;
                    }
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                match netease_client.check_qr_status(&key_clone).await {
                    Ok(result) => {
                        info!("登录状态码: {}", result.code);
                        match result.code {
                            800 => {
                                if let Some(client) = api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(&channel_id_clone,
                                        "⏰ 二维码已过期，请重新发送 `/wyylogin`").await;
                                }
                                break;
                            }
                            801 => {
                                // 等待扫码
                                info!("等待扫码中...");
                            }
                            802 => {
                                info!("已扫码，等待确认");
                                if let Some(client) = api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(&channel_id_clone,
                                        "✅ 已扫描，请在手机上确认登录").await;
                                }
                            }
                            803 => {
                                info!("登录成功! cookie: {:?}", result.cookie);
                                if let Some(ref cookie) = result.cookie {
                                    match update_netease_cookie(&config_path, cookie) {
                                        Ok(_) => {
                                            if let Some(client) = api_client.read().await.as_ref() {
                                                let nickname = result.nickname.as_deref().unwrap_or("用户");
                                                let _ = client.send_channel_message(&channel_id_clone,
                                                    &format!("🎉 登录成功！欢迎 **{}**\nCookie 已保存，请重启机器人后使用 `/wyy` 播放完整音质", nickname)).await;
                                            }
                                        }
                                        Err(e) => {
                                            error!("保存 cookie 失败: {}", e);
                                            if let Some(client) = api_client.read().await.as_ref() {
                                                let _ = client.send_channel_message(&channel_id_clone,
                                                    &format!("⚠️ 登录成功，但保存 Cookie 失败: {}", e)).await;
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                            _ => {
                                warn!("未知的登录状态码: {}", result.code);
                            }
                        }
                    }
                    Err(e) => {
                        error!("检查登录状态失败: {}", e);
                    }
                }
            }
        });
    }

    async fn send_help(&self, channel_id: &str) {
        let content = r#"🎵 **Kook Music Bot** 🎵

**可用命令：**
`{}help` - 显示此帮助
`{}join` - 加入你的语音频道
`{}leave` - 离开语音频道
`{}wyy <链接或关键词>` - 播放网易云音乐
`{}wyylogin` - 登录网易云账号（获取完整音质）

**支持：**
- 网易云音乐链接/分享链接
- 歌曲ID
- 歌曲名称搜索
"#;

        let content = content.replace("{}", &self.config.prefix);

        if let Some(client) = self.api_client.read().await.as_ref() {
            if let Err(e) = client.send_channel_message(channel_id, &content).await {
                error!("发送帮助消息失败: {}", e);
            }
        }
    }
}

#[async_trait]
impl EventHandler for BotEventHandler {
    async fn on_message(&self, data: MessageData) {
        // 处理文字消息和 KMarkdown 消息
        if data.is_text() || data.is_kmarkdown() {
            self.handle_message(&data).await;
        }
    }

    async fn on_system_message(&self, data: kook_music_bot::gateway::SystemMessageData) {
        info!("[EventHandler] 系统消息: {}", data.extra.event_type);
    }

    async fn on_unknown(&self, data: Value) {
        warn!("[EventHandler] 未知事件类型: {:?}", data.get("type"));
    }
}
