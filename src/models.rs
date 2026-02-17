use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KookResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceConnectionInfo {
    pub ip: String,
    pub port: u16,
    #[serde(rename = "rtcp_port")]
    pub rtcp_port: u16,
    #[serde(rename = "bitrate")]
    pub bit_rate: i32,
    pub ssrc: u32,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum VoiceChannelStatus {
    SenderNotInChannel,
    BotNotInChannel,
    SameChannel,
    DifferentChannel,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JoinedChannel {
    pub id: String,
    pub name: String,
    #[serde(rename = "user_id")]
    pub user_id: String,
    #[serde(rename = "guild_id")]
    pub guild_id: String,
    #[serde(rename = "channel_type")]
    pub channel_type: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(rename = "avatar")]
    pub avatar: Option<String>,
}
