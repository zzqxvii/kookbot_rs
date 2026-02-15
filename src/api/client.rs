use crate::config::BotConfig;
use crate::error::{BotError, Result};
use crate::models::{JoinedChannel, KookResponse, User, VoiceConnectionInfo};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Kook API 基础 URL
const KOOK_API_BASE: &str = "https://www.kookapp.cn/api/v3";

/// Kook API 客户端
#[derive(Debug, Clone)]
pub struct KookClient {
    http: Client,
    token: String,
    base_url: String,
}

impl KookClient {
    /// 创建新的 Kook 客户端
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

    /// 发送 API 请求
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
            return Err(BotError::HttpError(
                reqwest::Error::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("HTTP {}: {}", status, text),
                ))
            ));
        }

        let api_response: KookResponse<T> = response.json().await?;

        if api_response.code != 0 {
            return Err(BotError::KookApiError {
                code: api_response.code,
                message: api_response.message,
            });
        }

        api_response.data.ok_or_else(|| {
            BotError::KookApiError {
                code: -1,
                message: "响应数据为空".to_string(),
            }
        })
    }

    /// 获取当前登录用户信息
    pub async fn get_current_user(&self) -> Result<User> {
        self.request(Method::GET, "/user/me", None).await
    }

    /// 加入语音频道
    pub async fn join_voice_channel(
        &self,
        channel_id: &str,
    ) -> Result<VoiceConnectionInfo> {
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

    /// 离开语音频道
    pub async fn leave_voice_channel(
        &self,
        channel_id: &str,
    ) -> Result<()> {
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

    /// 获取用户加入的语音频道
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

    /// 发送频道消息
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

        response["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BotError::KookApiError {
                code: -1,
                message: "无法获取消息 ID".to_string(),
            })
    }
}