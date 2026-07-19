use crate::core::error::{BotError, Result};
use crate::player::{Music, Sender};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use std::sync::LazyLock;
use tracing::{debug, info, warn};
use regex::Regex;

/// 网易云音乐 API 客户端
pub struct NeteaseClient {
    http: Client,
    base_url: String,
    cookie: Option<String>,
    cache_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseSong {
    pub id: u64,
    pub name: String,
    #[serde(alias = "ar")]
    pub artists: Vec<NeteaseArtist>,
    #[serde(alias = "al")]
    pub album: NeteaseAlbum,
    #[serde(alias = "dt", default)]
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
    #[serde(alias = "picUrl", default)]
    pub pic_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrKeyData {
    pub unikey: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrCodeData {
    pub qrurl: String,
    #[serde(default)]
    pub qrimg: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoginResult {
    pub code: i32,
    pub cookie: Option<String>,
    pub nickname: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlaylistDetail {
    pub id: u64,
    pub name: String,
    pub track_ids: Vec<u64>,
}

static SONG_ID_PATTERNS: LazyLock<[Regex; 4]> = LazyLock::new(|| [
    // music.163.com/#/song?id=xxx
    Regex::new(r"music\.163\.com/(?:#/)?song\?id=(\d+)").unwrap(),
    // music.163.com/song/media/outer/url?id=xxx
    Regex::new(r"music\.163\.com/song/media/outer/url\?id=(\d+)").unwrap(),
    // y.music.163.com/m/song?appid=...&id=xxx
    Regex::new(r"y\.music\.163\.com/m/song[^\d]*(\d+)").unwrap(),
    // 分享链接: https://share.music.163.com/xxx?songId=xxx
    Regex::new(r"songId[=:](\d+)").unwrap(),
]);

impl NeteaseClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| BotError::StartupError(format!("创建 HTTP 客户端失败: {}", e)))?;

        Ok(Self {
            http,
            base_url: base_url.to_string(),
            cookie: None,
            cache_dir: String::from("./cache"),
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
    pub fn set_cache_dir(&mut self, dir: String) {
        self.cache_dir = dir;
    }
    
    /// 检查 API 是否可用
    pub async fn check_api(&self) -> Result<()> {
        let url = format!("{}/login/status", self.base_url);
        
        let response = self.http.get(&url).send().await?;
        
        if response.status().is_success() {
            Ok(())
        } else {
            Err(BotError::MusicApiError {
                code: response.status().as_u16() as i32,
                message: format!("网易云 API 返回错误状态: {}", response.status()),
            })
        }
    }
    
    /// 获取登录二维码 key
    pub async fn get_qr_key(&self) -> Result<QrKeyData> {
        let url = format!("{}/login/qr/key", self.base_url);
        
        let response = self.http.get(&url).send().await?;
        let json: serde_json::Value = response.json().await?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "获取二维码key失败".to_string(),
            });
        }
        
        let data = json.get("data").ok_or_else(|| BotError::KookApiError {
            code: -1,
            message: "响应数据为空".to_string(),
        })?;
        
        serde_json::from_value(data.clone())
            .map_err(|e| BotError::KookApiError {
                code: -1,
                message: format!("解析二维码key失败: {}", e),
            })
    }
    
    /// 生成二维码
    pub async fn create_qr_code(&self, key: &str) -> Result<QrCodeData> {
        let url = format!("{}/login/qr/create", self.base_url);
        
        let response = self.http
            .get(&url)
            .query(&[
                ("key", key),
                ("qrimg", "true"),
            ])
            .send()
            .await?;
        
        let json: serde_json::Value = response.json().await?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "生成二维码失败".to_string(),
            });
        }
        
        let data = json.get("data").ok_or_else(|| BotError::KookApiError {
            code: -1,
            message: "响应数据为空".to_string(),
        })?;
        
        serde_json::from_value(data.clone())
            .map_err(|e| BotError::KookApiError {
                code: -1,
                message: format!("解析二维码失败: {}", e),
            })
    }
    
    /// 检查二维码登录状态
    /// 
    /// 返回码说明:
    /// - 800: 二维码已过期
    /// - 801: 等待扫码
    /// - 802: 待确认
    /// - 803: 授权登录成功
    pub async fn check_qr_status(&self, key: &str) -> Result<LoginResult> {
        let url = format!("{}/login/qr/check", self.base_url);
        
        let response = self.http
            .get(&url)
            .query(&[("key", key)])
            .send()
            .await?;
        
        let json: serde_json::Value = response.json().await?;
        
        info!("二维码状态响应: {}", json);
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        
        let cookie = if code == 803 {
            // 登录成功，从响应中获取并清理 cookie
            json.get("cookie")
                .and_then(|c| c.as_str())
                .map(crate::common::utils::clean_cookie)
        } else {
            None
        };
        
        let nickname = json.get("nickname")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        
        Ok(LoginResult {
            code: code as i32,
            cookie,
            nickname,
        })
    }

    /// 解析网易云音乐分享链接，提取歌曲ID
    pub fn parse_song_id(input: &str) -> Option<u64> {
        // 尝试直接解析为数字ID
        if let Ok(id) = input.parse::<u64>() {
            return Some(id);
        }

        // 匹配各种分享链接格式
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
        // 尝试直接解析为数字ID
        if let Ok(id) = input.parse::<u64>() {
            return Some(id);
        }

        let patterns = [
            // music.163.com/#/playlist?id=xxx
            r"music\.163\.com/(?:#/)?playlist\?id=(\d+)",
            // playlist?id=xxx
            r"playlist\?id=(\d+)",
        ];

        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(input) {
                    if let Some(m) = caps.get(1) {
                        if let Ok(id) = m.as_str().parse::<u64>() {
                            return Some(id);
                        }
                    }
                }
            }
        }

        None
    }

    /// 获取歌单详情
    pub async fn get_playlist_detail(&self, playlist_id: u64) -> Result<PlaylistDetail> {
        let url = format!("{}/playlist/detail", self.base_url);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO)
            .as_millis() as u64;
        
        let mut request = self.http
            .get(&url)
            .query(&[
                ("id", &playlist_id.to_string()),
                ("timestamp", &timestamp.to_string()),
            ]);
        
        // 添加 cookie
        if let Some(ref cookie) = self.cookie {
            request = request.header("Cookie", cookie);
        }
        
        let response = request.send().await?;
        let json: serde_json::Value = response.json().await?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            warn!("歌单API返回错误: code={}, 响应: {}", code, json);
            return Err(BotError::KookApiError {
                code: code as i32,
                message: format!("获取歌单失败: code={}", code),
            });
        }
        
        let playlist = json.get("playlist").ok_or_else(|| {
            warn!("歌单响应中缺少 playlist 字段: {}", json);
            BotError::KookApiError {
                code: -1,
                message: "歌单数据为空".to_string(),
            }
        })?;
        
        let name = playlist.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("未知歌单")
            .to_string();
        
        // 尝试从 trackIds 或 tracks 获取歌曲
        let track_ids: Vec<u64> = if let Some(track_ids_json) = playlist.get("trackIds") {
            track_ids_json.as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            // trackIds 可能是数字或对象 {id: xxx, v: xxx}
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
            warn!("歌单数据中找不到 trackIds 或 tracks，playlist: {}", 
                serde_json::to_string(playlist).unwrap_or_default());
            Vec::new()
        };
        
        info!("获取歌单 '{}' 成功，共 {} 首歌曲", name, track_ids.len());
        
        Ok(PlaylistDetail {
            id: playlist_id,
            name,
            track_ids,
        })
    }

    /// 搜索歌曲
    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<NeteaseSong>> {
        let url = format!("{}/cloudsearch", self.base_url);
        
        let request = self.http
            .get(&url)
            .query(&[
                ("keywords", keyword),
                ("type", "1"),
                ("limit", &limit.to_string()),
            ]);
        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "搜索失败".to_string(),
            });
        }

        let songs: Vec<NeteaseSong> = json
            .get("result")
            .and_then(|r| r.get("songs"))
            .and_then(|s| serde_json::from_value(s.clone())
                .map_err(|e| { warn!("解析歌曲失败: {}", e); e }).ok())
            .unwrap_or_default();
        info!("搜索 \"{}\" 找到 {} 首歌曲", keyword, songs.len());
        Ok(songs)
    }

    /// 获取歌曲详情
    pub async fn get_song_detail(&self, song_id: u64) -> Result<NeteaseSong> {
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
                message: "获取歌曲详情失败".to_string(),
            });
        }

        let song = json
            .get("songs")
            .and_then(|s| s.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| BotError::KookApiError {
                code: 404,
                message: "歌曲不存在".to_string(),
            })?;

        serde_json::from_value(song.clone())
            .map_err(|e| BotError::KookApiError {
                code: -1,
                message: format!("解析歌曲详情失败: {}", e),
            })
    }

    /// 批量获取歌曲详情（分批获取，每批最多100首）
    /// 网易云API支持传入逗号分隔的多个ID，但有数量限制
    pub async fn get_songs_detail(&self, song_ids: &[u64]) -> Result<Vec<NeteaseSong>> {
        if song_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_songs = Vec::new();
        let chunk_size = 100; // 每批最多100首
        
        for chunk in song_ids.chunks(chunk_size) {
            let url = format!("{}/song/detail", self.base_url);
            let ids_str = chunk.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            
            debug!("批量获取歌曲，ID数量: {}", chunk.len());
            
            let request = self.http
                .get(&url)
                .query(&[("ids", &ids_str)]);
            
            let response = self.add_cookie(request).send().await?;
            let json: serde_json::Value = response.json().await?;
            
            let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
            if code != 200 {
                warn!("批量获取歌曲详情失败，code: {}, 响应: {:?}", code, json);
                continue;
            }

            let songs: Vec<NeteaseSong> = json
                .get("songs")
                .and_then(|s| serde_json::from_value(s.clone())
                    .map_err(|e| { warn!("批量解析歌曲失败: {}", e); e }).ok())
                .unwrap_or_default();
            
            all_songs.extend(songs);
        }

        info!("批量获取 {} 首歌曲详情，返回 {} 首", song_ids.len(), all_songs.len());
        Ok(all_songs)
    }

    /// 添加 cookie 到请求头
    fn add_cookie(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref cookie) = self.cookie {
            request.header("Cookie", cookie)
        } else {
            request
        }
    }

    /// 获取歌曲下载URL
    pub async fn get_song_url(&self, song_id: u64) -> Result<Option<String>> {
        let url = format!("{}/song/url", self.base_url);
        let song_id_str = song_id.to_string();
        let request = self.http
            .get(&url)
            .query(&[
                ("id", song_id_str.as_str()),
                ("br", "320000"),
            ]);
        
        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "获取歌曲URL失败".to_string(),
            });
        }

        let url = json
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string());

        Ok(url)
    }

    /// 获取歌曲下载URL (V2备用接口)
    pub async fn get_song_url_v2(&self, song_id: u64) -> Result<Option<String>> {
        let url = format!("{}/song/url/v1", self.base_url);
        let song_id_str = song_id.to_string();
        let request = self.http
            .get(&url)
            .query(&[
                ("id", song_id_str.as_str()),
                ("level", "exhigh"),
            ]);
        
        let response = self.add_cookie(request).send().await?;

        let json: serde_json::Value = response.json().await?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 200 {
            return Err(BotError::KookApiError {
                code: code as i32,
                message: "获取歌曲URL(V2)失败".to_string(),
            });
        }

        let url = json
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string());

        Ok(url)
    }
    
    /// 下载歌曲到临时文件
    /// 返回本地文件路径
    pub async fn download_song(&self, url: &str, song_id: u64) -> Result<String> {
        let cache_dir = std::path::Path::new(&self.cache_dir);
        if !cache_dir.exists() {
            std::fs::create_dir_all(cache_dir)?;
        }
        
        let file_path = cache_dir.join(format!("{}.mp3", song_id));
        
        // 如果已存在且大于1KB，直接返回（避免重复下载）
        if file_path.exists() {
            if let Ok(meta) = std::fs::metadata(&file_path) {
                if meta.len() > 1024 {
                    info!("歌曲已缓存: {:?}", file_path);
                    return Ok(file_path.to_string_lossy().to_string());
                }
            }
        }
        
        info!("正在下载歌曲: {} -> {:?}", url, file_path);
        
        // 不向 CDN 发送登录 Cookie
        let response = self.http.get(url).send().await?;
        
        if !response.status().is_success() {
            return Err(BotError::KookApiError {
                code: response.status().as_u16() as i32,
                message: format!("下载失败: {}", response.status()),
            });
        }
        
        let bytes = response.bytes().await?;
        std::fs::write(&file_path, &bytes)?;
        
        info!("歌曲下载完成: {} bytes", bytes.len());
        Ok(file_path.to_string_lossy().to_string())
    }

    /// 搜索或通过ID获取歌曲
    pub async fn get_or_search(&self, input: &str) -> Result<(NeteaseSong, Option<String>)> {
        // 尝试解析为歌曲ID
        if let Some(song_id) = Self::parse_song_id(input) {
            info!("解析到歌曲ID: {}", song_id);
            let song = self.get_song_detail(song_id).await?;
            let url = self.get_song_url(song_id).await?;
            return Ok((song, url));
        }

        // 搜索歌曲
        let songs = self.search(input, 1).await?;
        if songs.is_empty() {
            return Err(BotError::KookApiError {
                code: 404,
                message: format!("未找到歌曲: {}", input),
            });
        }

        let song = songs.into_iter().next().ok_or_else(|| BotError::MusicApiError {
            code: 500,
            message: "搜索结果为空".into(),
        })?;
        let url = self.get_song_url(song.id).await?;
        Ok((song, url))
    }

    /// 转换为 Music 结构
    pub fn to_music(&self, song: &NeteaseSong) -> Music {
        let author = song.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let pic_url = if song.album.pic_url.is_empty() {
            "https://p1.music.126.net/6y-UleORITEDbvrOLV0Q8A==/5639395138885805.jpg?param=130y130".to_string()
        } else {
            // 网易云图片添加缩略图参数
            let mut url = song.album.pic_url.clone();
            if !url.contains('?') {
                url.push_str("?param=130y130");
            }
            url
        };

        Music {
            title: song.name.clone(),
            author,
            pic_url,
            platform: "网易云".to_string(),
            source_url: Some(format!("https://music.163.com/#/song?id={}", song.id)),
            duration: Some(song.duration / 1000),
            sender: Sender::default(),
        }
    }

    /// 获取歌词
    pub async fn get_lyric(&self, song_id: u64) -> Result<Option<String>> {
        let url = format!("{}/lyric", self.base_url);

        #[derive(Deserialize)]
        struct LyricData {
            code: i32,
            lrc: Option<LyricContent>,
        }
        #[derive(Deserialize)]
        struct LyricContent {
            lyric: Option<String>,
        }

        let response = self.http
            .get(&url)
            .query(&[("id", song_id.to_string())])
            .send()
            .await?;

        let data: LyricData = response.json().await?;
        if data.code != 200 {
            return Ok(None);
        }
        Ok(data.lrc.and_then(|l| l.lyric))
    }
}
