use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use kook_music_bot::api::KookClient;
use kook_music_bot::config::{BotConfig, ConnectionMode};
use kook_music_bot::webhook::{WebhookHandler, WebhookServer, WebhookConfig};
use kook_music_bot::gateway::{GatewayClient, EventHandler, Event};
use kook_music_bot::gateway::events::{ReadyEvent, MessageCreateEvent};
use kook_music_bot::player::VoiceManager;
use async_trait::async_trait;
use serde_json::Value;

/// Kook 音乐机器人
#[derive(Parser, Debug)]
#[command(name = "kook-music-bot")]
#[command(about = "Kook Music Bot - Rust 实现")]
#[command(version = "0.1.0")]
struct Cli {
    /// 创建示例配置文件
    #[arg(long)]
    init: bool,

    /// 测试 FFmpeg
    #[arg(long)]
    test_ffmpeg: bool,

    /// 配置文件路径
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    println!("╔══════════════════════════════════════╗");
    println!("║     Kook Music Bot - Rust 实现       ║");
    println!("╚══════════════════════════════════════╝");
    println!();

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

    let config_content = r#"# Kook 音乐机器人配置文件

token = "你的 Bot Token"
prefix = "!"
admins = ["你的用户ID"]

[webhook]
host = "0.0.0.0"
port = 8080
path = "/webhook"
verify_token = "你的 Webhook 验证令牌"
use_ssl = false

[audio]
volume = 0.5
bit_rate = 64000
sample_rate = 48000
channels = 2

[network]
timeout = 30
retries = 3
packet_size = 1200
"#;

    if std::path::Path::new("config.toml").exists() {
        println!("配置文件已存在！");
        return Ok(());
    }

    tokio::fs::write("config.toml", config_content).await?;
    println!("✓ 配置文件已创建: config.toml");
    println!("\n请编辑 config.toml，填写你的 Kook Bot Token 和 Webhook 验证令牌");
    println!("获取 Token: https://developer.kookapp.cn/app/index");

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
    println!("【启动机器人】\n");

    // 加载配置
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("config.toml"));
    let config = BotConfig::from_file(&config_path)?;

    println!("✓ 配置加载成功");
    println!("  命令前缀: {}", config.prefix);
    println!("  连接模式: {:?}", config.mode);

    // 检查 FFmpeg
    match tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
    {
        Ok(_) => {
            println!("✓ FFmpeg 可用");
        }
        Err(_) => {
            println!("✗ FFmpeg 未找到，语音功能可能不可用");
            println!("  请安装 FFmpeg: https://ffmpeg.org/download.html");
        }
    }

    // 根据模式启动不同的连接方式
    match config.mode {
        ConnectionMode::Webhook => {
            start_webhook_mode(config).await?;
        }
        ConnectionMode::Websocket => {
            start_websocket_mode(config).await?;
        }
    }

    Ok(())
}

/// 启动 Webhook 模式
async fn start_webhook_mode(config: BotConfig) -> Result<()> {
    let webhook_config = WebhookConfig {
        host: config.webhook.host.clone(),
        port: config.webhook.port,
        path: config.webhook.path.clone(),
        verify_token: config.webhook.verify_token.clone(),
        use_ssl: config.webhook.use_ssl,
    };

    println!("\n【启动 Webhook 服务器】");
    println!("  地址: http://{}:{}{}", webhook_config.host, webhook_config.port, webhook_config.path);

    // 创建事件处理器
    let handler = Arc::new(BotWebhookHandler::new(config.clone()));

    // 创建并启动 Webhook 服务器
    let server = WebhookServer::new(webhook_config, handler);

    println!("✓ Webhook 服务器已启动，等待 KOOK 事件...");

    server.run().await?;

    Ok(())
}

/// 启动 WebSocket 模式
async fn start_websocket_mode(config: BotConfig) -> Result<()> {
    println!("\n【启动 WebSocket 连接】");

    // 创建 API 客户端来获取 Gateway URL
    let api_client = KookClient::new(&config)?;

    // 获取 Gateway URL
    println!("  正在获取 Gateway 地址...");
    let gateway_url = api_client.get_gateway_url().await?;
    println!("  ✓ Gateway 地址: {}", gateway_url);

    // 创建 Gateway 客户端
    let token = config.token.clone();

    // 创建客户端 (使用基本意图)
    let client = GatewayClient::with_basic_intents(&token);

    // 创建事件处理器
    let handler = BotEventHandler::new(config.clone());

    // 设置事件处理器
    client.set_event_handler(Box::new(handler)).await;

    // 连接到 Gateway (传入获取到的 URL)
    client.connect(&gateway_url).await?;

    println!("✓ WebSocket 客户端已连接，等待 KOOK 事件...");

    // 运行客户端 (阻塞直到断开)
    client.run().await?;

    Ok(())
}

/// Bot Webhook 事件处理器
struct BotWebhookHandler {
    config: BotConfig,
    api_client: Arc<RwLock<Option<KookClient>>>,
    voice_manager: Arc<Mutex<Option<VoiceManager>>>,
}

impl BotWebhookHandler {
    fn new(config: BotConfig) -> Self {
        let api_client = Arc::new(RwLock::new(
            KookClient::new(&config).ok()));

        Self {
            config,
            api_client,
            voice_manager: Arc::new(Mutex::new(None)),
        }
    }

    /// 获取或创建 VoiceManager
    async fn get_or_create_voice_manager(&self) -> Result<()> {
        let mut vm = self.voice_manager.lock().await;
        if vm.is_none() {
            let new_vm = VoiceManager::new(&self.config).await?;
            *vm = Some(new_vm);
        }
        Ok(())
    }

    /// 处理消息事件
    async fn handle_message(&self, data: Value) {
        // 解析消息
        let author_id = data.get("author_id").and_then(|v| v.as_str());
        let content = data.get("content").and_then(|v| v.as_str());
        let channel_id = data.get("target_id").and_then(|v| v.as_str());

        if author_id.is_none() || content.is_none() || channel_id.is_none() {
            return;
        }

        let author_id = author_id.unwrap();
        let content = content.unwrap();
        let channel_id = channel_id.unwrap();

        // 忽略自己的消息
        if data.get("extra")
            .and_then(|e| e.get("author"))
            .and_then(|a| a.get("bot"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false)
        {
            return;
        }

        // 检查是否是命令
        if let Some((cmd, args)) = self.parse_command(content) {
            info!(
                "收到命令: {} (来自: {}, 频道: {})",
                cmd, author_id, channel_id
            );

            match cmd {
                "help" | "h" => {
                    self.send_help(channel_id).await;
                }
                "join" | "j" => {
                    // 尝试获取用户当前所在的语音频道
                    if let Some(client) = self.api_client.read().await.as_ref() {
                        // 注意：这里需要从消息中获取 guild_id
                        // 暂时返回提示信息
                        let _ = client.send_channel_message(channel_id, "⚠️ 加入语音频道功能需要服务器ID，请使用 !join <频道ID> 直接指定频道").await;
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
                                    let _ = client.send_channel_message(channel_id, &format!("❌ 离开语音频道失败: {}", e)).await;
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
                            let _ = client
                                .send_channel_message(
                                    channel_id,
                                    "❌ 请提供搜索关键词或链接\n用法: `!play <关键词>`",
                                )
                                .await;
                        }
                    } else {
                        let query = args.join(" ");
                        if let Some(client) = self.api_client.read().await.as_ref() {
                            let _ = client
                                .send_channel_message(
                                    channel_id,
                                    &format!("🎵 搜索 \"{}\" 功能开发中...", query),
                                )
                                .await;
                        }
                    }
                }
                _ => {
                    debug!("未知命令: {}", cmd);
                }
            }
        }
    }

    /// 解析命令
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

    /// 发送帮助消息
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
        match event_type {
            0 => {
                // 验证/挑战
                debug!("收到验证请求");
            }
            1 => {
                // 消息创建
                self.handle_message(data).await;
            }
            _ => {
                debug!("收到未处理的事件类型: {}", event_type);
            }
        }
    }
}

// ============================================================================
// WebSocket 模式的 BotEventHandler
// ============================================================================

/// Bot WebSocket 事件处理器
struct BotEventHandler {
    config: BotConfig,
    api_client: Arc<RwLock<Option<KookClient>>>,
}

impl BotEventHandler {
    fn new(config: BotConfig) -> Self {
        let api_client = Arc::new(RwLock::new(
            KookClient::new(&config).ok()));

        Self {
            config,
            api_client,
        }
    }

    /// 解析命令
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

    /// 处理消息事件
    async fn handle_message(&self,
        author_id: &str,
        content: &str,
        channel_id: &str,
        data: Value
    ) {
        // 忽略自己的消息
        if data.get("extra")
            .and_then(|e| e.get("author"))
            .and_then(|a| a.get("bot"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false)
        {
            return;
        }

        // 最简单的测试：收到 "test123" 回复 "test321"
        if content == "test123" {
            info!("收到 test123，准备回复 test321");
            if let Some(client) = self.api_client.read().await.as_ref() {
                let _ = client.send_channel_message(channel_id, "test321").await;
            }
            return;
        }

        // 检查是否是命令
        if let Some((cmd, _args)) = self.parse_command(content) {
            info!(
                "[WebSocket] 收到命令: {} (来自: {}, 频道: {})",
                cmd, author_id, channel_id
            );

            match cmd {
                "help" | "h" => {
                    self.send_help(channel_id).await;
                }
                _ => {
                    debug!("[WebSocket] 未知命令: {}", cmd);
                }
            }
        }
    }

    /// 发送帮助消息
    async fn send_help(&self,
        channel_id: &str
    ) {
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
    async fn on_ready(&self,
        _data: ReadyEvent
    ) {
        info!("[WebSocket] 机器人已就绪");
    }

    async fn on_message_create(&self,
        data: MessageCreateEvent
    ) {
        // 使用 handle_message 处理消息
        self.handle_message(
            &data.author.id,
            &data.content,
            &data.channel_id,
            serde_json::to_value(&data).unwrap_or_default()
        ).await;
    }
}