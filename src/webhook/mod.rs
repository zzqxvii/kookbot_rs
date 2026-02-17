//! Webhook 事件接收模块
//!
//! 通过 HTTP Webhook 接收 KOOK 事件，替代 WebSocket 连接

pub mod server;
pub mod handler;
pub mod verifier;

pub use server::WebhookServer;
pub use handler::WebhookHandler;
pub use verifier::verify_signature;

/// Webhook 配置
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// 监听地址
    pub host: String,
    /// 监听端口
    pub port: u16,
    /// 回调路径
    pub path: String,
    /// 验证令牌 (用于验证请求签名)
    pub verify_token: String,
    /// 是否启用 SSL
    pub use_ssl: bool,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            path: "/webhook".to_string(),
            verify_token: String::new(),
            use_ssl: false,
        }
    }
}
