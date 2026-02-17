//! Gateway 协议定义

use serde::{Deserialize, Serialize};

/// Gateway 信令类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Event = 0,
    Hello = 1,
    Ping = 2,
    Pong = 3,
    Resume = 4,
    Reconnect = 5,
    ResumeAck = 6,
}

impl From<u8> for SignalType {
    fn from(value: u8) -> Self {
        match value {
            0 => SignalType::Event,
            1 => SignalType::Hello,
            2 => SignalType::Ping,
            3 => SignalType::Pong,
            4 => SignalType::Resume,
            5 => SignalType::Reconnect,
            6 => SignalType::ResumeAck,
            _ => SignalType::Event,
        }
    }
}

/// Gateway 消息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayMessage {
    /// 信令类型: 0=事件, 1=Hello, 2=Ping, 3=Pong, 4=Resume, 5=Reconnect, 6=ResumeAck
    pub s: u8,
    /// 数据内容
    pub d: serde_json::Value,
    /// 序列号 (可选)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sn: Option<u64>,
}

impl GatewayMessage {
    pub fn is_event(&self) -> bool {
        self.s == SignalType::Event as u8
    }

    pub fn is_hello(&self) -> bool {
        self.s == SignalType::Hello as u8
    }

    pub fn is_ping(&self) -> bool {
        self.s == SignalType::Ping as u8
    }

    pub fn is_pong(&self) -> bool {
        self.s == SignalType::Pong as u8
    }

    pub fn is_reconnect(&self) -> bool {
        self.s == SignalType::Reconnect as u8
    }

    pub fn pong() -> Self {
        Self {
            s: SignalType::Pong as u8,
            d: serde_json::json!({}),
            sn: None,
        }
    }

    pub fn ping() -> Self {
        Self {
            s: SignalType::Ping as u8,
            d: serde_json::json!({}),
            sn: None,
        }
    }

    pub fn resume(session_id: &str, sn: u64) -> Self {
        Self {
            s: SignalType::Resume as u8,
            d: serde_json::json!({
                "session_id": session_id,
                "sn": sn
            }),
            sn: None,
        }
    }

    pub fn heartbeat_interval(&self) -> Option<u64> {
        if self.is_hello() {
            self.d.get("heartbeat_interval")?.as_u64()
        } else {
            None
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        if self.is_hello() {
            self.d.get("session_id")?.as_str()
        } else {
            None
        }
    }
}

/// 会话信息
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub session_id: Option<String>,
    pub last_sn: u64,
}

/// 意图定义
pub struct Intents;

impl Intents {
    pub const GUILDS: u32 = 1 << 0;
    pub const GUILD_MEMBERS: u32 = 1 << 1;
    pub const GUILD_MESSAGES: u32 = 1 << 9;
    pub const DIRECT_MESSAGE: u32 = 1 << 12;
    pub const MESSAGE_REACTION: u32 = 1 << 10;
    pub const CHANNELS: u32 = 1 << 5;
    pub const GUILD_VOICE: u32 = 1 << 7;

    pub const BASIC: u32 = Self::GUILDS | Self::GUILD_MESSAGES | Self::CHANNELS;

    pub const ALL: u32 = Self::GUILDS
        | Self::GUILD_MEMBERS
        | Self::GUILD_MESSAGES
        | Self::DIRECT_MESSAGE
        | Self::MESSAGE_REACTION
        | Self::CHANNELS
        | Self::GUILD_VOICE;
}
