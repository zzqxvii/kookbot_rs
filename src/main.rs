use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn, Level};
use std::fmt;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

use kook_music_bot::api::{Channel, KookClient};
use kook_music_bot::config::{BotConfig, ConnectionMode};
use kook_music_bot::webhook::{WebhookHandler, WebhookServer};
use kook_music_bot::gateway::{EventHandler, GatewayClient, MessageData};
use kook_music_bot::player::VoiceManager;
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
        
        // 级别 (固定宽度 5)
        let level_str = match *meta.level() {
            tracing::Level::TRACE => "TRACE",
            tracing::Level::DEBUG => "DEBUG",
            tracing::Level::INFO => "INFO ",
            tracing::Level::WARN => "WARN ",
            tracing::Level::ERROR => "ERROR",
        };
        write!(writer, "{} ", level_str)?;
        
        // 时间
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        write!(writer, "{} ", timestamp)?;
        
        // 文件和行号 (路径固定宽度，行号紧跟)
        let file = meta.file().unwrap_or("unknown");
        let line = meta.line().unwrap_or(0);
        let location = if file.len() > 18 {
            format!("...{}:{}", &file[file.len()-15..], line)
        } else {
            format!("{}:{}", file, line)
        };
        write!(writer, "{:<22} ", location)?;
        
        // 消息
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Kook 音乐机器人
#[derive(Parser, Debug)]
#[command(name = "kook-music-bot")]
#[command(about = "Kook Music Bot - Rust 实现")]
#[command(version = "0.1.0")]
struct Cli {
    #[arg(long)]
    init: bool,
    #[arg(long)]
    test_ffmpeg: bool,
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = match cli.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .event_format(AlignedFormatter)
        .init();

    info!("Kook Music Bot - Rust 实现 v0.1.0");
    info!("日志级别: {}", cli.log_level);

    if cli.init {
        create_config().await?;
        return Ok(());
    }

    if cli.test_ffmpeg {
        test_ffmpeg().await?;
        return Ok(());
    }

    run_bot(cli.config).await?;
    Ok(())
}

async fn create_config() -> Result<()> {
    println!("【创建配置文件】\n");

    if std::path::Path::new("config.toml").exists() {
        println!("配置文件已存在！");
        return Ok(());
    }

    tokio::fs::copy("config.example.toml", "config.toml").await?;
    println!("✓ 配置文件已创建: config.toml");
    println!("\n请编辑 config.toml，填写以下内容：");
    println!("  - token: Kook Bot Token");
    println!("  - mode: 连接模式 (websocket/webhook)");
    println!("  - webhook.verify_token: Webhook 验证令牌 (webhook 模式需要)");
    println!("\n获取 Token: https://developer.kookapp.cn/app/index");

    Ok(())
}

async fn test_ffmpeg() -> Result<()> {
    println!("【FFmpeg 测试】\n");

    let output = tokio::process::Command::new("ffmpeg")
        .args(["-version"])
        .output()
        .await?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout);
        println!("FFmpeg 版本:");
        for line in version.lines().take(3) {
            println!("  {}", line);
        }
        println!();
    }

    println!("检查 Opus 编码器支持...");
    let output = tokio::process::Command::new("ffmpeg")
        .args(["-encoders"])
        .output()
        .await?;

    let encoders = String::from_utf8_lossy(&output.stdout);
    if encoders.contains("libopus") {
        println!("  ✓ libopus 编码器可用");
    } else {
        println!("  ✗ libopus 编码器不可用");
    }

    println!();
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
    let client = GatewayClient::with_basic_intents(&token);

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
}

impl BotEventHandler {
    fn new(config: BotConfig, api_client: KookClient) -> Self {
        Self {
            config,
            api_client: Arc::new(RwLock::new(Some(api_client))),
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
                            info!("成功加入语音频道: {}:{}", conn_info.ip, conn_info.port);
                            
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
                            info!("成功加入语音频道: {}:{}", conn_info.ip, conn_info.port);
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

    async fn send_help(&self, channel_id: &str) {
        let content = r#"🎵 **Kook Music Bot (WebSocket)** 🎵

**可用命令：**
`{}help` - 显示此帮助
`{}join` - 加入你的语音频道
`{}leave` - 离开语音频道
`{}play <关键词>` - 播放音乐

**连接模式：** WebSocket
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
