//! Gateway 协议定义
//!
//! 定义与 Kook Gateway 通信的消息格式

use serde::{Deserialize, Serialize};

/// Gateway 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[repr(u8)]
pub enum GatewayOp {
    /// 事件 (Dispatch)
    Event = 0,
    /// 心跳
    Heartbeat = 1,
    /// 身份验证
    Identify = 2,
    /// 状态更新
    StatusUpdate = 3,
    /// 恢复连接
    Resume = 6,
    /// 重连
    Reconnect = 7,
    /// 请求 Guild 成员
    RequestGuildMembers = 8,
    /// 无效会话
    InvalidSession = 9,
    /// Hello
    Hello = 10,
    /// 心跳确认
    HeartbeatAck = 11,
    /// HTTP 回调 Ack
    HttpCallbackAck = 12,
}

/// Gateway 负载数据
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayPayload {
    /// 操作码
    pub op: u8,
    /// 事件数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<serde_json::Value>,
    /// 序列号 (用于恢复会话)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<u64>,
    /// 事件名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

impl GatewayPayload {
    /// 创建心跳消息
    pub fn heartbeat(s: Option<u64>) -> Self {
        Self {
            op: GatewayOp::Heartbeat as u8,
            d: s.map(|seq| serde_json::json!(seq)),
            s: None,
            t: None,
        }
    }

    /// 创建身份验证消息
    pub fn identify(token: &str, intents: u32) -> Self {
        Self {
            op: GatewayOp::Identify as u8,
            d: Some(serde_json::json!({
                "token": format!("Bot {}", token),
                "intents": intents,
                "properties": {
                    "os": "windows",
                    "browser": "kook-music-bot",
                    "device": "kook-music-bot"
                },
                "compress": false,
                "large_threshold": 250
            })),
            s: None,
            t: None,
        }
    }

    /// 创建恢复会话消息
    pub fn resume(token: &str, session_id: &str, seq: u64) -> Self {
        Self {
            op: GatewayOp::Resume as u8,
            d: Some(serde_json::json!({
                "token": format!("Bot {}", token),
                "session_id": session_id,
                "seq": seq
            })),
            s: None,
            t: None,
        }
    }

    /// 检查是否是心跳确认
    pub fn is_heartbeat_ack(&self) -> bool {
        self.op == GatewayOp::HeartbeatAck as u8
    }

    /// 检查是否是 Hello 消息
    pub fn is_hello(&self) -> bool {
        self.op == GatewayOp::Hello as u8
    }

    /// 检查是否是重连请求
    pub fn is_reconnect(&self) -> bool {
        self.op == GatewayOp::Reconnect as u8
    }

    /// 检查是否是无效会话
    pub fn is_invalid_session(&self) -> bool {
        self.op == GatewayOp::InvalidSession as u8
    }

    /// 获取心跳间隔 (从 Hello 消息中)
    pub fn heartbeat_interval(&self) -> Option<u64> {
        if self.is_hello() {
            self.d.as_ref()?.get("heartbeat_interval")?.as_u64()
        } else {
            None
        }
    }
}

/// 身份验证负载
#[derive(Debug, Clone, Serialize)]
pub struct IdentifyPayload {
    pub token: String,
    pub intents: u32,
    pub properties: serde_json::Value,
    pub compress: bool,
    #[serde(rename = "large_threshold")]
    pub large_threshold: u32,
}

/// 恢复会话负载
#[derive(Debug, Clone, Serialize)]
pub struct ResumePayload {
    pub token: String,
    pub session_id: String,
    pub seq: u64,
}

/// Gateway 会话信息
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    /// 会话 ID
    pub session_id: Option<String>,
    /// 最后收到的序列号
    pub last_seq: u64,
    /// 是否已认证
    pub authenticated: bool,
}

/// 意图 (Intents) 定义
pub struct Intents;

impl Intents {
    /// 服务器相关事件
    pub const GUILDS: u32 = 1 << 0;
    /// 服务器成员相关事件
    pub const GUILD_MEMBERS: u32 = 1 << 1;
    /// 消息相关事件
    pub const GUILD_MESSAGES: u32 = 1 << 9;
    /// 私信相关事件
    pub const DIRECT_MESSAGE: u32 = 1 << 12;
    /// 消息反馈相关事件
    pub const MESSAGE_REACTION: u32 = 1 << 10;
    /// 频道相关事件
    pub const CHANNELS: u32 = 1 << 5;
    /// 服务器语音频道事件
    pub const GUILD_VOICE: u32 = 1 << 7;
    /// 私信频道事件
    pub const DIRECT_MESSAGES: u32 = 1 << 12;

    /// 常用组合：基础消息和服务器事件
    pub const BASIC: u32 = Self::GUILDS | Self::GUILD_MESSAGES | Self::CHANNELS;

    /// 完整意图（包括所有事件）
    pub const ALL: u32 = Self::GUILDS
        | Self::GUILD_MEMBERS
        | Self::GUILD_MESSAGES
        | Self::DIRECT_MESSAGE
        | Self::MESSAGE_REACTION
        | Self::CHANNELS
        | Self::GUILD_VOICE;
}
