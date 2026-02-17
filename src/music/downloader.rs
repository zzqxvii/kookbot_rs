use crate::error::{BotError, Result};
use crate::player::Music;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, warn};

/// 下载进度回调
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send + Sync>;

/// 音乐下载器
pub struct MusicDownloader {
    http: reqwest::Client,
    cache_dir: PathBuf,
    progress_callback: Option<ProgressCallback>,
}

impl MusicDownloader {
    /// 创建新的下载器
    pub fn new(cache_dir: impl AsRef<Path>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("创建 HTTP 客户端失败");

        Self {
            http,
            cache_dir: cache_dir.as_ref().to_path_buf(),
            progress_callback: None,
        }
    }

    /// 设置进度回调
    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// 从 URL 下载音乐
    pub async fn download_from_url(
        &self,
        url: &str,
        filename: &str,
    ) -> Result<PathBuf> {
        let target_path = self.cache_dir.join(filename);

        // 检查是否已缓存
        if target_path.exists() {
            let metadata = fs::metadata(&target_path).await
                .map_err(|e| BotError::IoError(e))?;

            if metadata.len() > 0 {
                debug!("使用缓存文件: {:?}", target_path);
                return Ok(target_path);
            }
        }

        // 确保缓存目录存在
        fs::create_dir_all(&self.cache_dir).await
            .map_err(|e| BotError::IoError(e))?;

        info!("开始下载: {} -> {:?}", url, target_path);

        // 发送请求
        let response = self.http.get(url)
            .send()
            .await
            .map_err(|e| BotError::HttpError(e))?;

        let status = response.status();
        if !status.is_success() {
            return Err(BotError::KookApiError {
                code: status.as_u16() as i32,
                message: format!("HTTP error: {}", status),
            });
        }

        // 获取总大小
        let total_size = response.content_length();

        // 下载并写入文件
        let mut file = fs::File::create(&target_path).await
            .map_err(|e| BotError::IoError(e))?;

        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| BotError::HttpError(e))?;
            file.write_all(&chunk).await
                .map_err(|e| BotError::IoError(e))?;

            downloaded += chunk.len() as u64;

            // 调用进度回调
            if let Some(ref callback) = self.progress_callback {
                callback(downloaded, total_size.unwrap_or(0));
            }
        }

        file.flush().await.map_err(|e| BotError::IoError(e))?;

        info!("下载完成: {:?} ({} 字节)", target_path, downloaded);
        Ok(target_path)
    }

    /// 下载音乐（自动获取 URL）
    pub async fn download(
        &self,
        music: &Music,
        url_provider: impl FnOnce(&Music) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'static>>,
    ) -> Result<PathBuf> {
        let url = url_provider(music).await?;
        let filename = format!("{}_{}.mp3", music.platform, safe_filename(&music.title));

        self.download_from_url(&url, &filename).await
    }

    /// 获取缓存路径（如果已缓存）
    pub fn get_cached_path(&self,
        music: &Music,
    ) -> Option<PathBuf> {
        let filename = format!("{}_{}.mp3", music.platform, safe_filename(&music.title));
        let path = self.cache_dir.join(filename);

        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// 清理缓存
    pub async fn clear_cache(&self,
    ) -> Result<()> {
        if !self.cache_dir.exists() {
            return Ok(());
        }

        let mut entries = fs::read_dir(&self.cache_dir).await
            .map_err(|e| BotError::IoError(e))?;

        let mut count = 0;
        while let Some(entry) = entries.next_entry().await
            .map_err(|e| BotError::IoError(e))? {
            let path = entry.path();
            if path.is_file() {
                fs::remove_file(&path).await
                    .map_err(|e| BotError::IoError(e))?;
                count += 1;
            }
        }

        info!("清理缓存完成，删除 {} 个文件", count);
        Ok(())
    }
}

/// 生成安全的文件名
fn safe_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}