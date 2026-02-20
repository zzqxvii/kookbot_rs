//! Webhook 事件处理器
//!
//! 处理接收到的 KOOK Webhook 事件

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info};

/// Webhook 请求体
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookRequest {
    /// 事件类型
    #[serde(rename = "type")]
    pub event_type: u32,
    /// 事件数据
    #[serde(rename = "d")]
    pub data: Value,
}

/// 事件处理器 trait
#[async_trait]
pub trait WebhookHandler: Send + Sync {
    /// 处理 Webhook 事件
    async fn handle_event(&self, event_type: u32, data: Value);

    /// 处理心跳/验证 (type = 0)
    async fn handle_challenge(&self, _challenge: &str) {
        debug!("收到验证请求");
    }
}

/// 默认处理器实现
pub struct DefaultWebhookHandler;

#[async_trait]
impl WebhookHandler for DefaultWebhookHandler {
    async fn handle_event(&self, event_type: u32, data: Value) {
        match event_type {
            0 => debug!("收到验证请求"),
            _ => {
                info!("收到事件类型 {}: {:?}", event_type, data);
            }
        }
    }
}

/// 事件类型常量
pub mod event_type {
    /// 验证/挑战
    pub const CHALLENGE: u32 = 0;
    /// 消息创建
    pub const MESSAGE_CREATE: u32 = 1;
    /// 消息更新
    pub const MESSAGE_UPDATE: u32 = 2;
    /// 消息删除
    pub const MESSAGE_DELETE: u32 = 3;
    /// 频道创建
    pub const CHANNEL_CREATE: u32 = 4;
    /// 频道更新
    pub const CHANNEL_UPDATE: u32 = 5;
    /// 频道删除
    pub const CHANNEL_DELETE: u32 = 6;
    /// 成员加入
    pub const GUILD_MEMBER_ADD: u32 = 7;
    /// 成员离开
    pub const GUILD_MEMBER_REMOVE: u32 = 8;
    /// 成员更新
    pub const GUILD_MEMBER_UPDATE: u32 = 9;
    /// 语音状态更新
    pub const VOICE_STATE_UPDATE: u32 = 12;
}
