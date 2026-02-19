//! Kook Bot (RKM) - 用 Rust 编写的 Kook 机器人框架
//! 
//! 本项目是一个通用的 Kook Bot 平台，采用模块化设计。
//! 音乐播放只是内置的其中一个功能模块，可以通过命令系统轻松扩展更多功能。

pub mod api;
pub mod audio;
pub mod bot;
pub mod common;
pub mod core;
pub mod gateway;
pub mod music;
pub mod player;
pub mod webhook;

// 重新导出核心类型，方便使用
pub use core::{BotConfig, BotError, Result};
