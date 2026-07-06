use crate::core::error::{BotError, Result};
use crate::player::{Music, Sender};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};
use regex::Regex;
use std::sync::LazyLock;

/// QQ 音乐 API 客户端
pub struct QQMusicClient {
    http: Client,
    base_url: String,
    cookie: Option<String>,
    cache_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QQMusicSong {
    pub id: u64,
    pub name: String,
    #[serde(alias = "ar", alias = "singer")]
    pub artists: Vec<QQMusicArtist>,
    #[serde(alias = "al")]
    pub album: QQMusicAlbum,
    #[serde(alias = "dt", alias = "interval", default)]
    pub duration: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QQMusicArtist {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QQMusicAlbum {
    pub id: u64,
    pub name: String,
    #[serde(alias = "picUrl", alias = "pic", default)]
    pub pic_url: String,
}

#[derive(Debug, Clone)]
pub struct QQPlaylistDetail {
    pub id: u64,
    pub name: String,
    pub track_ids: Vec<u64>,
}

static SONG_ID_PATTERNS: LazyLock<[Regex; 4]> = LazyLock::new(|| [
    Regex::new(r"y\.qq\.com/n/ryqq/songDetail/([A-Za-z0-9]+)").unwrap(),
    Regex::new(r"songid=(\d+)").unwrap(),
    Regex::new(r"y\.qq\.com[^\s]*[?&]id=(\d+)").unwrap(),
    Regex::new(r"qq\.com[^\s]*[?&]id=(\d+)").unwrap(),
]);

static PLAYLIST_ID_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| [
    Regex::new(r"y\.qq\.com/n/ryqq/playlist/(\d+)").unwrap(),
    Regex::new(r"playlist\?id=(\d+)").unwrap(),
]);

impl QQMusicClient {
    pub fn new(base_url: &str) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("创建 HTTP 客户端失败");

        Self {
            http,
            base_url: base_url.to_string(),
            cookie: None,
            cache_dir: String::from("./cache"),
        }
    }

    pub fn with_cookie(base_url: &str, cookie: Option<String>) -> Self {
        let mut client = Self::new(base_url);
        client.cookie = cookie;
        client
    }

    pub fn set_cookie(&mut self, cookie: String) {
        self.cookie = Some(cookie);
    }

    pub fn has_cookie(&self) -> bool {
        self.cookie.as_deref().map_or(false, |c| !c.is_empty())
    }

    /// 设置缓存目录
    pub fn set_cache_dir(&mut self, dir: String) {
        self.cache_dir = dir;
    }

    /// 添加 cookie 到请求头
    fn add_cookie(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref cookie) = self.cookie {
            request.header("Cookie", cookie)
        } else {
            request
        }
    }

    /// 检查 API 是否可用
    pub async fn check_api(&self) -> Result<()> {
        let url = format!("{}/search", self.base_url);

        let response = self.http
            .get(&url)
            .query(&[("keyword", "test")])
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(BotError::ConfigError(format!(
                "QQ 音乐 API 返回错误状态: {}",
                response.status()
            )))
        }
    }

    /// 解析 QQ 音乐分享链接，提取歌曲 ID
    pub fn parse_song_id(input: &str) -> Option<u64> {
        // 尝试直接解析为数字 ID
        if let Ok(id) = input.parse::<u64>() {
            return Some(id);
        }

        for re in SONG_ID_PATTERNS.iter() {
            if let Some(caps) = re.captures(input) {
                if let Some(m) = caps.get(1) {
                    if let Ok(id) = m.as_str().parse::<u64>() {
                        return Some(id);
                    }
                }
            }
        }

        None
    }

    /// 解析歌单 ID
    pub fn parse_playlist_id(input: &str) -> Option<u64> {
        if let Ok(id) = input.parse::<u64>() {
            return Some(id);
        }

        for re in PLAYLIST_ID_PATTERNS.iter() {
            if let Some(caps) = re.captures(input) {
                if let Some(m) = caps.get(1) {
                    if let Ok(id) = m.as_str().parse::<u64>() {
                        return Some(id);
                    }
                }
            }
        }

        None
    }

    /// 搜索歌曲
    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<QQMusicSong>> {
        let url = format!("{}/search", self.base_url);

        let request = self.http
            .get(&url)
            .query(&[
                ("keyword", keyword),
                ("limit", &limit.to_string()),
            ]);
        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;

        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "QQ音乐搜索失败".to_string(),
            });
        }

        // 尝试多种响应格式
        let songs: Vec<QQMusicSong> = json
            .get("result")
            .and_then(|r| r.get("songs"))
            .and_then(|s| serde_json::from_value(s.clone()).ok())
            .or_else(|| {
                json.get("data")
                    .and_then(|d| d.get("list"))
                    .and_then(|l| serde_json::from_value(l.clone()).ok())
            })
            .or_else(|| {
                json.get("result")
                    .and_then(|r| r.get("song"))
                    .and_then(|s| s.get("list"))
                    .and_then(|l| serde_json::from_value(l.clone()).ok())
            })
            .unwrap_or_default();

        info!("QQ音乐搜索 \"{}\" 找到 {} 首歌曲", keyword, songs.len());
        Ok(songs)
    }

    /// 获取歌曲详情
    pub async fn get_song_detail(&self, song_id: u64) -> Result<QQMusicSong> {
        let url = format!("{}/song/detail", self.base_url);

        let request = self.http
            .get(&url)
            .query(&[("ids", &song_id.to_string())]);
        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;

        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "获取QQ音乐歌曲详情失败".to_string(),
            });
        }

        let song = json
            .get("songs")
            .and_then(|s| s.as_array())
            .and_then(|arr| arr.first())
            .or_else(|| {
                json.get("data")
                    .and_then(|d| d.as_array())
                    .and_then(|arr| arr.first())
            })
            .or_else(|| {
                json.get("data")
            })
            .ok_or_else(|| BotError::KookApiError {
                code: 404,
                message: "歌曲不存在".to_string(),
            })?;

        serde_json::from_value(song.clone())
            .map_err(|e| BotError::KookApiError {
                code: -1,
                message: format!("解析QQ音乐歌曲详情失败: {}", e),
            })
    }

    /// 获取歌曲下载 URL
    pub async fn get_song_url(&self, song_id: u64) -> Result<Option<String>> {
        let url = format!("{}/song/url", self.base_url);
        let song_id_str = song_id.to_string();

        let request = self.http
            .get(&url)
            .query(&[("id", &song_id_str)]);

        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;

        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "获取QQ音乐歌曲URL失败".to_string(),
            });
        }

        let url = json
            .get("data")
            .and_then(|d| {
                // data 可能是数组 [{url: "..."}]
                if let Some(arr) = d.as_array() {
                    arr.first()
                        .and_then(|item| item.get("url"))
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                } else {
                    // 或者直接是 {url: "..."}
                    d.get("url")
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                }
            });

        Ok(url)
    }

    /// 获取歌单详情
    pub async fn get_playlist_detail(&self, playlist_id: u64) -> Result<QQPlaylistDetail> {
        let url = format!("{}/playlist/detail", self.base_url);

        let request = self.http
            .get(&url)
            .query(&[("id", &playlist_id.to_string())]);
        let response = self.add_cookie(request).send().await?;
        let json: serde_json::Value = response.json().await?;

        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            warn!("QQ音乐歌单API返回错误: code={}", code);
            return Err(BotError::KookApiError {
                code: code as i32,
                message: format!("获取QQ音乐歌单失败: code={}", code),
            });
        }

        let playlist = json.get("playlist").ok_or_else(|| {
            BotError::KookApiError {
                code: -1,
                message: "歌单数据为空".to_string(),
            }
        })?;

        let name = playlist.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("未知歌单")
            .to_string();

        let track_ids: Vec<u64> = if let Some(track_ids_json) = playlist.get("trackIds") {
            track_ids_json.as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            item.as_u64()
                                .or_else(|| item.get("id")?.as_u64())
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else if let Some(tracks) = playlist.get("tracks") {
            tracks.as_array()
                .map(|arr| arr.iter().filter_map(|t| t.get("id")?.as_u64()).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        info!("获取QQ音乐歌单 '{}' 成功，共 {} 首歌曲", name, track_ids.len());

        Ok(QQPlaylistDetail {
            id: playlist_id,
            name,
            track_ids,
        })
    }

    /// 下载歌曲到临时文件
    pub async fn download_song(&self, url: &str, song_id: u64) -> Result<String> {
        let cache_dir = std::path::Path::new(&self.cache_dir);
        if !cache_dir.exists() {
            std::fs::create_dir_all(cache_dir)?;
        }

        let file_path = cache_dir.join(format!("qq_{}.mp3", song_id));

        // 如果已存在且大于 1KB，直接返回
        if file_path.exists() {
            if let Ok(meta) = std::fs::metadata(&file_path) {
                if meta.len() > 1024 {
                    info!("QQ音乐歌曲已缓存: {:?}", file_path);
                    return Ok(file_path.to_string_lossy().to_string());
                }
            }
        }

        info!("正在下载QQ音乐歌曲: {} -> {:?}", url, file_path);

        let request = self.http.get(url);
        let response = self.add_cookie(request).send().await?;

        if !response.status().is_success() {
            return Err(BotError::KookApiError {
                code: response.status().as_u16() as i32,
                message: format!("下载失败: {}", response.status()),
            });
        }

        let bytes = response.bytes().await?;
        std::fs::write(&file_path, &bytes)?;

        info!("QQ音乐歌曲下载完成: {} bytes", bytes.len());
        Ok(file_path.to_string_lossy().to_string())
    }

    /// 搜索或通过 ID 获取歌曲
    pub async fn get_or_search(&self, input: &str) -> Result<(QQMusicSong, Option<String>)> {
        // 尝试解析为歌曲 ID
        if let Some(song_id) = Self::parse_song_id(input) {
            info!("解析到QQ音乐歌曲ID: {}", song_id);
            let song = self.get_song_detail(song_id).await?;
            let url = self.get_song_url(song_id).await?;
            return Ok((song, url));
        }

        // 搜索歌曲
        let songs = self.search(input, 1).await?;
        if songs.is_empty() {
            return Err(BotError::KookApiError {
                code: 404,
                message: format!("未找到QQ音乐歌曲: {}", input),
            });
        }

        let song = songs.into_iter().next().unwrap();
        let url = self.get_song_url(song.id).await?;
        Ok((song, url))
    }

    /// 转换为 Music 结构
    pub fn to_music(&self, song: &QQMusicSong) -> Music {
        let author = song.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let pic_url = if song.album.pic_url.is_empty() {
            "https://y.gtimg.cn/music/photo_new/T002R300x300M000000.jpg".to_string()
        } else {
            let mut url = song.album.pic_url.clone();
            if !url.contains('?') {
                url.push_str("?max_age=2592000");
            }
            url
        };

        Music {
            title: song.name.clone(),
            author,
            pic_url,
            platform: "QQ音乐".to_string(),
            source_url: Some(format!("https://y.qq.com/n/ryqq/songDetail/{}", song.id)),
            duration: if song.duration > 0 { Some(song.duration / 1000) } else { None },
            sender: Sender::default(),
        }
    }
}
