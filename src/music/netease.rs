use crate::error::{BotError, Result};
use crate::player::Music;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// 网易云音乐 API 客户端
pub struct NeteaseClient {
    http: Client,
    cookie: Option<String>,
    base_url: String,
}

/// 网易云歌曲信息
#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseSong {
    pub id: u64,
    pub name: String,
    #[serde(alias = "ar")]
    pub artists: Vec<NeteaseArtist>,
    #[serde(alias = "al")]
    pub album: NeteaseAlbum,
    #[serde(alias = "dt")]
    pub duration: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseArtist {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseAlbum {
    pub id: u64,
    pub name: String,
    #[serde(alias = "picUrl")]
    pub pic_url: Option<String>,
}

/// 网易云歌单信息
#[derive(Debug, Clone, Deserialize)]
pub struct NeteasePlaylist {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    #[serde(alias = "coverImgUrl")]
    pub cover_url: Option<String>,
    #[serde(alias = "trackCount")]
    pub track_count: u32,
    #[serde(alias = "playCount")]
    pub play_count: u64,
    pub tracks: Option<Vec<NeteaseSong>>,
}

/// 搜索响应
#[derive(Debug, Clone, Deserialize)]
struct SearchResponse {
    code: i32,
    result: Option<SearchResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchResult {
    songs: Option<Vec<NeteaseSong>>,
    #[serde(alias = "songCount")]
    song_count: Option<u32>,
    playlists: Option<Vec<NeteasePlaylist>>,
    #[serde(alias = "playlistCount")]
    playlist_count: Option<u32>,
}

/// 歌曲 URL 响应
#[derive(Debug, Clone, Deserialize)]
struct SongUrlResponse {
    code: i32,
    data: Option<Vec<SongUrl>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SongUrl {
    id: u64,
    url: Option<String>,
    #[serde(alias = "br")]
    bitrate: Option<u32>,
    #[serde(alias = "size")]
    size: Option<u64>,
}

/// 歌单详情响应
#[derive(Debug, Clone, Deserialize)]
struct PlaylistDetailResponse {
    code: i32,
    playlist: Option<NeteasePlaylist>,
}

impl NeteaseClient {
    /// 创建新的网易云 API 客户端
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("创建 HTTP 客户端失败");

        Self {
            http,
            cookie: None,
            base_url: "https://neteasecloudmusicapi.vercel.app".to_string(),
        }
    }

    /// 使用 Cookie 创建（用于访问 VIP 歌曲）
    pub fn with_cookie(cookie: impl Into<String>) -> Self {
        let mut client = Self::new();
        client.cookie = Some(cookie.into());
        client
    }

    /// 设置 API 基础 URL（用于自建 API）
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// 搜索歌曲
    pub async fn search_songs(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<NeteaseSong>> {
        let url = format!("{}/search", self.base_url);

        let params = vec![
            ("keywords".to_string(), keyword.to_string()),
            ("type".to_string(), "1".to_string()), // 1 = 单曲
            ("limit".to_string(), limit.to_string()),
        ];

        let response = self.send_request::<SearchResponse>(Method::GET, &url, Some(params)).await?;

        if response.code != 200 {
            return Err(BotError::KookApiError {
                code: response.code,
                message: "搜索歌曲失败".to_string(),
            });
        }

        let songs = response.result
            .and_then(|r| r.songs)
            .unwrap_or_default();

        info!("搜索 \"{}\" 找到 {} 首歌曲", keyword, songs.len());
        Ok(songs)
    }

    /// 获取歌曲 URL
    pub async fn get_song_url(
        &self,
        song_id: u64,
        bitrate: u32,
    ) -> Result<Option<String>> {
        let url = format!("{}/song/url", self.base_url);

        let params = vec![
            ("id".to_string(), song_id.to_string()),
            ("br".to_string(), bitrate.to_string()),
        ];

        let response = self.send_request::<SongUrlResponse>(Method::GET, &url, Some(params)).await?;

        if response.code != 200 {
            return Err(BotError::KookApiError {
                code: response.code,
                message: "获取歌曲 URL 失败".to_string(),
            });
        }

        let song_url = response.data
            .and_then(|data| data.into_iter().next())
            .and_then(|song| song.url);

        Ok(song_url)
    }

    /// 获取歌单详情
    pub async fn get_playlist(&self, playlist_id: u64) -> Result<NeteasePlaylist> {
        let url = format!("{}/playlist/detail", self.base_url);

        let params = vec![
            ("id".to_string(), playlist_id.to_string()),
        ];

        let response = self.send_request::<PlaylistDetailResponse>(Method::GET, &url, Some(params)).await?;

        if response.code != 200 {
            return Err(BotError::KookApiError {
                code: response.code,
                message: "获取歌单详情失败".to_string(),
            });
        }

        response.playlist.ok_or_else(|| BotError::ConfigError("歌单不存在".to_string()))
    }

    /// 将 NeteaseSong 转换为 Music
    pub fn to_music(&self, song: &NeteaseSong) -> Music {
        let artist_name = song.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        Music {
            title: song.name.clone(),
            author: artist_name,
            pic_url: song.album.pic_url.clone()
                .unwrap_or_else(|| "https://p1.music.126.net/6y-UleORITEDbvrOLV0Q8A==/5639395138885805.jpg".to_string()),
            platform: "网易云音乐".to_string(),
            source_url: Some(format!("https://music.163.com/#/song?id={}", song.id)),
            duration: Some(song.duration / 1000), // 毫秒转秒
        }
    }

    /// 发送 HTTP 请求
    async fn send_request<T: for<'de> serde::Deserialize<'de>>(
        &self,
        method: Method,
        url: &str,
        params: Option<Vec<(String, String)>>,
    ) -> Result<T> {
        let mut request = self.http.request(method, url);

        // 添加 Cookie
        if let Some(cookie) = &self.cookie {
            request = request.header("Cookie", cookie);
        }

        // 添加参数
        if let Some(params) = params {
            request = request.query(&params);
        }

        let response = request.send().await
            .map_err(|e| BotError::HttpError(e))?;

        let status = response.status();
        if !status.is_success() {
            return Err(BotError::KookApiError {
                code: status.as_u16() as i32,
                message: format!("HTTP error: {}", status),
            });
        }

        let data: T = response.json().await
            .map_err(|e| BotError::HttpError(e))?;

        Ok(data)
    }
}