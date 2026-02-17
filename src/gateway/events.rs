//! Gateway 事件定义和处理
//!
//! 定义 Kook Gateway 发送的各种事件

use serde::{Deserialize, Serialize};
use std::fmt;

/// 事件处理器 trait
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    /// 处理通用事件
    async fn on_event(&self, event: Event) {
        match event {
            Event::Ready(data) => self.on_ready(data).await,
            Event::Resumed => self.on_resumed().await,
            Event::MessageCreate(data) => self.on_message_create(data).await,
            Event::MessageUpdate(data) => self.on_message_update(data).await,
            Event::MessageDelete(data) => self.on_message_delete(data).await,
            Event::ChannelCreate(data) => self.on_channel_create(data).await,
            Event::ChannelUpdate(data) => self.on_channel_update(data).await,
            Event::ChannelDelete(data) => self.on_channel_delete(data).await,
            Event::GuildMemberAdd(data) => self.on_guild_member_add(data).await,
            Event::GuildMemberRemove(data) => self.on_guild_member_remove(data).await,
            Event::GuildMemberUpdate(data) => self.on_guild_member_update(data).await,
            Event::VoiceStateUpdate(data) => self.on_voice_state_update(data).await,
            Event::Unknown(data) => self.on_unknown_event(data).await,
            _ => {}
        }
    }

    /// Bot 准备好接收事件
    async fn on_ready(&self, _data: ReadyEvent) {}

    /// 会话恢复成功
    async fn on_resumed(&self) {}

    /// 收到新消息
    async fn on_message_create(&self, _data: MessageCreateEvent) {}

    /// 消息更新
    async fn on_message_update(&self, _data: MessageUpdateEvent) {}

    /// 消息删除
    async fn on_message_delete(&self, _data: MessageDeleteEvent) {}

    /// 频道创建
    async fn on_channel_create(&self, _data: ChannelCreateEvent) {}

    /// 频道更新
    async fn on_channel_update(&self, _data: ChannelUpdateEvent) {}

    /// 频道删除
    async fn on_channel_delete(&self, _data: ChannelDeleteEvent) {}

    /// 成员加入服务器
    async fn on_guild_member_add(&self, _data: GuildMemberAddEvent) {}

    /// 成员离开服务器
    async fn on_guild_member_remove(&self, _data: GuildMemberRemoveEvent) {}

    /// 成员信息更新
    async fn on_guild_member_update(&self, _data: GuildMemberUpdateEvent) {}

    /// 语音状态更新
    async fn on_voice_state_update(&self, _data: VoiceStateUpdateEvent) {}

    /// 未知事件
    async fn on_unknown_event(&self, _data: UnknownEvent) {}
}

/// 事件枚举
#[derive(Debug, Clone)]
pub enum Event {
    /// 准备好
    Ready(ReadyEvent),
    /// 会话恢复
    Resumed,
    /// 收到消息
    MessageCreate(MessageCreateEvent),
    /// 消息更新
    MessageUpdate(MessageUpdateEvent),
    /// 消息删除
    MessageDelete(MessageDeleteEvent),
    /// 频道创建
    ChannelCreate(ChannelCreateEvent),
    /// 频道更新
    ChannelUpdate(ChannelUpdateEvent),
    /// 频道删除
    ChannelDelete(ChannelDeleteEvent),
    /// 服务器成员加入
    GuildMemberAdd(GuildMemberAddEvent),
    /// 服务器成员离开
    GuildMemberRemove(GuildMemberRemoveEvent),
    /// 服务器成员更新
    GuildMemberUpdate(GuildMemberUpdateEvent),
    /// 语音状态更新
    VoiceStateUpdate(VoiceStateUpdateEvent),
    /// 未知事件
    Unknown(UnknownEvent),
    /// 心跳确认
    HeartbeatAck,
    /// 心跳发送
    HeartbeatSent,
}

// ... 事件数据结构定义（ReadyEvent, MessageCreateEvent 等）...
// 为了保持简洁，这里省略具体的事件结构定义
// 实际实现中需要包含所有事件类型的完整定义

/// 准备好事件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadyEvent {
    pub version: u32,
    pub session_id: String,
    pub user: User,
    pub guilds: Vec<Guild>,
}

/// 用户
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(rename = "avatar")]
    pub avatar: Option<String>,
    #[serde(rename = "bot")]
    pub bot: Option<bool>,
}

/// 服务器
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
}

/// 消息创建事件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageCreateEvent {
    pub id: String,
    #[serde(rename = "channel_id")]
    pub channel_id: String,
    #[serde(rename = "guild_id")]
    pub guild_id: Option<String>,
    pub author: User,
    pub content: String,
    pub timestamp: String,
    #[serde(rename = "edited_timestamp")]
    pub edited_timestamp: Option<String>,
    pub mentions: Vec<User>,
}

// 其他事件类型...（MessageUpdateEvent, ChannelCreateEvent 等）
pub type MessageUpdateEvent = MessageCreateEvent;
pub type MessageDeleteEvent = MessageCreateEvent;
pub type ChannelCreateEvent = MessageCreateEvent;
pub type ChannelUpdateEvent = MessageCreateEvent;
pub type ChannelDeleteEvent = MessageCreateEvent;

/// 成员加入服务器事件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GuildMemberAddEvent {
    pub user: User,
    #[serde(rename = "guild_id")]
    pub guild_id: String,
    pub nick: Option<String>,
    pub roles: Vec<String>,
}

// 成员事件别名
pub type GuildMemberRemoveEvent = GuildMemberAddEvent;
pub type GuildMemberUpdateEvent = GuildMemberAddEvent;

/// 语音状态更新事件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceStateUpdateEvent {
    pub user_id: String,
    #[serde(rename = "channel_id")]
    pub channel_id: Option<String>,
    #[serde(rename = "guild_id")]
    pub guild_id: String,
}

/// 未知事件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnknownEvent {
    #[serde(flatten)]
    pub data: serde_json::Value,
}
