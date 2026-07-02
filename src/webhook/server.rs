//! Webhook HTTP 服务器
//!
//! 接收 KOOK 发送的 Webhook 事件

use crate::core::config::WebhookConfig;
use crate::core::error::{BotError, Result};
use crate::webhook::handler::{WebhookHandler, WebhookRequest};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

/// Webhook 服务器
pub struct WebhookServer {
    config: WebhookConfig,
    handler: Arc<dyn WebhookHandler>,
}

impl WebhookServer {
    /// 创建新的 Webhook 服务器
    pub fn new(config: WebhookConfig, handler: Arc<dyn WebhookHandler>) -> Self {
        Self { config, handler }
    }

    /// 启动服务器
    pub async fn run(self) -> Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        info!("启动 Webhook 服务器: http://{}", addr);

        let app = Router::new()
            .route(&self.config.path, post(handle_webhook))
            .route("/health", get(health_check))
            .with_state(Arc::new(self));

        let listener = TcpListener::bind(&addr).await
            .map_err(|e| BotError::NetworkError(format!("无法绑定地址 {}: {}", addr, e)))?;

        info!("Webhook 服务器已启动，监听: {}", addr);
        info!("健康检查: http://{}/health", addr);

        axum::serve(listener, app).await
            .map_err(|e| BotError::NetworkError(format!("服务器错误: {}", e)))?;

        Ok(())
    }
}

/// 处理 Webhook 请求
async fn handle_webhook(
    State(server): State<Arc<WebhookServer>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    debug!("收到 Webhook 请求");

    // 验证签名
    if let Err(e) = verify_request(&server.config.verify_token, &headers, &body).await {
        warn!("Webhook 验证失败: {}", e);
        return (StatusCode::UNAUTHORIZED, "验证失败");
    }

    // 解析请求体
    let request: WebhookRequest = match serde_json::from_str(&body) {
        Ok(req) => req,
        Err(e) => {
            error!("解析 Webhook 请求失败: {}", e);
            return (StatusCode::BAD_REQUEST, "请求格式错误");
        }
    };

    // 处理事件
    server.handler.handle_event(request.event_type, request.data).await;

    // 返回 200 OK
    (StatusCode::OK, "OK")
}

/// 健康检查端点
async fn health_check() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "service": "kook-music-bot"
    }))
}

/// 验证请求签名（委托给 `verifier` 模块，消除重复逻辑）
async fn verify_request(
    token: &str,
    headers: &HeaderMap,
    body: &str,
) -> Result<()> {
    let timestamp = headers
        .get("X-Kook-Timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| BotError::ConfigError("缺少 X-Kook-Timestamp 头部".to_string()))?;

    let signature = headers
        .get("X-Kook-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| BotError::ConfigError("缺少 X-Kook-Signature 头部".to_string()))?;

    crate::webhook::verifier::verify_signature(token, body.as_bytes(), timestamp, signature)
        .map_err(|e| BotError::ConfigError(e.to_string()))?;

    Ok(())
}
