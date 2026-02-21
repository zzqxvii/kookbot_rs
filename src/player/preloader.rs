use crate::core::error::{BotError, Result};
use super::Music;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

/// 预加载任务
#[derive(Debug, Clone)]
pub struct PreloadTask {
    /// 音乐信息
    pub music: Music,
    /// 目标路径
    pub target_path: PathBuf,
    /// 优先级（越高越优先）
    pub priority: i32,
}

/// 预加载状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreloadStatus {
    /// 等待中
    Pending,
    /// 正在下载
    Downloading { progress: u8 },
    /// 已完成
    Completed { path: PathBuf },
    /// 失败
    Failed { error: String },
    /// 已取消
    Cancelled,
}

/// 预加载管理器配置
#[derive(Debug, Clone)]
pub struct PreloaderConfig {
    /// 最大并发下载数
    pub max_concurrent_downloads: usize,
    /// 缓存目录
    pub cache_dir: PathBuf,
    /// 最大缓存大小(MB)
    pub max_cache_size_mb: u64,
    /// 预加载数量（提前下载多少首歌）
    pub preload_ahead: usize,
    /// 是否启用预加载
    pub enabled: bool,
}

impl Default for PreloaderConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 3,
            cache_dir: PathBuf::from("./cache"),
            max_cache_size_mb: 1024,
            preload_ahead: 2,
            enabled: true,
        }
    }
}

/// 预加载管理器
pub struct PreloadManager {
    /// 配置
    config: PreloaderConfig,
    /// 预加载状态
    statuses: Arc<Mutex<HashMap<String, PreloadStatus>>>,
    /// 任务发送通道
    task_tx: mpsc::Sender<PreloadTask>,
}

impl PreloadManager {
    /// 创建新的预加载管理器
    pub fn new(config: PreloaderConfig) -> Self {
        let (task_tx, _) = mpsc::channel(100);

        // 确保缓存目录存在
        let cache_dir = config.cache_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = fs::create_dir_all(&cache_dir).await {
                warn!("创建缓存目录失败: {}", e);
            }
        });

        Self {
            config,
            statuses: Arc::new(Mutex::new(HashMap::new())),
            task_tx,
        }
    }

    /// 提交预加载任务
    pub async fn submit(&self, task: PreloadTask) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let music_id = self.get_music_id(&task.music);

        // 检查是否已存在
        let statuses = self.statuses.lock().await;
        if let Some(status) = statuses.get(&music_id) {
            match status {
                PreloadStatus::Completed { .. } | PreloadStatus::Downloading { .. } => {
                    debug!("音乐 {} 已在预加载状态: {:?}", music_id, status);
                    return Ok(());
                }
                _ => {}
            }
        }
        drop(statuses);

        // 更新状态为等待中
        self.update_status(&music_id, PreloadStatus::Pending).await;

        // 提交任务
        self.task_tx
            .send(task)
            .await
            .map_err(|e| BotError::ConfigError(format!("提交预加载任务失败: {}", e)))?;

        debug!("提交预加载任务: {}", music_id);
        Ok(())
    }

    /// 获取音乐 ID（用于缓存键）
    fn get_music_id(&self, music: &Music) -> String {
        format!("{}_{}", music.platform, music.title).replace(|c: char| !c.is_alphanumeric(), "_")
    }

    /// 更新预加载状态
    async fn update_status(&self, music_id: &str, status: PreloadStatus) {
        let mut statuses = self.statuses.lock().await;
        statuses.insert(music_id.to_string(), status);
    }

    /// 检查音乐是否已就绪
    pub async fn is_ready(&self, music: &Music) -> bool {
        let music_id = self.get_music_id(music);
        let statuses = self.statuses.lock().await;

        matches!(
            statuses.get(&music_id),
            Some(PreloadStatus::Completed { .. })
        )
    }

    /// 获取缓存路径
    pub async fn get_cached_path(&self, music: &Music) -> Option<PathBuf> {
        let music_id = self.get_music_id(music);
        let statuses = self.statuses.lock().await;

        if let Some(PreloadStatus::Completed { path }) = statuses.get(&music_id) {
            Some(path.clone())
        } else {
            None
        }
    }

    /// 预加载接下来的 N 首歌曲
    pub async fn preload_ahead(
        &self,
        playlist: &[(Music, String)],
        current_index: usize,
        count: usize,
    ) {
        for i in 1..=count {
            let idx = current_index + i;
            if idx >= playlist.len() {
                break;
            }

            let (music, _) = &playlist[idx];

            // 检查是否已就绪
            if self.is_ready(music).await {
                continue;
            }

            // 提交预加载任务
            let task = PreloadTask {
                music: music.clone(),
                target_path: self.config.cache_dir.join(self.get_cached_filename(music)),
                priority: (count - i + 1) as i32, // 越近的优先级越高
            };

            if let Err(e) = self.submit(task).await {
                warn!("提交预加载任务失败: {}", e);
            }
        }
    }

    /// 获取缓存文件名
    fn get_cached_filename(&self, music: &Music) -> String {
        let id = self.get_music_id(music);
        format!("{}.mp3", id)
    }

    /// 清理缓存
    pub async fn cleanup_cache(&self, max_size_mb: u64) -> Result<()> {
        let mut entries = Vec::new();

        // 读取缓存目录
        let mut dir = fs::read_dir(&self.config.cache_dir)
            .await
            .map_err(|e| BotError::IoError(e))?;

        while let Some(entry) = dir.next_entry().await.map_err(|e| BotError::IoError(e))? {
            let metadata = entry.metadata().await.map_err(|e| BotError::IoError(e))?;

            if metadata.is_file() {
                let size = metadata.len();
                let modified = metadata.modified().map_err(|e| BotError::IoError(e))?;

                entries.push((entry.path(), size, modified));
            }
        }

        // 计算总大小
        let total_size: u64 = entries.iter().map(|(_, size, _)| size).sum();
        let max_size = max_size_mb * 1024 * 1024;

        if total_size <= max_size {
            debug!(
                "缓存大小正常: {} / {} MB",
                total_size / 1024 / 1024,
                max_size_mb
            );
            return Ok(());
        }

        // 按修改时间排序（删除最旧的）
        entries.sort_by(|a, b| a.2.cmp(&b.2));

        let mut freed = 0u64;
        for (path, size, _) in entries {
            if total_size - freed <= max_size {
                break;
            }

            match fs::remove_file(&path).await {
                Ok(_) => {
                    freed += size;
                    info!("删除缓存文件: {:?}, 释放 {} MB", path, size / 1024 / 1024);
                }
                Err(e) => {
                    warn!("删除缓存文件失败: {:?} - {}", path, e);
                }
            }
        }

        info!("缓存清理完成，释放 {} MB", freed / 1024 / 1024);
        Ok(())
    }
}

impl Default for PreloadManager {
    fn default() -> Self {
        Self::new(PreloaderConfig::default())
    }
}
