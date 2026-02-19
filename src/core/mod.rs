//! 核心模块 - Bot 的基础设施
//!
//! 包含 Bot 运行所需的核心组件：
//! - config: 配置管理
//! - error: 错误定义

pub mod config;
pub mod error;

pub use config::BotConfig;
pub use error::{BotError, Result};
