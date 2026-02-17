//! Kook Gateway WebSocket 连接模块

pub mod client;
pub mod events;
pub mod protocol;

pub use client::GatewayClient;
pub use events::{
    Author, ChannelType, Event, EventHandler, MessageData, MessageExtra, MessageType,
    SystemMessageData, SystemMessageExtra,
};
pub use protocol::{GatewayMessage, Intents, SessionInfo, SignalType};
