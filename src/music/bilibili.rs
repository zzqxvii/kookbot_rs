use crate::core::error::{BotError, Result};
use crate::player::{Music, Sender};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};
use regex::Regex;
use std::sync::LazyLock;

/// 哔哩哔哩音乐 API 客户端
pub struct BilibiliClient {
    http: Client,
    base_url: String,
    cookie: Option<String>,
    cache_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BilibiliSong {
    pub bvid: String,
    pub name: String,
    pub author: BilibiliAuthor,
    #[serde(default)]
    pub duration: u64,
    #[serde(default)]
    pub pic_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BilibiliAuthor {
    pub name: String,
}

static BV_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^BV[a-zA-Z0-9]{10}$").unwrap());
static URL_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| [
    Regex::new(r"bilibili\.com/video/(BV[a-zA-Z0-9]{10})").unwrap(),
    Regex::new(r"b23\.tv/(BV[a-zA-Z0-9]{10})").unwrap(),
    Regex::new(r"bilibili\.com/.*[/?&]bvid=(BV[a-zA-Z0-9]{10})").unwrap(),
]);

impl BilibiliClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let http = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; RKM-Bot/1.0)")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| BotError::StartupError(format!("创建 HTTP 客户端失败: {}", e)))?;

        Ok(Self {
            http,
            base_url: base_url.to_string(),
            cookie: None,
            cache_dir: PathBuf::from("./cache"),
        })
    }

    pub fn with_cookie(base_url: &str, cookie: Option<String>) -> Result<Self> {
        let mut client = Self::new(base_url)?;
        client.cookie = cookie;
        Ok(client)
    }

    pub fn set_cookie(&mut self, cookie: String) {
        self.cookie = Some(cookie);
    }

    pub fn has_cookie(&self) -> bool {
        self.cookie.as_deref().is_some_and(|c| !c.is_empty())
    }

    /// 设置缓存目录
    pub fn set_cache_dir(&mut self, dir: impl Into<PathBuf>) {
        self.cache_dir = dir.into();
    }

    /// 添加 cookie 到请求头
    fn add_cookie(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref cookie) = self.cookie {
            request.header("Cookie", cookie)
        } else {
            request
        }
    }

    /// 解析 BV 号
    /// 支持格式:
    /// - BV1xx411c7mD (直接的BV号)
    /// - https://www.bilibili.com/video/BV1xx411c7mD
    /// - https://b23.tv/BV1xx411c7mD
    pub fn parse_bvid(input: &str) -> Option<String> {
        if BV_PATTERN.is_match(input) {
            return Some(input.to_string());
        }

        // 从 URL 中提取 BV 号
        for re in URL_PATTERNS.iter() {
            if let Some(caps) = re.captures(input) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }

        None
    }

    /// 搜索歌曲
    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<BilibiliSong>> {
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
            return Err(BotError::MusicApiError {
                code: code as i32,
                message: "B站搜索失败".to_string(),
            });
        }

        // 尝试多种响应格式
        let songs: Vec<BilibiliSong> = json
            .get("data")
            .and_then(|d| d.get("list"))
            .and_then(|l| serde_json::from_value(l.clone()).map_err(|e| {
                warn!("B站搜索结果解析失败 (data.list): {}", e);
            }).ok())
            .or_else(|| {
                json.get("result")
                    .and_then(|r| r.get("list"))
                    .and_then(|l| serde_json::from_value(l.clone()).map_err(|e| {
                        warn!("B站搜索结果解析失败 (result.list): {}", e);
                    }).ok())
            })
            .or_else(|| {
                json.get("data")
                    .and_then(|d| serde_json::from_value(d.clone()).map_err(|e| {
                        warn!("B站搜索结果解析失败 (data): {}", e);
                    }).ok())
            })
            .unwrap_or_default();

        info!("B站搜索 \"{}\" 找到 {} 首歌曲", keyword, songs.len());
        Ok(songs)
    }

    /// 获取歌曲详情
    pub async fn get_song_detail(&self, bvid: &str) -> Result<BilibiliSong> {
        let url = format!("{}/song/detail", self.base_url);

        let request = self.http
            .get(&url)
            .query(&[("bvid", bvid)]);
        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;

        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::MusicApiError {
                code: code as i32,
                message: "获取B站歌曲详情失败".to_string(),
            });
        }

        let song = json
            .get("data")
            .ok_or_else(|| BotError::MusicApiError {
                code: 404,
                message: "歌曲不存在".to_string(),
            })?;

        serde_json::from_value(song.clone())
            .map_err(|e| BotError::MusicApiError {
                code: -1,
                message: format!("解析B站歌曲详情失败: {}", e),
            })
    }

    /// 获取歌曲音频 URL
    pub async fn get_song_url(&self, bvid: &str) -> Result<Option<String>> {
        let url = format!("{}/song/url", self.base_url);

        let request = self.http
            .get(&url)
            .query(&[("bvid", bvid)]);

        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;

        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::MusicApiError {
                code: code as i32,
                message: "获取B站歌曲URL失败".to_string(),
            });
        }

        let audio_url = json
            .get("data")
            .and_then(|d| {
                if let Some(arr) = d.as_array() {
                    arr.first()
                        .and_then(|item| item.get("url"))
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                } else {
                    d.get("url")
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                }
            });

        Ok(audio_url)
    }

    /// 下载歌曲到临时文件
    pub async fn download_song(&self, url: &str, bvid: &str) -> Result<String> {
        if !self.cache_dir.exists() {
            tokio::fs::create_dir_all(&self.cache_dir).await?;
        }

        let file_path = self.cache_dir.join(format!("bili_{}.mp3", bvid));

        // 如果已存在且大于 1KB，直接返回
        if file_path.exists() {
            if let Ok(meta) = tokio::fs::metadata(&file_path).await {
                if meta.len() > 1024 {
                    info!("B站歌曲已缓存: {:?}", file_path);
                    return Ok(file_path.to_string_lossy().to_string());
                }
            }
        }

        info!("正在下载B站歌曲: {} -> {:?}", url, file_path);

        let request = self.http.get(url);
        let response = self.add_cookie(request).send().await?;

        if !response.status().is_success() {
            return Err(BotError::MusicApiError {
                code: response.status().as_u16() as i32,
                message: format!("下载失败: {}", response.status()),
            });
        }

        let mut file = tokio::fs::File::create(&file_path).await?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;

        info!("B站歌曲下载完成: {:?}", file_path);
        Ok(file_path.to_string_lossy().to_string())
    }

    /// 搜索或通过 BV 号获取歌曲
    pub async fn get_or_search(&self, input: &str) -> Result<(BilibiliSong, Option<String>)> {
        // 尝试解析为 BV 号
        if let Some(bvid) = Self::parse_bvid(input) {
            info!("解析到B站BV号: {}", bvid);
            let song = self.get_song_detail(&bvid).await?;
            let url = self.get_song_url(&bvid).await?;
            return Ok((song, url));
        }

        // 搜索歌曲
        let songs = self.search(input, 1).await?;
        if songs.is_empty() {
            return Err(BotError::MusicApiError {
                code: 404,
                message: format!("未找到B站歌曲: {}", input),
            });
        }

        let song = songs.into_iter().next().ok_or_else(|| BotError::MusicApiError {
            code: 500,
            message: "搜索结果为空".into(),
        })?;
        let url = self.get_song_url(&song.bvid).await?;
        Ok((song, url))
    }

    /// 转换为 Music 结构
    pub fn to_music(&self, song: &BilibiliSong) -> Music {
        let pic_url = if song.pic_url.is_empty() {
            "https://i0.hdslb.com/bfs/static/jinkela/long/images/logo.png".to_string()
        } else {
            song.pic_url.clone()
        };

        Music {
            title: song.name.clone(),
            author: song.author.name.clone(),
            pic_url,
            platform: "B站".to_string(),
            source_url: Some(format!("https://www.bilibili.com/video/{}", song.bvid)),
            duration: if song.duration > 0 {
                Some(song.duration / 1000)
            } else {
                None
            },
            sender: Sender::default(),
        }
    }
}
