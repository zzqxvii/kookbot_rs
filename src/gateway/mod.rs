//! Kook Gateway WebSocket 连接模块

pub mod client;
pub mod events;
pub mod protocol;

pub use client::GatewayClient;
pub use events::{
    Author, ButtonClickBody, ButtonClickData, ButtonClickExtra, ButtonClickUserInfo, ChannelType,
    EmojiInfo, Event, EventHandler, MessageData, MessageExtra, MessageType, ReactionEventBody,
    ReactionEventData, ReactionEventExtra, SystemMessageData, SystemMessageExtra,
    VoiceChannelEventBody, VoiceChannelEventData, VoiceChannelEventExtra,
};
pub use protocol::{GatewayMessage, Intents, SessionInfo, SignalType};
