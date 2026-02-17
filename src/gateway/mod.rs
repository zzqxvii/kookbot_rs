//! Kook Gateway WebSocket 连接模块
//!
//! 负责与 Kook Gateway 建立 WebSocket 连接，处理身份验证、心跳和事件

pub mod client;
pub mod events;
pub mod heartbeat;
pub mod protocol;

pub use client::GatewayClient;
pub use events::{Event, EventHandler, ReadyEvent, MessageCreateEvent, MessageUpdateEvent, MessageDeleteEvent,
    ChannelCreateEvent, ChannelUpdateEvent, ChannelDeleteEvent,
    GuildMemberAddEvent, GuildMemberRemoveEvent, GuildMemberUpdateEvent,
    VoiceStateUpdateEvent, UnknownEvent};
pub use protocol::{GatewayPayload, IdentifyPayload, ResumePayload, Intents};
