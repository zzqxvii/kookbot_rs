//! Kook Bot 入口 - 负责初始化和启动
//! 
//! 这是程序的入口点，仅包含启动逻辑：
//! 1. 初始化日志系统
//! 2. 加载配置
//! 3. 创建 Bot 实例
//! 4. 启动连接（WebSocket 或 Webhook）
//! 
//! 所有业务逻辑（命令处理等）都在 bot 模块中实现

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};

use kook_music_bot::api::KookClient;
use kook_music_bot::bot::{create_bot, Bot, BotEventHandler, BotWebhookHandler};
use kook_music_bot::core::config::{BotConfig, ConnectionMode};
use kook_music_bot::common::logging::init_logging;
use kook_music_bot::webhook::WebhookServer;
use kook_music_bot::gateway::GatewayClient;

#[tokio::main]
async fn main() -> Result<()> {
    init_logging(Level::INFO);
    
    info!("========================================");
    info!("Kook Bot (RKM) - Rust 实现 v0.1.0");
    info!("========================================");
    
    run_bot(None).await
}

async fn run_bot(config_path: Option<PathBuf>) -> Result<()> {
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("config.toml"));
    let config = BotConfig::from_file(&config_path)?;
    
    info!("✓ 配置加载成功");
    info!("  命令前缀: {}", config.prefix);
    info!("  连接模式: {:?}", config.mode);
    info!("  Token: {}...{}",
        &config.token.chars().take(4).collect::<String>(),
        &config.token.chars().last().unwrap_or('?'));
    
    // 创建 API 客户端
    let api_client = KookClient::new(&config)?;
    
    // 显示机器人信息
    display_bot_info(&api_client).await;
    
    // 显示服务器列表
    display_guilds(&api_client).await;
    
    // 创建 Bot 实例和事件处理器
    let (bot, ws_handler, webhook_handler) = create_bot(config.clone(), api_client.clone());
    
    // 根据连接模式启动
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
            info!("获取用户信息失败: {}", e);
        }
    }
}

async fn display_guilds(api_client: &KookClient) {
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
                    
                    // 获取频道列表
                    match api_client.get_channel_list(&guild.id).await {
                        Ok(channels) => {
                            let text_count = channels.iter()
                                .filter(|c| c.channel_type == 1 && !c.is_category)
                                .count();
                            let voice_count = channels.iter()
                                .filter(|c| c.channel_type == 2 && !c.is_category)
                                .count();
                            let category_count = channels.iter()
                                .filter(|c| c.is_category)
                                .count();
                            
                            info!("    频道: {} 文字, {} 语音, {} 分类",
                                text_count, voice_count, category_count);
                        }
                        Err(e) => {
                            info!("    获取频道列表失败: {}", e);
                        }
                    }
                }
                info!("----------------------------------------");
            }
        }
        Err(e) => {
            info!("获取服务器列表失败: {}", e);
        }
    }
}

async fn start_webhook_mode(config: BotConfig, handler: BotWebhookHandler) -> Result<()> {
    info!("========================================");
    info!("启动 Webhook 服务器");
    info!("========================================");
    info!("地址: http://{}:{}", config.webhook.host, config.webhook.port);
    info!("路径: {}", config.webhook.path);
    
    let server = WebhookServer::new(config.webhook.clone(), Arc::new(handler));
    
    info!("✓ Webhook 服务器已启动，等待 KOOK 事件...");
    info!("========================================");
    
    server.run().await.map_err(|e| anyhow::anyhow!(e))
}

async fn start_websocket_mode(
    config: BotConfig,
    bot: Arc<Bot>,
    handler: BotEventHandler,
) -> Result<()> {
    info!("========================================");
    info!("启动 WebSocket 连接");
    info!("========================================");

    // 获取 Gateway 地址
    let gateway_url = {
        let api_client = bot.api_client();
        let client = api_client.read().await;
        client.as_ref().unwrap().get_gateway_url().await?
    };
    info!("✓ Gateway 地址: {}", gateway_url);

    // 创建 Gateway 客户端并连接
    let client = GatewayClient::with_all_intents(&config.token);
    client.connect(&gateway_url).await?;

    // 设置事件处理器
    client.set_event_handler(Box::new(handler)).await;

    info!("✓ WebSocket 客户端已连接");
    info!("========================================");

    client.run().await.map_err(|e| anyhow::anyhow!(e))
}
