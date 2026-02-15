use thiserror::Error;

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

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Channel not found")]
    ChannelNotFound,

    #[error("Not in voice channel")]
    NotInVoiceChannel,

    #[error("Stream already started")]
    StreamAlreadyStarted,

    #[error("Stream not started")]
    StreamNotStarted,
}

pub type Result<T> = std::result::Result<T, BotError>;