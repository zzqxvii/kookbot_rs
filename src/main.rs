//! Kook Bot 入口 - 负责初始化和启动

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, error, Level};
use unicode_width::UnicodeWidthStr;

use kook_music_bot::api::KookClient;
use kook_music_bot::bot::{create_bot, Bot, BotEventHandler, BotWebhookHandler};
use kook_music_bot::core::config::{BotConfig, ConnectionMode};
use kook_music_bot::common::logging::init_logging;
use kook_music_bot::common::cache;
use kook_music_bot::webhook::WebhookServer;
use kook_music_bot::gateway::GatewayClient;
use kook_music_bot::music::NeteaseClient;

const WIDTH: usize = 52;

fn w(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn center(s: &str, width: usize) -> String {
    let s_width = w(s);
    if s_width >= width {
        s.to_string()
    } else {
        let left = (width - s_width) / 2;
        let right = width - s_width - left;
        format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
    }
}

fn line(left: &str, content: &str, right: &str, width: usize) -> String {
    let content_width = w(content);
    let total = w(left) + content_width + w(right);
    if total >= width {
        format!("{}{}{}", left, content, right)
    } else {
        format!("{}{}{}{}", left, content, " ".repeat(width - total), right)
    }
}

macro_rules! box_title {
    ($title:expr) => {
        info!("╭{}╮", "─".repeat(WIDTH - 2));
        info!("│{}│", center($title, WIDTH - 2));
        info!("├{}┤", "─".repeat(WIDTH - 2));
    };
}

macro_rules! box_item {
    ($label:expr, $value:expr) => {
        info!("│ {}│", line("", &format!("{}: {}", $label, $value), "", WIDTH - 3));
    };
}

macro_rules! box_end {
    () => {
        info!("╰{}╯", "─".repeat(WIDTH - 2));
        info!("");
    };
}

macro_rules! status_ok {
    ($label:expr, $value:expr) => {
        info!("  ✅ {}: {}", $label, $value);
    };
}

macro_rules! status_fail {
    ($label:expr, $msg:expr) => {
        info!("  ❌ {}: {}", $label, $msg);
    };
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging(Level::INFO);
    
    info!("");
    info!("╭{}╮", "─".repeat(WIDTH - 2));
    info!("│{}│", center("🎵 Kook Music Bot (RKM) v0.1.0", WIDTH - 2));
    info!("│{}│", center("Rust Implementation", WIDTH - 2));
    info!("╰{}╯", "─".repeat(WIDTH - 2));
    info!("");
    
    if let Err(e) = run_bot(None).await {
        error!("");
        error!("╭{}╮", "─".repeat(WIDTH - 2));
        error!("│{}│", center("❌ 启动失败", WIDTH - 2));
        error!("╰{}╯", "─".repeat(WIDTH - 2));
        error!("  错误: {}", e);
        error!("");
        std::process::exit(1);
    }
    
    Ok(())
}

async fn run_bot(config_path: Option<PathBuf>) -> Result<()> {
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("config.toml"));
    let config = BotConfig::from_file(&config_path)?;
    
    info!("╭{}╮", "─".repeat(WIDTH - 2));
    info!("│{}│", center("⚙️ 配置信息", WIDTH - 2));
    info!("├{}┤", "─".repeat(WIDTH - 2));
    box_item!("命令前缀", config.prefix);
    box_item!("连接模式", format!("{:?}", config.mode));
    box_item!("Token", format!("{}...{}", 
        &config.token.chars().take(4).collect::<String>(),
        &config.token.chars().last().unwrap_or('?')));
    info!("╰{}╯", "─".repeat(WIDTH - 2));
    info!("");
    
    check_dependencies()?;
    check_netease_api(&config.music.netease_api_url).await?;
    cleanup_cache(&config.music.cache_dir, config.music.max_cache_size_mb).await?;
    
    let api_client = KookClient::new(&config)?;
    display_bot_info(&api_client).await;
    display_guilds(&api_client).await;
    
    let (bot, ws_handler, webhook_handler) = create_bot(config.clone(), api_client.clone());
    
    match config.mode {
        ConnectionMode::Webhook => {
            start_webhook_mode(config, webhook_handler).await?;
        }
        ConnectionMode::Websocket => {
            start_websocket_mode(config, bot, ws_handler).await?;
        }
    }
    
    Ok(())
}

async fn display_bot_info(api_client: &KookClient) {
    box_title!("🤖 机器人信息");
    
    match api_client.get_current_user().await {
        Ok(user) => {
            box_item!("ID", user.id);
            box_item!("名称", user.username);
        }
        Err(e) => {
            box_item!("获取失败", e.to_string());
        }
    }
    box_end!();
}

async fn display_guilds(api_client: &KookClient) {
    match api_client.get_guild_list().await {
        Ok(guilds) => {
            if guilds.is_empty() {
                info!("  机器人未加入任何服务器");
                info!("");
                return;
            }
            
            box_title!("📋 服务器列表");
            for guild in &guilds {
                let content = format!("• {} ({})", guild.name, guild.id);
                info!("│   {}{}│", content, " ".repeat(WIDTH - 5 - w(&content)));
            }
            box_end!();
        }
        Err(e) => {
            info!("  获取服务器列表失败: {}", e);
            info!("");
        }
    }
}

fn check_dependencies() -> Result<()> {
    info!("╭{}╮", "─".repeat(WIDTH - 2));
    info!("│{}│", center("🔍 检查依赖项", WIDTH - 2));
    info!("├{}┤", "─".repeat(WIDTH - 2));
    
    let ffmpeg_result = std::process::Command::new("ffmpeg")
        .arg("-version")
        .output();
    
    match ffmpeg_result {
        Ok(output) if output.status.success() => {
            let version_info = String::from_utf8_lossy(&output.stdout);
            let version_line = version_info.lines().next().unwrap_or("unknown");
            let version = version_line.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            status_ok!("FFmpeg", version);
        }
        Ok(_) => {
            status_fail!("FFmpeg", "执行失败");
            info!("╰{}╯", "─".repeat(WIDTH - 2));
            error!("");
            error!("FFmpeg 执行失败，请确保 FFmpeg 已正确安装");
            return Err(anyhow::anyhow!("FFmpeg 执行失败"));
        }
        Err(_) => {
            status_fail!("FFmpeg", "未安装");
            info!("╰{}╯", "─".repeat(WIDTH - 2));
            error!("");
            error!("FFmpeg 未找到，请安装 FFmpeg 并添加到 PATH 环境变量");
            return Err(anyhow::anyhow!("FFmpeg 未安装"));
        }
    }
    
    info!("╰{}╯", "─".repeat(WIDTH - 2));
    info!("");
    
    Ok(())
}

async fn check_netease_api(api_url: &str) -> Result<()> {
    info!("╭{}╮", "─".repeat(WIDTH - 2));
    info!("│{}│", center("🌐 网易云 API", WIDTH - 2));
    info!("├{}┤", "─".repeat(WIDTH - 2));
    info!("  地址: {}", api_url);
    
    let client = NeteaseClient::new(api_url);
    
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.check_api()
    ).await {
        Ok(Ok(())) => {
            status_ok!("状态", "可用");
            info!("╰{}╯", "─".repeat(WIDTH - 2));
            info!("");
            Ok(())
        }
        Ok(Err(e)) => {
            status_fail!("状态", "不可用");
            info!("╰{}╯", "─".repeat(WIDTH - 2));
            error!("");
            error!("网易云 API 不可用: {}", e);
            error!("请确保 NeteaseCloudMusicApi 已启动");
            error!("项目地址: https://github.com/Binaryify/NeteaseCloudMusicApi");
            Err(anyhow::anyhow!("网易云 API 不可用: {}", e))
        }
        Err(_) => {
            status_fail!("状态", "连接超时");
            info!("╰{}╯", "─".repeat(WIDTH - 2));
            error!("");
            error!("网易云 API 连接超时 (5秒)");
            error!("请确保 NeteaseCloudMusicApi 已启动并监听正确端口");
            Err(anyhow::anyhow!("网易云 API 连接超时"))
        }
    }
}

async fn cleanup_cache(cache_dir: &str, max_size_mb: u64) -> Result<()> {
    use std::fs;
    
    info!("╭{}╮", "─".repeat(WIDTH - 2));
    info!("│{}│", center("🧹 缓存清理", WIDTH - 2));
    info!("├{}┤", "─".repeat(WIDTH - 2));
    info!("  目录: {}", cache_dir);
    info!("  限制: {} MB", max_size_mb);
    
    let cache_path = std::path::Path::new(cache_dir);
    if !cache_path.exists() {
        fs::create_dir_all(cache_path)?;
        info!("  创建缓存目录");
        info!("╰{}╯", "─".repeat(WIDTH - 2));
        info!("");
        return Ok(());
    }
    
    let current_mb = cache::get_cache_size_mb(cache_dir);
    info!("  当前: {} MB", current_mb);
    
    cache::cleanup_cache(cache_dir, max_size_mb);
    
    let after_mb = cache::get_cache_size_mb(cache_dir);
    if after_mb < current_mb {
        info!("  清理后: {} MB (释放 {} MB)", after_mb, current_mb - after_mb);
    } else {
        info!("  状态: 无需清理");
    }
    
    info!("╰{}╯", "─".repeat(WIDTH - 2));
    info!("");
    
    Ok(())
}

async fn start_webhook_mode(config: BotConfig, handler: BotWebhookHandler) -> Result<()> {
    box_title!("🚀 启动 Webhook 服务器");
    box_item!("地址", format!("http://{}:{}", config.webhook.host, config.webhook.port));
    box_item!("路径", config.webhook.path);
    info!("├{}┤", "─".repeat(WIDTH - 2));
    info!("│{}│", center("等待 KOOK 事件...", WIDTH - 2));
    box_end!();
    
    let server = WebhookServer::new(config.webhook.clone(), Arc::new(handler));
    server.run().await.map_err(|e| anyhow::anyhow!(e))
}

async fn start_websocket_mode(
    config: BotConfig,
    bot: Arc<Bot>,
    handler: BotEventHandler,
) -> Result<()> {
    box_title!("🚀 启动 WebSocket 连接");

    let gateway_url = {
        let api_client = bot.api_client();
        let client = api_client.read().await;
        client.as_ref().unwrap().get_gateway_url().await?
    };

    let client = GatewayClient::with_all_intents(&config.token);
    client.connect(&gateway_url).await?;
    client.set_event_handler(Box::new(handler)).await;

    info!("├{}┤", "─".repeat(WIDTH - 2));
    info!("│{}│", center("✨ 已连接，等待事件...", WIDTH - 2));
    box_end!();

    client.run().await.map_err(|e| anyhow::anyhow!(e))
}
