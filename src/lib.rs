//! Kook Music Bot - Rust 实现的 Kook 音乐机器人

pub mod api;
pub mod audio;
pub mod config;
pub mod error;
pub mod gateway;
pub mod models;
pub mod music;
pub mod player;
pub mod utils;
pub mod webhook;

pub use config::BotConfig;
pub use error::{BotError, Result};
