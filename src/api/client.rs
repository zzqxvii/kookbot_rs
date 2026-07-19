use crate::core::config::BotConfig;
use crate::core::error::{BotError, Result};
use crate::common::models::{KookResponse, User, VoiceConnectionInfo};
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::{debug, error, info, warn};

const KOOK_API_BASE: &str = "https://www.kookapp.cn/api/v3";

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub master_id: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub notify_type: i32,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub enable_open: bool,
    #[serde(default)]
    pub open_id: String,
    #[serde(default)]
    pub default_channel_id: String,
    #[serde(default)]
    pub welcome_channel_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(rename = "user_id")]
    #[serde(default)]
    pub user_id: String,
    #[serde(rename = "guild_id")]
    #[serde(default)]
    pub guild_id: String,
    #[serde(rename = "parent_id")]
    #[serde(default)]
    pub parent_id: String,
    #[serde(default)]
    pub level: i32,
    #[serde(default)]
    pub limit_amount: i32,
    pub is_category: bool,
    #[serde(rename = "type")]
    pub channel_type: i32,
    #[serde(default)]
    pub topic: String,
}

#[derive(Debug, Clone)]
pub struct KookClient {
    http: Client,
    token: String,
    base_url: String,
}

impl KookClient {
    pub fn new(config: &BotConfig) -> Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(config.network.timeout))
            .build()
            .map_err(|e| BotError::StartupError(format!("创建 HTTP 客户端失败: {}", e)))?;

        Ok(Self {
            http,
            token: config.token.clone(),
            base_url: KOOK_API_BASE.to_string(),
        })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        self.request_inner(method, endpoint, body).await
    }

    async fn request_inner<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        let is_idempotent = matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
        const MAX_ATTEMPTS: u32 = 4;

        for attempt in 0..MAX_ATTEMPTS {
            debug!("发送请求: {} {} (attempt {})", method, url, attempt + 1);

            let mut request = self
                .http
                .request(method.clone(), &url)
                .header("Authorization", format!("Bot {}", self.token))
                .header("Content-Type", "application/json");

            if let Some(body) = &body {
                request = request.json(body);
            }

            let response = match request.send().await {
                Ok(r) => r,
                Err(e) => {
                    if e.is_timeout() && is_idempotent && attempt + 1 < MAX_ATTEMPTS {
                        let delay = 2u64.pow(attempt);
                        warn!("请求超时，{}秒后重试 (attempt {}/{})", delay, attempt + 1, MAX_ATTEMPTS);
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let status = response.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt + 1 < MAX_ATTEMPTS {
                let retry_after = response.headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5);
                warn!("速率限制，{}秒后重试", retry_after);
                tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                continue;
            }

            if status.is_server_error() && is_idempotent && attempt < 2 {
                let delay = 2u64.pow(attempt + 1);
                warn!("服务器错误 {}，{}秒后重试", status, delay);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                continue;
            }

            if !status.is_success() {
                let text = response.text().await?;
                error!("API 请求失败: {} - {}", status, text);
                return Err(BotError::KookApiError {
                    code: status.as_u16() as i32,
                    message: format!("HTTP {}: {}", status, text),
                });
            }

            let raw_text = response.text().await?;
            debug!("API 原始响应: {}", raw_text);

            let api_response: KookResponse<T> = serde_json::from_str(&raw_text)
                .map_err(|e| {
                    error!("解析响应失败: {}, 原始内容: {}", e, raw_text);
                    BotError::KookApiError {
                        code: -1,
                        message: format!("解析响应失败: {}", e),
                    }
                })?;

            if api_response.code != 0 {
                return Err(BotError::KookApiError {
                    code: api_response.code,
                    message: api_response.message,
                });
            }

            return api_response.data.ok_or_else(|| BotError::KookApiError {
                code: -1,
                message: "响应数据为空".to_string(),
            });
        }

        Err(BotError::KookApiError {
            code: -1,
            message: "请求失败: 已达最大重试次数".to_string(),
        })
    }

    pub async fn get_current_user(&self) -> Result<User> {
        info!("获取当前用户信息...");
        self.request(Method::GET, "/user/me", None).await
    }

    pub async fn get_gateway_url(&self) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct GatewayData {
            url: String,
        }

        let data: GatewayData = self.request(Method::GET, "/gateway/index", None).await?;
        Ok(data.url)
    }

    pub async fn get_guild_list(&self) -> Result<Vec<Guild>> {
        info!("获取服务器列表...");
        
        #[derive(serde::Deserialize)]
        struct GuildListData {
            items: Vec<Guild>,
        }

        let data: GuildListData = self.request(
            Method::GET, 
            "/guild/list?page=1&page_size=100", 
            None
        ).await?;
        
        info!("获取到 {} 个服务器", data.items.len());
        Ok(data.items)
    }

    pub async fn get_channel_list(&self, guild_id: &str) -> Result<Vec<Channel>> {
        debug!("获取服务器 {} 的频道列表...", guild_id);
        
        let endpoint = format!("/channel/list?guild_id={}", guild_id);
        
        #[derive(serde::Deserialize)]
        struct ChannelListData {
            items: Vec<Channel>,
        }
        
        let data: ChannelListData = self.request(Method::GET, &endpoint, None).await?;
        
        debug!("服务器 {} 有 {} 个频道", guild_id, data.items.len());
        Ok(data.items)
    }

    pub async fn join_voice_channel(&self, channel_id: &str) -> Result<VoiceConnectionInfo> {
        let body = json!({
            "channel_id": channel_id,
        });

        info!("正在加入语音频道: {}", channel_id);
        let info: VoiceConnectionInfo = self
            .request(Method::POST, "/voice/join", Some(body))
            .await?;

        info!(
            "成功加入语音频道，RTP 服务器: {}:{}",
            info.ip(), info.port()
        );
        Ok(info)
    }

    pub async fn leave_voice_channel(&self, channel_id: &str) -> Result<()> {
        let body = json!({
            "channel_id": channel_id,
        });

        info!("正在离开语音频道: {}", channel_id);
        let _: serde_json::Value = self
            .request(Method::POST, "/voice/leave", Some(body))
            .await?;

        info!("成功离开语音频道: {}", channel_id);
        Ok(())
    }

    /// 获取语音频道中的用户列表
    pub async fn get_voice_channel_users(&self, channel_id: &str) -> Result<Vec<User>> {
        let endpoint = format!("/channel/user-list?channel_id={}", channel_id);
        let users: Vec<User> = self.request(Method::GET, &endpoint, None).await?;
        Ok(users)
    }

    /// 获取用户所在的语音频道
    /// 遍历服务器的语音频道，查找用户所在的频道
    pub async fn get_user_voice_channel(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<Option<Channel>> {
        // 获取服务器所有频道
        let channels = self.get_channel_list(guild_id).await?;
        
        // 过滤出语音频道 (type=2)
        let voice_channels: Vec<&Channel> = channels.iter()
            .filter(|c| c.channel_type == 2 && !c.is_category)
            .collect();

        // 遍历语音频道查找用户
        for channel in voice_channels {
            match self.get_voice_channel_users(&channel.id).await {
                Ok(users) => {
                    if users.iter().any(|u| u.id == user_id) {
                        info!("用户 {} 在语音频道: {} ({})", user_id, channel.name, channel.id);
                        return Ok(Some(channel.clone()));
                    }
                }
                Err(e) => {
                    debug!("获取频道 {} 用户列表失败: {}", channel.id, e);
                }
            }
        }

        Ok(None)
    }

    pub async fn send_channel_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<String> {
        let body = json!({
            "target_id": channel_id,
            "content": content,
        });

        let response: serde_json::Value = self
            .request(Method::POST, "/message/create", Some(body))
            .await?;

        response["msg_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                tracing::warn!("发送消息响应: {:?}", response);
                BotError::KookApiError {
                    code: -1,
                    message: format!("无法获取消息 ID: {:?}", response),
                }
            })
    }
    
    /// 上传图片到 Kook 服务器
    /// 
    /// 返回图片 URL
    pub async fn upload_image(&self, image_data: &[u8]) -> Result<String> {
        let url = format!("{}/asset/create", KOOK_API_BASE);

        const MAX_ATTEMPTS: u32 = 3;

        for attempt in 0..MAX_ATTEMPTS {
            let part = reqwest::multipart::Part::bytes(image_data.to_vec())
                .file_name("qrcode.png")
                .mime_str("image/png")
                .map_err(|e| BotError::IoError(std::io::Error::other(e)))?;

            let form = reqwest::multipart::Form::new()
                .part("file", part);
            let response = match self.http
                .post(&url)
                .header("Authorization", format!("Bot {}", self.token))
                .multipart(form)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if e.is_timeout() && attempt + 1 < MAX_ATTEMPTS {
                        let delay = 2u64.pow(attempt);
                        warn!("上传超时，{}秒后重试 (attempt {}/{})", delay, attempt + 1, MAX_ATTEMPTS);
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let status = response.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt + 1 < MAX_ATTEMPTS {
                let retry_after = response.headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5);
                warn!("上传速率限制，{}秒后重试", retry_after);
                tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                continue;
            }

            if !status.is_success() {
                let text = response.text().await?;
                error!("上传失败: {} - {}", status, text);
                return Err(BotError::KookApiError {
                    code: status.as_u16() as i32,
                    message: format!("上传失败: {}", text),
                });
            }

            let json: serde_json::Value = response.json().await?;
            let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);

            if code != 0 {
                return Err(BotError::KookApiError {
                    code: code as i32,
                    message: json.get("message").and_then(|v| v.as_str()).unwrap_or("上传失败").to_string(),
                });
            }

            let url = json.get("data")
                .and_then(|d| d.get("url"))
                .and_then(|u| u.as_str())
                .ok_or_else(|| BotError::KookApiError {
                    code: -1,
                    message: "无法获取图片 URL".to_string(),
                })?;

            info!("图片上传成功: {}", url);
            return Ok(url.to_string());
        }

        Err(BotError::KookApiError {
            code: -1,
            message: "上传失败: 已达最大重试次数".to_string(),
        })
    }
    
    /// 发送图片消息
    pub async fn send_image_message(&self, channel_id: &str, image_url: &str) -> Result<String> {
        let body = json!({
            "target_id": channel_id,
            "content": image_url,
            "type": 2, // 图片消息
        });

        let response: serde_json::Value = self
            .request(Method::POST, "/message/create", Some(body))
            .await?;

        response["msg_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                BotError::KookApiError {
                    code: -1,
                    message: format!("无法获取消息 ID: {:?}", response),
                }
            })
    }

    /// 发送卡片消息
    pub async fn send_card_message(
        &self,
        channel_id: &str,
        card_json: &serde_json::Value,
    ) -> Result<String> {
        let card_str = card_json.to_string();
        info!("发送卡片消息到频道: {}", channel_id);
        debug!("卡片内容: {}", card_str);
        
        let body = json!({
            "target_id": channel_id,
            "type": 10, // Card 消息类型
            "content": card_str,
        });

        let response: serde_json::Value = self
            .request(Method::POST, "/message/create", Some(body))
            .await?;

        let msg_id = response["msg_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                BotError::KookApiError {
                    code: -1,
                    message: format!("无法获取消息 ID: {:?}", response),
                }
            })?;
        
        info!("卡片消息发送成功, msg_id: {}", msg_id);
        Ok(msg_id)
    }

    /// 删除消息
    pub async fn delete_message(&self, msg_id: &str) -> Result<()> {
        let body = json!({
            "msg_id": msg_id,
        });
        
        let _: serde_json::Value = self
            .request(Method::POST, "/message/delete", Some(body))
            .await?;
        
        info!("已删除消息: {}", msg_id);
        Ok(())
    }
}
