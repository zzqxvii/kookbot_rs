//! Webhook 签名验证
//!
//! 验证 KOOK 发送的请求签名，防止伪造请求

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

/// 签名验证错误
#[derive(Debug)]
pub enum VerifyError {
    /// 缺少必要头部
    MissingHeader(String),
    /// 签名格式错误
    InvalidFormat,
    /// 签名验证失败
    InvalidSignature,
    /// 时间戳过期
    TimestampExpired,
    /// 解码错误
    DecodeError(String),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::MissingHeader(h) => write!(f, "缺少必要头部: {}", h),
            VerifyError::InvalidFormat => write!(f, "签名格式错误"),
            VerifyError::InvalidSignature => write!(f, "签名验证失败"),
            VerifyError::TimestampExpired => write!(f, "时间戳过期"),
            VerifyError::DecodeError(e) => write!(f, "解码错误: {}", e),
        }
    }
}

impl std::error::Error for VerifyError {}

type HmacSha256 = Hmac<Sha256>;

/// 验证 Webhook 请求签名
///
/// # 参数
/// - `token`: Webhook 验证令牌
/// - `body`: 请求体
/// - `timestamp`: 时间戳头部值
/// - `signature`: 签名头部值
///
/// # 返回
/// 成功返回 Ok(())，失败返回 VerifyError
pub fn verify_signature(
    token: &str,
    body: &[u8],
    timestamp: &str,
    signature: &str,
) -> Result<(), VerifyError> {
    // 检查时间戳（防止重放攻击）
    let ts = timestamp
        .parse::<u64>()
        .map_err(|_| VerifyError::InvalidFormat)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 允许 5 分钟的时间差
    if now.saturating_sub(ts) > 300 {
        return Err(VerifyError::TimestampExpired);
    }

    // 构造待签名字符串
    let payload = format!("{timestamp}.{}", base64::encode(body));

    // 计算 HMAC-SHA256
    let mut mac = HmacSha256::new_from_slice(token.as_bytes())
        .map_err(|e| VerifyError::DecodeError(e.to_string()))?;
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let computed_sig = hex::encode(result.into_bytes());

    // 比较签名
    if computed_sig != signature {
        return Err(VerifyError::InvalidSignature);
    }

    Ok(())
}

/// 从请求头中提取必要信息
pub struct WebhookHeaders {
    pub timestamp: String,
    pub signature: String,
}

impl WebhookHeaders {
    /// 从 HTTP 头中提取
    pub fn from_headers(headers: &[(String, String)]) -> Result<Self, VerifyError> {
        let mut timestamp = None;
        let mut signature = None;

        for (key, value) in headers {
            match key.to_lowercase().as_str() {
                "x-kook-timestamp" => timestamp = Some(value.clone()),
                "x-kook-signature" => signature = Some(value.clone()),
                _ => {}
            }
        }

        let timestamp = timestamp
            .ok_or_else(|| VerifyError::MissingHeader("X-Kook-Timestamp".to_string()))?;
        let signature = signature
            .ok_or_else(|| VerifyError::MissingHeader("X-Kook-Signature".to_string()))?;

        Ok(Self { timestamp, signature })
    }
}
