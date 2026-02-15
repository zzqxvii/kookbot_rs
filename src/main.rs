use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::{error, info, warn};

mod api;
mod audio;
mod config;
mod error;
mod models;
mod music_api;
mod playlist;
mod preloader;
mod queue;
mod utils;
mod voice;

use voice::VoiceManager;

/// Kook 音乐机器人
#[derive(Parser, Debug)]
#[command(name = "kook-music-bot")]
#[command(about = "Kook Music Bot - Pure Rust implementation without FFmpeg")]
#[command(version = "0.1.0")]
struct Cli {
    /// 配置文件路径
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// 创建示例配置文件
    #[arg(long)]
    init: bool,

    /// 日志级别
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// 测试模式: 加入频道、播放测试音频、然后离开
    #[arg(long)]
    test_stream: bool,

    /// 测试音频文件路径
    #[arg(long, default_value = "test.mp3")]
    test_file: String,

    /// 频道 ID
    #[arg(short, long)]
    channel_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 初始化日志
    init_logging(&cli.log_level)?;

    info!("Kook Music Bot 启动中...");

    // 处理初始化命令
    if cli.init {
        init_config().await?;
        return Ok(());
    }

    // 加载配置
    let config = load_config(&cli).await?;

    // 测试模式
    if cli.test_stream {
        run_test_mode(&config, &cli).await?;
        return Ok(());
    }

    // 正常运行模式
    run_bot(&config).await?;

    Ok(())
}

/// 初始化日志系统
fn init_logging(log_level: &str) -> Result<()> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("kook_music_bot={}", log_level)));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_line_number(true),
        )
        .with(env_filter)
        .init();

    Ok(())
}

/// 初始化配置文件
async fn init_config() -> Result<()> {
    let config = config::BotConfig::create_example();

    // 确定配置文件路径
    let config_path = if let Some(default_path) = config::BotConfig::default_path() {
        // 确保目录存在
        if let Some(parent) = default_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        default_path
    } else {
        PathBuf::from("config.toml")
    };

    // 检查文件是否已存在
    if config_path.exists() {
        warn!("配置文件已存在: {:?}", config_path);
        print!("是否覆盖? [y/N]: ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            info!("取消配置文件创建");
            return Ok(());
        }
    }

    // 保存配置文件
    config.save_to_file(&config_path)?;
    info!("配置文件已创建: {:?}", config_path);

    println!("\n配置文件已创建: {:?}", config_path);
    println!("请编辑配置文件，设置你的 Kook Bot Token 和其他选项。");

    Ok(())
}

/// 加载配置
async fn load_config(cli: &Cli) -> Result<config::BotConfig> {
    // 确定配置文件路径
    let config_path = if let Some(ref path) = cli.config {
        path.clone()
    } else if let Some(default_path) = config::BotConfig::default_path() {
        default_path
    } else {
        PathBuf::from("config.toml")
    };

    info!("加载配置文件: {:?}", config_path);

    // 加载配置
    let config = config::BotConfig::from_file(&config_path)
        .map_err(|e| {
            error!("加载配置文件失败: {}", e);
            eprintln!("错误: 无法加载配置文件");
            eprintln!("提示: 运行 `kook-music-bot --init` 创建示例配置文件");
            e
        })?;

    info!("配置文件加载成功");
    Ok(config)
}

/// 运行测试模式
async fn run_test_mode(config: &config::BotConfig, cli: &Cli) -> Result<()> {
    info!("运行测试模式");

    // 获取频道 ID
    let channel_id = cli.channel_id.as_ref()
        .ok_or_else(|| anyhow::anyhow!("测试模式需要指定频道 ID，使用 --channel-id"))?;

    // 创建语音管理器
    let mut voice_manager = VoiceManager::new(config).await?;

    // 加入频道
    info!("加入频道: {}", channel_id);
    voice_manager.join_channel(channel_id).await?;

    // 播放测试文件
    let test_file = &cli.test_file;
    info!("播放测试文件: {}", test_file);

    if let Err(e) = voice_manager.play_file(test_file).await {
        error!("播放文件失败: {}", e);
    }

    // 等待一段时间
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // 离开频道
    info!("离开频道");
    voice_manager.leave_channel().await?;

    info!("测试模式完成");
    Ok(())
}

/// 运行机器人主循环
async fn run_bot(_config: &config::BotConfig) -> Result<()> {
    info!("启动 Kook 音乐机器人");

    // TODO: 实现完整的机器人逻辑
    // 包括：
    // 1. 连接 Kook WebSocket
    // 2. 监听命令
    // 3. 管理播放队列
    // 4. 处理语音频道操作

    info!("机器人运行中...");

    // 保持运行
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}