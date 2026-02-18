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
//! ## 事件主要格式 (d 字段内容)
//!
//! | 字段 | 类型 | 说明 |
//! |------|------|------|
//! | channel_type | string | 消息通道类型: GROUP(组播), PERSON(单播), BROADCAST(广播) |
//! | type | int | 消息类型: 1=文字, 2=图片, 3=视频, 4=文件, 8=音频, 9=KMarkdown, 10=Card, 255=系统消息 |
//! | target_id | string | 发送目的: 频道消息时为 channel_id，系统消息时为 guild_id |
//! | author_id | string | 发送者 id，1 代表系统 |
//! | content | string | 消息内容，文件/图片/视频时为 url |
//! | msg_id | string | 消息 id |
//! | msg_timestamp | int | 消息发送时间的毫秒时间戳 |
//! | nonce | string | 随机串 |
//! | extra | object | 额外信息，不同消息类型结构不同 |
//!
//! ## 文字频道消息 extra 说明 (type 非 255)
//!
//! | 字段 | 类型 | 说明 |
//! |------|------|------|
//! | type | int | 同上面 type |
//! | guild_id | string | 服务器 id |
//! | channel_name | string | 频道名 |
//! | mention | Array | 提及到的用户 id 列表 |
//! | mention_all | boolean | 是否 mention 所有用户 |
//! | mention_roles | Array | mention 用户角色的数组 |
//! | mention_here | boolean | 是否 mention 在线用户 |
//! | author | Map | 用户信息 |
//!
//! ## 系统事件消息 extra 说明 (type=255)
//!
//! | 字段 | 类型 | 说明 |
//! |------|------|------|
//! | type | string | 事件类型标识 |
//! | body | Map | 事件关联的具体数据 |

use serde::{Deserialize, Serialize};

/// 消息通道类型
///
/// - GROUP: 组播消息 (频道消息)
/// - PERSON: 单播消息 (私聊)
/// - BROADCAST: 广播消息
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ChannelType {
    GROUP,
    PERSON,
    BROADCAST,
}

/// 消息类型
///
/// | 值 | 类型 | 说明 |
/// |----|------|------|
/// | 1 | Text | 文字消息 |
/// | 2 | Image | 图片消息 |
/// | 3 | Video | 视频消息 |
/// | 4 | File | 文件消息 |
/// | 8 | Audio | 音频消息 |
/// | 9 | KMarkdown | KMarkdown 消息 |
/// | 10 | Card | Card 消息 |
/// | 255 | System | 系统消息 |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        Ok(match num {
            1 => MessageType::Text,
            2 => MessageType::Image,
            3 => MessageType::Video,
            4 => MessageType::File,
            8 => MessageType::Audio,
            9 => MessageType::KMarkdown,
            10 => MessageType::Card,
            255 => MessageType::System,
            _ => MessageType::Unknown,
        })
    }
}

impl Serialize for MessageType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_i32(*self as i32)
    }
}

/// 事件处理器 trait
///
/// 实现此 trait 来处理接收到的 Kook 事件。
///
/// # Example
///
/// ```rust,ignore
/// struct MyHandler;
///
/// #[async_trait]
/// impl EventHandler for MyHandler {
///     async fn on_message(&self, data: MessageData) {
///         println!("收到消息: {}", data.content);
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
            Event::Unknown(data) => self.on_unknown(data).await,
        }
    }

    /// 收到消息事件 (type: 1-10)
    async fn on_message(&self, _data: MessageData) {}

    /// 收到系统事件 (type: 255)
    async fn on_system_message(&self, _data: SystemMessageData) {}

    /// 收到未知类型事件
    async fn on_unknown(&self, _data: serde_json::Value) {}
}

/// 事件枚举
#[derive(Debug, Clone)]
pub enum Event {
    /// 消息事件 (type: 1=文字, 2=图片, 3=视频, 4=文件, 8=音频, 9=KMarkdown, 10=Card)
    Message(MessageData),
    /// 系统事件 (type: 255)
    SystemMessage(SystemMessageData),
    /// 未知事件
    Unknown(serde_json::Value),
}

/// 消息数据 (s=0, type 非255)
///
/// 包含用户发送的聊天消息详细信息。
///
/// # 示例
///
/// ```json
/// {
///   "channel_type": "GROUP",
///   "type": 1,
///   "target_id": "123456789",
///   "author_id": "987654321",
///   "content": "Hello World",
///   "msg_id": "abc-def-ghi",
///   "msg_timestamp": 1234567890123,
///   "nonce": "random-string",
///   "extra": { ... }
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageData {
    /// 消息通道类型: GROUP/PERSON/BROADCAST
    pub channel_type: ChannelType,
    /// 消息类型: 1=文字, 2=图片, 3=视频, 4=文件, 8=音频, 9=KMarkdown, 10=Card
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// 目标ID: 频道消息时为 channel_id，系统消息时为 guild_id
    pub target_id: String,
    /// 发送者ID (1 代表系统)
    pub author_id: String,
    /// 消息内容 (文件/图片/视频时为 url)
    pub content: String,
    /// 消息ID
    pub msg_id: String,
    /// 消息发送时间的毫秒时间戳
    pub msg_timestamp: i64,
    /// 随机串，与用户消息发送 API 中传的 nonce 保持一致
    #[serde(default)]
    pub nonce: String,
    /// 额外信息 (不同消息类型结构不同)
    pub extra: MessageExtra,
    /// 其他未知字段 (用于兼容 API 变更)
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// 消息额外信息 (type 非255)
///
/// 文字频道消息的 extra 字段结构。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageExtra {
    /// 消息类型 (同 MessageData.msg_type)
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
    /// 是否 @在线用户
    #[serde(default)]
    pub mention_here: bool,
    /// 发送者信息
    pub author: Author,
    /// 其他未知字段
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// 作者/用户信息
///
/// 消息发送者的详细信息。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Author {
    /// 用户ID
    pub id: String,
    /// 用户名
    #[serde(default)]
    pub username: String,
    /// 服务器昵称
    #[serde(default)]
    pub nickname: String,
    /// 识别码 (用户名后的 #xxxx)
    #[serde(default)]
    pub identify_num: String,
    /// 头像URL
    #[serde(default)]
    pub avatar: String,
    /// 是否在线
    #[serde(default)]
    pub online: bool,
    /// 是否为机器人
    #[serde(default)]
    pub bot: bool,
    /// 用户状态
    #[serde(default)]
    pub status: i32,
    /// 其他未知字段
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// 系统消息数据 (s=0, type=255)
///
/// 系统事件消息，如用户加入/离开服务器、消息置顶等。
///
/// ## extra.type 常见值
///
/// | type | 说明 |
/// |------|------|
/// | joined_channel | 用户加入语音频道 |
/// | exited_channel | 用户离开语音频道 |
/// | message_pinned | 消息被置顶 |
/// | message_unpinned | 消息取消置顶 |
/// | guild_member_online | 服务器成员上线 |
/// | guild_member_offline | 服务器成员下线 |
/// | added_reaction | 用户添加表情回应 |
/// | deleted_reaction | 用户删除表情回应 |
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMessageData {
    /// 消息通道类型
    pub channel_type: ChannelType,
    /// 目标ID (系统消息时为 guild_id)
    pub target_id: String,
    /// 发送者ID (系统消息为 "1")
    pub author_id: String,
    /// 消息内容
    pub content: String,
    /// 消息ID
    pub msg_id: String,
    /// 消息时间戳(毫秒)
    pub msg_timestamp: i64,
    /// 系统事件额外信息
    pub extra: SystemMessageExtra,
    /// 其他未知字段
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// 系统消息额外信息 (type=255)
///
/// 系统事件的详细信息，不同事件类型的 body 结构不同。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMessageExtra {
    /// 事件类型标识 (如 "joined_channel", "exited_channel" 等)
    #[serde(rename = "type")]
    pub event_type: String,
    /// 服务器ID
    #[serde(default)]
    pub guild_id: String,
    /// 事件关联的具体数据 (结构随 event_type 变化)
    #[serde(default)]
    pub body: serde_json::Value,
    /// 其他未知字段
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
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

    /// 判断是否提及了指定用户
    pub fn mentions_user(&self, user_id: &str) -> bool {
        self.extra.mention.contains(&user_id.to_string())
    }

    /// 判断是否 @所有人
    pub fn mentions_all(&self) -> bool {
        self.extra.mention_all
    }
}

/// 解析事件
///
/// 根据 type 字段将原始 JSON 数据解析为对应的事件类型。
pub fn parse_event(data: serde_json::Value) -> Option<Event> {
    let msg_type = data.get("type")?.as_i64()?;
    
    match msg_type {
        255 => {
            match serde_json::from_value::<SystemMessageData>(data.clone()) {
                Ok(sys_data) => Some(Event::SystemMessage(sys_data)),
                Err(e) => {
                    tracing::warn!("解析 SystemMessageData 失败: {}", e);
                    Some(Event::Unknown(data))
                }
            }
        }
        1 | 2 | 3 | 4 | 8 | 9 | 10 => {
            match serde_json::from_value::<MessageData>(data.clone()) {
                Ok(msg_data) => Some(Event::Message(msg_data)),
                Err(e) => {
                    tracing::warn!("解析 MessageData 失败: {}", e);
                    tracing::debug!("数据: {}", serde_json::to_string(&data).unwrap_or_default());
                    Some(Event::Unknown(data))
                }
            }
        }
        _ => {
            Some(Event::Unknown(data))
        }
    }
}
