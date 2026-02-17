//! Gateway 事件定义

use serde::{Deserialize, Serialize};

/// 消息通道类型
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChannelType {
    Group,
    Person,
    Broadcast,
}

/// 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[repr(i32)]
pub enum MessageType {
    Text = 1,
    Image = 2,
    Video = 3,
    File = 4,
    Audio = 8,
    KMarkdown = 9,
    Card = 10,
    System = 255,
}

/// 事件处理器 trait
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn on_event(&self, event: Event) {
        match event {
            Event::Message(data) => self.on_message(data).await,
            Event::SystemMessage(data) => self.on_system_message(data).await,
            Event::Unknown(data) => self.on_unknown(data).await,
        }
    }

    async fn on_message(&self, _data: MessageData) {}
    async fn on_system_message(&self, _data: SystemMessageData) {}
    async fn on_unknown(&self, _data: serde_json::Value) {}
}

/// 事件枚举
#[derive(Debug, Clone)]
pub enum Event {
    Message(MessageData),
    SystemMessage(SystemMessageData),
    Unknown(serde_json::Value),
}

/// 消息数据 (s=0, type 非255)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageData {
    /// 消息通道类型
    pub channel_type: ChannelType,
    /// 消息类型
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// 目标ID (频道ID或服务器ID)
    pub target_id: String,
    /// 发送者ID
    pub author_id: String,
    /// 消息内容
    pub content: String,
    /// 消息ID
    pub msg_id: String,
    /// 消息时间戳(毫秒)
    pub msg_timestamp: i64,
    /// 随机串
    #[serde(default)]
    pub nonce: String,
    /// 额外信息
    pub extra: MessageExtra,
}

/// 消息额外信息 (type 非255)
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
    /// 提及用户列表
    #[serde(default)]
    pub mention: Vec<String>,
    /// 是否@所有人
    #[serde(default)]
    pub mention_all: bool,
    /// 提及角色列表
    #[serde(default)]
    pub mention_roles: Vec<String>,
    /// 是否@在线用户
    #[serde(default)]
    pub mention_here: bool,
    /// 作者信息
    pub author: Author,
}

/// 作者信息
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
    /// 头像
    #[serde(default)]
    pub avatar: String,
    /// 是否在线
    #[serde(default)]
    pub online: bool,
    /// 是否机器人
    #[serde(default)]
    pub bot: bool,
    /// 状态
    #[serde(default)]
    pub status: i32,
    /// 手机已验证
    #[serde(default)]
    pub mobile_verified: bool,
    /// 系统标识
    #[serde(default)]
    pub sys: bool,
}

/// 系统消息数据 (s=0, type=255)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMessageData {
    /// 消息通道类型
    pub channel_type: ChannelType,
    /// 目标ID
    pub target_id: String,
    /// 发送者ID (系统消息为1)
    pub author_id: String,
    /// 消息内容
    pub content: String,
    /// 消息ID
    pub msg_id: String,
    /// 消息时间戳
    pub msg_timestamp: i64,
    /// 额外信息
    pub extra: SystemMessageExtra,
}

/// 系统消息额外信息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMessageExtra {
    /// 事件类型
    #[serde(rename = "type")]
    pub event_type: String,
    /// 服务器ID
    #[serde(default)]
    pub guild_id: String,
    /// 事件数据
    #[serde(default)]
    pub body: serde_json::Value,
}

impl MessageData {
    pub fn is_text(&self) -> bool {
        self.msg_type == MessageType::Text
    }

    pub fn is_image(&self) -> bool {
        self.msg_type == MessageType::Image
    }

    pub fn is_kmarkdown(&self) -> bool {
        self.msg_type == MessageType::KMarkdown
    }

    pub fn is_card(&self) -> bool {
        self.msg_type == MessageType::Card
    }

    pub fn is_from_bot(&self) -> bool {
        self.extra.author.bot
    }

    pub fn is_group_message(&self) -> bool {
        self.channel_type == ChannelType::Group
    }

    pub fn is_private_message(&self) -> bool {
        self.channel_type == ChannelType::Person
    }

    pub fn mentions_user(&self, user_id: &str) -> bool {
        self.extra.mention.contains(&user_id.to_string())
    }

    pub fn mentions_all(&self) -> bool {
        self.extra.mention_all
    }
}

/// 解析事件
pub fn parse_event(data: serde_json::Value) -> Option<Event> {
    let msg_type = data.get("type")?.as_i64()?;
    
    match msg_type {
        255 => {
            let sys_data: SystemMessageData = serde_json::from_value(data).ok()?;
            Some(Event::SystemMessage(sys_data))
        }
        1 | 2 | 3 | 4 | 8 | 9 | 10 => {
            let msg_data: MessageData = serde_json::from_value(data).ok()?;
            Some(Event::Message(msg_data))
        }
        _ => Some(Event::Unknown(data)),
    }
}
