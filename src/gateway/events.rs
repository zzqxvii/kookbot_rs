//! Gateway 事件定义
//!
//! # 事件结构/格式说明
//!
//! 当 websocket 或 webhook 收到 s=0 的消息时，代表当前收到的消息是事件
//! (包含用户的聊天消息及系统的通知消息等)。
//!
//! ## 事件基本结构
//!
//! ```json
//! {
//!   "s": 0,      // 信令类型
//!   "d": {},     // 数据
//!   "sn": 1      // 序列号(可选)
//! }
//! ```
//!
//! ## 事件类型判断逻辑
//!
//! 1. extra.type == "12" -> ItemConsumedEvent (道具消耗)
//! 2. extra.type 在 EventTypeMap 中 -> 系统事件 (type=255)
//! 3. extra.type 是数字 (1-10) -> 普通消息事件

use serde::{Deserialize, Serialize};

/// 消息通道类型
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ChannelType {
    GROUP,
    PERSON,
    BROADCAST,
}

/// 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MessageType {
    Text = 1,
    Image = 2,
    Video = 3,
    File = 4,
    Audio = 8,
    KMarkdown = 9,
    Card = 10,
    System = 255,
    Unknown = 0,
}

impl<'de> Deserialize<'de> for MessageType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let num = match value {
            serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
            serde_json::Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        Ok(Self::from(num as u8))
    }
}

impl From<u8> for MessageType {
    fn from(value: u8) -> Self {
        match value {
            1 => MessageType::Text,
            2 => MessageType::Image,
            3 => MessageType::Video,
            4 => MessageType::File,
            8 => MessageType::Audio,
            9 => MessageType::KMarkdown,
            10 => MessageType::Card,
            255 => MessageType::System,
            _ => MessageType::Unknown,
        }
    }
}

/// 事件处理器 Trait
///
/// 实现此 trait 以处理各种事件。
///
/// # 示例
///
/// ```rust,ignore
/// struct MyHandler;
///
/// #[async_trait]
/// impl EventHandler for MyHandler {
///     async fn on_message(&self, data: MessageData) {
///         println!("收到消息: {}", data.content);
///     }
///
///     async fn on_button_click(&self, data: ButtonClickData) {
///         println!("按钮被点击: {}", data.extra.body.value);
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    /// 默认事件分发方法
    async fn on_event(&self, event: Event) {
        match event {
            Event::Message(data) => self.on_message(data).await,
            Event::SystemMessage(data) => self.on_system_message(data).await,
            Event::ButtonClick(data) => self.on_button_click(data).await,
            Event::UserJoinVoice(data) => self.on_user_join_voice(data).await,
            Event::UserLeaveVoice(data) => self.on_user_leave_voice(data).await,
            Event::UserAddReaction(data) => self.on_user_add_reaction(data).await,
            Event::UserRemoveReaction(data) => self.on_user_remove_reaction(data).await,
            Event::Unknown(data) => self.on_unknown(data).await,
        }
    }

    /// 收到消息事件 (type: 1-10)
    async fn on_message(&self, _data: MessageData) {}

    /// 收到系统事件 (type: 255)
    async fn on_system_message(&self, _data: SystemMessageData) {}

    /// 按钮点击事件 (extra.type: message_btn_click)
    async fn on_button_click(&self, _data: ButtonClickData) {}

    /// 用户加入语音频道 (extra.type: joined_channel)
    async fn on_user_join_voice(&self, _data: VoiceChannelEventData) {}

    /// 用户离开语音频道 (extra.type: exited_channel)
    async fn on_user_leave_voice(&self, _data: VoiceChannelEventData) {}

    /// 用户添加表情 (extra.type: added_reaction)
    async fn on_user_add_reaction(&self, _data: ReactionEventData) {}

    /// 用户删除表情 (extra.type: deleted_reaction)
    async fn on_user_remove_reaction(&self, _data: ReactionEventData) {}

    /// 收到未知类型事件
    async fn on_unknown(&self, _data: serde_json::Value) {}
}

/// 事件枚举
#[derive(Debug, Clone)]
pub enum Event {
    /// 消息事件 (type: 1=文字, 2=图片, 3=视频, 4=文件, 8=音频, 9=KMarkdown, 10=Card)
    Message(MessageData),
    /// 系统事件 (type: 255, 未识别的具体类型)
    SystemMessage(SystemMessageData),
    /// 按钮点击事件
    ButtonClick(ButtonClickData),
    /// 用户加入语音频道
    UserJoinVoice(VoiceChannelEventData),
    /// 用户离开语音频道
    UserLeaveVoice(VoiceChannelEventData),
    /// 用户添加表情
    UserAddReaction(ReactionEventData),
    /// 用户删除表情
    UserRemoveReaction(ReactionEventData),
    /// 未知事件
    Unknown(serde_json::Value),
}

/// 消息数据 (s=0, type 非255)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageData {
    /// 消息通道类型: GROUP/PERSON/BROADCAST
    pub channel_type: ChannelType,
    /// 消息类型: 1=文字, 2=图片, 3=视频, 4=文件, 8=音频, 9=KMarkdown, 10=Card
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// 目标ID: 频道消息时为 channel_id
    pub target_id: String,
    /// 发送者ID
    pub author_id: String,
    /// 消息内容
    pub content: String,
    /// 消息ID
    pub msg_id: String,
    /// 消息发送时间的毫秒时间戳
    pub msg_timestamp: i64,
    /// 随机串
    #[serde(default)]
    pub nonce: String,
    /// 额外信息
    pub extra: MessageExtra,
    /// 其他未知字段
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// 消息额外信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageExtra {
    /// 消息类型
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// 服务器ID
    pub guild_id: String,
    /// 频道名称
    #[serde(default)]
    pub channel_name: String,
    /// 提及到的用户ID列表
    #[serde(default)]
    pub mention: Vec<String>,
    /// 是否 @所有人
    #[serde(default)]
    pub mention_all: bool,
    /// 提及的角色ID数组
    #[serde(default)]
    pub mention_roles: Vec<String>,
    /// 是否 mention 在线用户
    #[serde(default)]
    pub mention_here: bool,
    /// 作者信息
    pub author: Author,
    /// 其他未知字段
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// 用户信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Author {
    /// 用户ID
    pub id: String,
    /// 用户名
    pub username: String,
    /// 昵称
    #[serde(default)]
    pub nickname: String,
    /// 识别码
    #[serde(default)]
    pub identify_num: String,
    /// 是否在线
    #[serde(default)]
    pub online: bool,
    /// 是否机器人
    #[serde(default)]
    pub bot: bool,
    /// 状态
    #[serde(default)]
    pub status: i32,
    /// 头像
    #[serde(default)]
    pub avatar: String,
    /// 是否VIP
    #[serde(default)]
    pub is_vip: bool,
    /// 是否音乐会员
    #[serde(default)]
    pub is_music_vip: bool,
}

/// 系统消息数据 (s=0, type=255)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMessageData {
    /// 消息通道类型
    pub channel_type: String,
    /// 目标ID (系统消息时为 guild_id)
    pub target_id: String,
    /// 发送者ID (系统消息为 "1")
    pub author_id: String,
    /// 消息内容
    #[serde(default)]
    pub content: String,
    /// 消息ID
    #[serde(default)]
    pub msg_id: String,
    /// 消息时间戳(毫秒)
    pub msg_timestamp: i64,
    /// 系统事件额外信息
    pub extra: SystemMessageExtra,
}

/// 系统消息额外信息 (type=255)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMessageExtra {
    /// 事件类型标识
    #[serde(rename = "type")]
    pub event_type: String,
    /// 服务器ID
    #[serde(default)]
    pub guild_id: String,
    /// 事件关联的具体数据
    #[serde(default)]
    pub body: serde_json::Value,
}

/// 按钮点击事件数据 (type=255, extra.type="message_btn_click")
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ButtonClickData {
    /// 消息通道类型: GROUP/PERSON
    pub channel_type: String,
    /// 消息类型 (255 = 系统消息)
    #[serde(rename = "type")]
    pub msg_type: u8,
    /// 目标ID (私聊时与user_id相同，频道消息时为频道ID)
    pub target_id: String,
    /// 发送者ID (系统消息为 "1")
    pub author_id: String,
    /// 消息内容
    #[serde(default)]
    pub content: String,
    /// 消息ID (顶层的msg_id)
    #[serde(default)]
    pub msg_id: String,
    /// 消息时间戳(毫秒)
    pub msg_timestamp: i64,
    /// 随机串
    #[serde(default)]
    pub nonce: String,
    /// 按钮点击详情
    pub extra: ButtonClickExtra,
}

/// 按钮点击事件额外信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ButtonClickExtra {
    /// 事件类型 "message_btn_click"
    #[serde(rename = "type")]
    pub event_type: String,
    /// 按钮点击详情
    pub body: ButtonClickBody,
}

/// 按钮点击信息 (extra.body)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ButtonClickBody {
    /// 点击用户ID
    pub user_id: String,
    /// 目标ID (频道ID)
    pub target_id: String,
    /// 消息ID (被点击的卡片消息ID)
    pub msg_id: String,
    /// 按钮的 value 值
    pub value: String,
    /// 用户信息 (可选)
    #[serde(default)]
    pub user_info: Option<ButtonClickUserInfo>,
}

/// 按钮点击用户信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ButtonClickUserInfo {
    /// 用户ID
    pub id: String,
    /// 用户名
    pub username: String,
    /// 昵称
    #[serde(default)]
    pub nickname: String,
    /// 识别码
    #[serde(rename = "identifyNum", default)]
    pub identify_num: String,
    /// 是否在线
    #[serde(default)]
    pub online: bool,
    /// 是否机器人
    #[serde(default)]
    pub bot: bool,
    /// 头像
    #[serde(default)]
    pub avatar: String,
}

/// 语音频道事件数据 (joined_channel / exited_channel)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceChannelEventData {
    /// 消息通道类型
    pub channel_type: String,
    /// 消息类型 (255)
    #[serde(rename = "type")]
    pub msg_type: u8,
    /// 目标ID (guild_id)
    pub target_id: String,
    /// 发送者ID ("1")
    pub author_id: String,
    /// 消息时间戳(毫秒)
    pub msg_timestamp: i64,
    /// 事件详情
    pub extra: VoiceChannelEventExtra,
}

/// 语音频道事件额外信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceChannelEventExtra {
    /// 事件类型 "joined_channel" 或 "exited_channel"
    #[serde(rename = "type")]
    pub event_type: String,
    /// 事件详情
    pub body: VoiceChannelEventBody,
}

/// 语音频道事件信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceChannelEventBody {
    /// 用户ID
    pub user_id: String,
    /// 语音频道ID
    pub channel_id: String,
}

/// 表情反应事件数据 (added_reaction / deleted_reaction)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReactionEventData {
    /// 消息通道类型
    pub channel_type: String,
    /// 消息类型 (255)
    #[serde(rename = "type")]
    pub msg_type: u8,
    /// 目标ID (channel_id)
    pub target_id: String,
    /// 发送者ID ("1")
    pub author_id: String,
    /// 消息时间戳(毫秒)
    pub msg_timestamp: i64,
    /// 事件详情
    pub extra: ReactionEventExtra,
}

/// 表情反应事件额外信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReactionEventExtra {
    /// 事件类型
    #[serde(rename = "type")]
    pub event_type: String,
    /// 事件详情
    pub body: ReactionEventBody,
}

/// 表情反应事件信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReactionEventBody {
    /// 用户ID
    pub user_id: String,
    /// 消息ID
    pub msg_id: String,
    /// 表情信息
    pub emoji: EmojiInfo,
}

/// 表情信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmojiInfo {
    /// 表情ID
    pub id: String,
    /// 表情名称
    #[serde(default)]
    pub name: String,
}

impl MessageData {
    /// 判断是否为文字消息 (type=1)
    pub fn is_text(&self) -> bool {
        self.msg_type == MessageType::Text
    }

    /// 判断是否为图片消息 (type=2)
    pub fn is_image(&self) -> bool {
        self.msg_type == MessageType::Image
    }

    /// 判断是否为 KMarkdown 消息 (type=9)
    pub fn is_kmarkdown(&self) -> bool {
        self.msg_type == MessageType::KMarkdown
    }

    /// 判断是否为 Card 消息 (type=10)
    pub fn is_card(&self) -> bool {
        self.msg_type == MessageType::Card
    }

    /// 判断发送者是否为机器人
    pub fn is_from_bot(&self) -> bool {
        self.extra.author.bot
    }

    /// 判断是否为频道消息 (GROUP)
    pub fn is_group_message(&self) -> bool {
        self.channel_type == ChannelType::GROUP
    }

    /// 判断是否为私聊消息 (PERSON)
    pub fn is_private_message(&self) -> bool {
        self.channel_type == ChannelType::PERSON
    }
}

/// 解析事件
///
/// 根据 extra.type 字段将原始 JSON 数据解析为对应的事件类型。
pub fn parse_event(data: serde_json::Value) -> Option<Event> {
    let msg_type = data.get("type")?.as_i64()?;
    
    match msg_type {
        255 => {
            let event_type = data.get("extra")
                .and_then(|e| e.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            
            tracing::info!("[parse_event] 系统事件: extra.type={}", event_type);
            
            match event_type {
                "message_btn_click" => {
                    tracing::info!("[parse_event] 识别到按钮点击事件");
                    tracing::debug!("[parse_event] 按钮事件原始数据: {}", serde_json::to_string(&data).unwrap_or_default());
                    
                    match serde_json::from_value::<ButtonClickData>(data.clone()) {
                        Ok(btn_data) => {
                            tracing::info!("[parse_event] 按钮事件解析成功: value={}", btn_data.extra.body.value);
                            Some(Event::ButtonClick(btn_data))
                        }
                        Err(e) => {
                            tracing::error!("[parse_event] 解析 ButtonClickData 失败: {}", e);
                            tracing::error!("[parse_event] 数据: {}", serde_json::to_string(&data).unwrap_or_default());
                            Some(Event::Unknown(data))
                        }
                    }
                }
                "joined_channel" => {
                    match serde_json::from_value::<VoiceChannelEventData>(data.clone()) {
                        Ok(voice_data) => Some(Event::UserJoinVoice(voice_data)),
                        Err(e) => {
                            tracing::warn!("解析 VoiceChannelEventData 失败: {}", e);
                            Some(Event::Unknown(data))
                        }
                    }
                }
                "exited_channel" => {
                    match serde_json::from_value::<VoiceChannelEventData>(data.clone()) {
                        Ok(voice_data) => Some(Event::UserLeaveVoice(voice_data)),
                        Err(e) => {
                            tracing::warn!("解析 VoiceChannelEventData 失败: {}", e);
                            Some(Event::Unknown(data))
                        }
                    }
                }
                "added_reaction" => {
                    match serde_json::from_value::<ReactionEventData>(data.clone()) {
                        Ok(reaction_data) => Some(Event::UserAddReaction(reaction_data)),
                        Err(e) => {
                            tracing::warn!("解析 ReactionEventData 失败: {}", e);
                            Some(Event::Unknown(data))
                        }
                    }
                }
                "deleted_reaction" => {
                    match serde_json::from_value::<ReactionEventData>(data.clone()) {
                        Ok(reaction_data) => Some(Event::UserRemoveReaction(reaction_data)),
                        Err(e) => {
                            tracing::warn!("解析 ReactionEventData 失败: {}", e);
                            Some(Event::Unknown(data))
                        }
                    }
                }
                _ => {
                    match serde_json::from_value::<SystemMessageData>(data.clone()) {
                        Ok(sys_data) => Some(Event::SystemMessage(sys_data)),
                        Err(e) => {
                            tracing::warn!("解析 SystemMessageData 失败: {}", e);
                            Some(Event::Unknown(data))
                        }
                    }
                }
            }
        }
        1 | 2 | 3 | 4 | 8 | 9 | 10 => {
            match serde_json::from_value::<MessageData>(data.clone()) {
                Ok(msg_data) => Some(Event::Message(msg_data)),
                Err(e) => {
                    tracing::warn!("解析 MessageData 失败: {}", e);
                    Some(Event::Unknown(data))
                }
            }
        }
        _ => {
            tracing::warn!("[parse_event] 未知消息类型: {}", msg_type);
            Some(Event::Unknown(data))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_message() {
        let json = serde_json::json!({
            "channel_type": "GROUP",
            "type": 1,
            "target_id": "chan1",
            "author_id": "user1",
            "content": "/help",
            "msg_id": "msg-001",
            "msg_timestamp": 1700000000000i64,
            "extra": {
                "type": 1,
                "guild_id": "guild1",
                "author": {
                    "id": "user1",
                    "username": "testuser",
                    "nickname": "Test",
                    "identify_num": "0001",
                    "online": true,
                    "bot": false,
                    "status": 1,
                    "avatar": "",
                    "is_vip": false,
                    "is_music_vip": false
                }
            }
        });
        let event = parse_event(json).expect("should parse message event");
        match event {
            Event::Message(msg) => {
                assert_eq!(msg.content, "/help");
                assert_eq!(msg.author_id, "user1");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn test_parse_button_click() {
        let json = serde_json::json!({
            "channel_type": "GROUP",
            "type": 255,
            "target_id": "chan1",
            "author_id": "1",
            "content": "",
            "msg_id": "",
            "msg_timestamp": 1700000000000i64,
            "extra": {
                "type": "message_btn_click",
                "body": {
                    "user_id": "user1",
                    "target_id": "chan1",
                    "msg_id": "card-msg-001",
                    "value": "nextMusic",
                    "user_info": {
                        "id": "user1",
                        "username": "testuser",
                        "nickname": "Test",
                        "identifyNum": "0001",
                        "online": true,
                        "bot": false,
                        "avatar": ""
                    }
                }
            }
        });
        let event = parse_event(json).expect("should parse button click event");
        match event {
            Event::ButtonClick(btn) => {
                assert_eq!(btn.extra.body.value, "nextMusic");
            }
            _ => panic!("expected ButtonClick event"),
        }
    }

    #[test]
    fn test_parse_unknown_type() {
        let json = serde_json::json!({
            "type": 999,
            "extra": {}
        });
        let event = parse_event(json).expect("should return Unknown for unhandled type");
        assert!(matches!(event, Event::Unknown(_)));
    }

    #[test]
    fn test_parse_voice_join() {
        let json = serde_json::json!({
            "channel_type": "GROUP",
            "type": 255,
            "target_id": "guild1",
            "author_id": "1",
            "msg_timestamp": 1700000000000i64,
            "extra": {
                "type": "joined_channel",
                "body": {
                    "user_id": "user1",
                    "channel_id": "vc1"
                }
            }
        });
        let event = parse_event(json).expect("should parse voice join event");
        assert!(matches!(event, Event::UserJoinVoice(_)));
    }
}
