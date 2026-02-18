use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KookResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

/// 语音连接信息
///
/// 参考: https://developer.kookapp.cn/doc/http/voice#加入语音频道
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceConnectionInfo {
    /// 媒体服务器的推流ip
    pub ip: String,
    /// 媒体服务器的推流端口
    pub port: i32,
    /// 媒体服务器的rtcp推流端口
    #[serde(default)]
    pub rtcp_port: Option<i32>,
    /// 是否将rtcp与rtp使用同一个端口进行传输
    #[serde(default)]
    pub rtcp_mux: Option<bool>,
    /// 当前语音房间要求的比特率
    #[serde(default)]
    pub bitrate: Option<i32>,
    /// 传输的语音数据的ssrc
    #[serde(default)]
    pub audio_ssrc: Option<i32>,
    /// 传输的语音数据的payload_type
    #[serde(default)]
    pub audio_pt: Option<i32>,
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
