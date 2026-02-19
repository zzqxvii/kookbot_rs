//! Kook Bot (RKM) - 用 Rust 编写的 Kook 机器人框架
//! 
//! 本项目是一个通用的 Kook Bot 平台，采用模块化设计。
//! 音乐播放只是内置的其中一个功能模块，可以通过命令系统轻松扩展更多功能。

pub mod api;
pub mod audio;
pub mod bot;
pub mod config;
pub mod error;
pub mod gateway;
pub mod logging;
pub mod models;
pub mod music;
pub mod player;
pub mod utils;
pub mod webhook;

pub use config::BotConfig;
pub use error::{BotError, Result};
