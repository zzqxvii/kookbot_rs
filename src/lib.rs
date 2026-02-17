//! Kook Music Bot - Rust 实现的 Kook 音乐机器人
//!
//! 功能：
//! - Webhook 事件接收
//! - 语音频道加入/离开
//! - 音乐播放

pub mod api;
pub mod audio;
pub mod config;
pub mod error;
pub mod gateway;
pub mod models;
pub mod utils;
pub mod voice;
pub mod webhook;
pub mod queue;
pub mod playlist;
pub mod preloader;
pub mod music_api;

// 导出常用类型
pub use config::BotConfig;
pub use error::{BotError, Result};
