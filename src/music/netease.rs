use crate::error::{BotError, Result};
use crate::player::Music;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};
use regex::Regex;

/// 网易云音乐 API 客户端
pub struct NeteaseClient {
    http: Client,
    base_url: String,
    cookie: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct NeteaseResponse<T> {
    code: i32,
    #[serde(default)]
    data: Option<T>,
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
struct SearchResult {
    #[serde(alias = "songs", default)]
    songs: Vec<NeteaseSong>,
}

#[derive(Debug, Clone, Deserialize)]
struct SongUrlData {
    #[serde(default)]
    url: Option<String>,
    #[serde(alias = "br", default)]
    bitrate: u32,
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

impl NeteaseClient {
    pub fn new(base_url: &str) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("创建 HTTP 客户端失败");

        Self {
            http,
            base_url: base_url.to_string(),
            cookie: None,
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
        self.cookie.is_some()
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
            // 登录成功，从响应中获取 cookie
            json.get("cookie")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
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
        let patterns = [
            // music.163.com/#/song?id=xxx
            r"music\.163\.com/(?:#/)?song\?id=(\d+)",
            // music.163.com/song/media/outer/url?id=xxx
            r"music\.163\.com/song/media/outer/url\?id=(\d+)",
            // y.music.163.com/m/song?appid=...&id=xxx
            r"y\.music\.163\.com/m/song[^\d]*(\d+)",
            // 分享链接: https://share.music.163.com/xxx?songId=xxx
            r"songId[=:](\d+)",
            // 短链接中的ID
            r"id[=:](\d+)",
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

    /// 搜索歌曲
    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<NeteaseSong>> {
        let url = format!("{}/cloudsearch", self.base_url);
        
        let response = self.http
            .get(&url)
            .query(&[
                ("keywords", keyword),
                ("type", "1"),
                ("limit", &limit.to_string()),
            ])
            .send()
            .await?;

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
            .and_then(|s| serde_json::from_value(s.clone()).ok())
            .unwrap_or_default();

        info!("搜索 \"{}\" 找到 {} 首歌曲", keyword, songs.len());
        Ok(songs)
    }

    /// 获取歌曲详情
    pub async fn get_song_detail(&self, song_id: u64) -> Result<NeteaseSong> {
        let url = format!("{}/song/detail", self.base_url);
        
        let response = self.http
            .get(&url)
            .query(&[("ids", &song_id.to_string())])
            .send()
            .await?;

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
        let br = "320000".to_string();
        
        let request = self.http
            .get(&url)
            .query(&[
                ("id", &song_id_str),
                ("br", &br),
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
        let level = "exhigh".to_string();
        
        let request = self.http
            .get(&url)
            .query(&[
                ("id", &song_id_str),
                ("level", &level),
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

        let song = songs.into_iter().next().unwrap();
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

        Music {
            title: song.name.clone(),
            author,
            pic_url: if song.album.pic_url.is_empty() {
                "https://p1.music.126.net/6y-UleORITEDbvrOLV0Q8A==/5639395138885805.jpg".to_string()
            } else {
                song.album.pic_url.clone()
            },
            platform: "网易云".to_string(),
            source_url: Some(format!("https://music.163.com/#/song?id={}", song.id)),
            duration: Some(song.duration / 1000),
        }
    }
}
