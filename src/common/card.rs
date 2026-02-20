use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DEFAULT_COVER: &str = "https://img.kookapp.cn/assets/2023-07/bek0jyKtlt02i02s.gif";

const PLATFORM_ICONS: &[(&str, &str)] = &[
    (
        "bilibili",
        "https://img.kookapp.cn/assets/2023-05/r4WyrVwPho00w00w.png",
    ),
    (
        "netease",
        "https://img.kookapp.cn/assets/2023-05/hULgrDPVq200w00w.png",
    ),
    (
        "qqmusic",
        "https://img.kookapp.cn/assets/2023-05/KapvPXiQe800w00w.png",
    ),
];

pub fn get_platform_icon(platform: &str) -> &'static str {
    for (name, url) in PLATFORM_ICONS {
        if platform.to_lowercase().contains(name) {
            return url;
        }
    }
    DEFAULT_COVER
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sender {
    pub nick_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlayMusic {
    pub title: String,
    pub author: String,
    pub platform: String,
    pub pic_url: String,
    pub sender: Sender,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueMusic {
    pub title: String,
    pub author: String,
    pub platform: String,
    pub pic_url: String,
    pub sender: Sender,
}

#[derive(Debug, Clone)]
pub struct PlayCardData {
    pub current: PlayMusic,
    pub queue: Vec<QueueMusic>,
    pub queue_total: usize,
}

impl PlayCardData {
    pub fn new(current: PlayMusic) -> Self {
        Self {
            current,
            queue: Vec::new(),
            queue_total: 1,
        }
    }

    pub fn with_queue(mut self, queue: Vec<QueueMusic>, total: usize) -> Self {
        self.queue = queue;
        self.queue_total = total;
        self
    }
}

pub fn build_play_card(data: &PlayCardData) -> Value {
    let pic_url = if data.current.pic_url.is_empty() {
        DEFAULT_COVER.to_string()
    } else {
        data.current.pic_url.clone()
    };

    let platform_lower = data.current.platform.to_lowercase();
    let platform_display = match platform_lower.as_str() {
        "bilibili" => "B站",
        "netease" => "网易云",
        "qqmusic" => "QQ音乐",
        other => other,
    };

    let mut modules = vec![
        // 当前播放标题
        json!({
            "type": "section",
            "text": {
                "type": "kmarkdown",
                "content": "**当前正在播放：**\n---"
            }
        }),
        // 当前播放 - 图片在右侧，信息竖着显示
        json!({
            "type": "section",
            "text": {
                "type": "kmarkdown",
                "content": format!(
                    "  **歌名:  {}**\n  **歌手:  {}**\n  **音源:  {}**\n  **用户:  (font){}(font)[pink]**",
                    truncate_text(&data.current.title, 30),
                    truncate_text(&data.current.author, 20),
                    platform_display,
                    data.current.sender.nick_name
                )
            },
            "mode": "right",
            "accessory": {
                "type": "image",
                "src": pic_url,
                "size": "sm"
            }
        }),
        // 分割线
        json!({
            "type": "section",
            "text": {
                "type": "kmarkdown",
                "content": "---"
            }
        }),
        // 按钮（使用空文本占位实现靠右）
        json!({
            "type": "action-group",
            "elements": [
                {
                    "type": "button",
                    "theme": "primary",
                    "value": "nextMusic",
                    "click": "return-val",
                    "text": {
                        "type": "plain-text",
                        "content": "下一首"
                    }
                },
                {
                    "type": "button",
                    "theme": "danger",
                    "value": "stop",
                    "click": "return-val",
                    "text": {
                        "type": "plain-text",
                        "content": "停止"
                    }
                }
            ]
        }),
    ];

    // 播放队列 - 带封面
    if !data.queue.is_empty() {
        // 先加分割线
        modules.push(json!({
            "type": "divider"
        }));

        for (idx, music) in data.queue.iter().enumerate().take(2) {
            let pic_url = if music.pic_url.is_empty() {
                DEFAULT_COVER.to_string()
            } else {
                music.pic_url.clone()
            };

            modules.push(json!({
                "type": "section",
                "text": {
                    "type": "kmarkdown",
                    "content": format!(
                        "{} - {}",
                        truncate_text(&music.title, 25),
                        truncate_text(&music.author, 15)
                    )
                },
                "mode": "left",
                "accessory": {
                    "type": "image",
                    "src": pic_url,
                    "size": "sm"
                }
            }));

            // 每首歌后加分割线
            modules.push(json!({
                "type": "divider"
            }));
        }

        if data.queue_total > 2 {
            modules.push(json!({
                "type": "section",
                "text": {
                    "type": "kmarkdown",
                    "content": format!("还有 {} 首...", data.queue_total - 2)
                }
            }));
        }
    }

    json!([{
        "type": "card",
        "theme": "info",
        "size": "lg",
        "modules": modules
    }])
}

pub fn build_bot_status_card(
    title: &str,
    author: &str,
    user_name: &str,
    user_avatar: &str,
    queue_size: usize,
    join_time: &str,
    play_time: &str,
) -> Value {
    json!([{
        "type": "card",
        "theme": "info",
        "size": "lg",
        "modules": [
            {
                "type": "section",
                "text": {
                    "type": "kmarkdown",
                    "content": format!("**正在播放 : **{} - {}", title, author)
                },
                "mode": "left",
                "accessory": {
                    "type": "image",
                    "src": user_avatar
                }
            },
            {
                "type": "section",
                "text": {
                    "type": "kmarkdown",
                    "content": "---"
                }
            },
            {
                "type": "context",
                "elements": [
                    {
                        "type": "plain-text",
                        "content": format!("当前音乐点歌用户: {}", user_name)
                    }
                ]
            },
            {
                "type": "context",
                "elements": [
                    {
                        "type": "plain-text",
                        "content": format!("队列共有{}首歌曲", queue_size)
                    }
                ]
            },
            {
                "type": "context",
                "elements": [
                    {
                        "type": "plain-text",
                        "content": format!("Bot 进入语音时间: {}", join_time)
                    }
                ]
            },
            {
                "type": "context",
                "elements": [
                    {
                        "type": "plain-text",
                        "content": format!("Bot 播放歌曲时间: {}", play_time)
                    }
                ]
            }
        ]
    }])
}

pub fn build_search_card(
    keyword: &str,
    results: &[(String, String, String)],
    platform: &str,
    user_name: &str,
    user_avatar: &str,
) -> Value {
    let platform_icon = get_platform_icon(platform);
    let platform_lower = platform.to_lowercase();
    let platform_display = match platform_lower.as_str() {
        "bilibili" => "B站",
        "netease" => "网易云",
        "qqmusic" => "QQ音乐",
        other => other,
    };

    let mut modules = vec![json!({
        "type": "section",
        "text": {
            "type": "kmarkdown",
            "content": format!("搜索词： [**{}**]", keyword)
        }
    })];

    for (idx, (title, author, _pic)) in results.iter().enumerate().take(5) {
        modules.push(json!({
            "type": "section",
            "text": {
                "type": "kmarkdown",
                "content": format!("  ** {}. {} - {}**", idx + 1, title, author)
            },
            "mode": "left"
        }));
    }

    modules.push(json!({
        "type": "divider"
    }));

    modules.push(json!({
        "type": "context",
        "elements": [
            {
                "type": "plain-text",
                "content": format!("搜索音源: {}", platform_display)
            },
            {
                "type": "image",
                "src": platform_icon
            },
            {
                "type": "plain-text",
                "content": format!("  |  搜索用户: {}", user_name)
            },
            {
                "type": "image",
                "src": user_avatar
            },
            {
                "type": "plain-text",
                "content": "  |  根据下列回应选择歌曲"
            }
        ]
    }));

    json!([{
        "type": "card",
        "theme": "info",
        "size": "lg",
        "modules": modules
    }])
}

fn truncate_text(text: &str, max_len: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        text.to_string()
    } else {
        chars[..max_len].iter().collect::<String>() + "..."
    }
}
