use thiserror::Error;

/// Bot 全局错误类型
///
/// String 元组变体（如 `AudioDecodeError(String)`）保留向后兼容，
/// 同时通过 `with_source()` 方法支持链入底层错误源。
#[derive(Error, Debug)]
pub enum BotError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("JSON parsing failed: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Audio decoding error: {0}")]
    AudioDecodeError(String),

    #[error("Opus encoding error: {0}")]
    OpusError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Kook API error: {code} - {message}")]
    KookApiError { code: i32, message: String },

    #[error("Gateway error: {0}")]
    GatewayError(String),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Voice error: {0}")]
    VoiceError(String),

    #[error("Channel not found")]
    ChannelNotFound,

    #[error("Not in voice channel")]
    NotInVoiceChannel,

    #[error("Stream already started")]
    StreamAlreadyStarted,

    #[error("Stream not started")]
    StreamNotStarted,
}

impl BotError {
    /// 获取底层错误链（含源错误信息）。
    ///
    /// 通过 `#[from]` 自动派生的变体（HttpError, JsonError, IoError）
    /// 已自动包含源错误。其余变体可通过 `with_source` 构造器附加源。
    pub fn root_cause(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(self)
    }
}

pub type Result<T> = std::result::Result<T, BotError>;
