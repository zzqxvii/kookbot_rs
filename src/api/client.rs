use crate::config::BotConfig;
use crate::error::{BotError, Result};
use crate::models::{JoinedChannel, KookResponse, User, VoiceConnectionInfo};
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::{debug, error, info};

const KOOK_API_BASE: &str = "https://www.kookapp.cn/api/v3";

#[derive(Debug, serde::Deserialize)]
struct GatewayResponse {
    url: String,
}

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
            .map_err(|e| BotError::ConfigError(format!("创建 HTTP 客户端失败: {}", e)))?;

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
        let url = format!("{}{}", self.base_url, endpoint);
        debug!("发送请求: {} {}", method, url);

        let mut request = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Bot {}", self.token))
            .header("Content-Type", "application/json");

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request.send().await?;
        let status = response.status();

        if !status.is_success() {
            let text = response.text().await?;
            error!("API 请求失败: {} - {}", status, text);
            return Err(BotError::KookApiError {
                code: status.as_u16() as i32,
                message: format!("HTTP {}: {}", status, text),
            });
        }

        let api_response: KookResponse<T> = response.json().await?;

        if api_response.code != 0 {
            return Err(BotError::KookApiError {
                code: api_response.code,
                message: api_response.message,
            });
        }

        api_response.data.ok_or_else(|| BotError::KookApiError {
            code: -1,
            message: "响应数据为空".to_string(),
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
            #[serde(default)]
            meta: serde_json::Value,
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
        let channels: Vec<Channel> = self.request(Method::GET, &endpoint, None).await?;
        
        debug!("服务器 {} 有 {} 个频道", guild_id, channels.len());
        Ok(channels)
    }

    pub async fn join_voice_channel(&self, channel_id: &str) -> Result<VoiceConnectionInfo> {
        let body = json!({
            "channel_id": channel_id,
        });

        info!("正在加入语音频道: {}", channel_id);
        let info: VoiceConnectionInfo = self
            .request(Method::POST, "/channel/voice/join", Some(body))
            .await?;

        info!(
            "成功加入语音频道，RTP 服务器: {}:{}",
            info.ip, info.port
        );
        Ok(info)
    }

    pub async fn leave_voice_channel(&self, channel_id: &str) -> Result<()> {
        let body = json!({
            "channel_id": channel_id,
        });

        info!("正在离开语音频道: {}", channel_id);
        let _: serde_json::Value = self
            .request(Method::POST, "/channel/voice/leave", Some(body))
            .await?;

        info!("成功离开语音频道: {}", channel_id);
        Ok(())
    }

    pub async fn get_user_voice_channel(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<Option<JoinedChannel>> {
        let endpoint = format!("/guild/{}/user-channel/{}?user_id={}", guild_id, user_id, user_id);

        match self.request::<Vec<JoinedChannel>>(Method::GET, &endpoint, None).await {
            Ok(channels) => Ok(channels.into_iter().next()),
            Err(BotError::KookApiError { code: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
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

        // Kook API 返回 msg_id 字段
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
}
