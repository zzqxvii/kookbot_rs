use crate::error::{BotError, Result};
use crate::models::Music;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{info, warn};

/// 队列项状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueItemStatus {
    /// 等待下载
    Pending,
    /// 正在下载
    Downloading,
    /// 已下载/就绪
    Ready { path: PathBuf },
    /// 下载失败
    Failed { error: String },
    /// 正在播放
    Playing,
    /// 已播放完成
    Completed,
}

/// 播放队列项
#[derive(Debug, Clone)]
pub struct QueueItem {
    /// 音乐信息
    pub music: Music,
    /// 请求者 ID
    pub requested_by: String,
    /// 状态
    pub status: QueueItemStatus,
    /// 下载/播放优先级（越高越优先）
    pub priority: i32,
}

impl QueueItem {
    /// 检查是否已就绪（可播放）
    pub fn is_ready(&self) -> bool {
        matches!(self.status, QueueItemStatus::Ready { .. })
    }

    /// 获取文件路径（如果已就绪）
    pub fn file_path(&self) -> Option<&PathBuf> {
        match &self.status {
            QueueItemStatus::Ready { path } => Some(path),
            _ => None,
        }
    }
}

/// 播放队列配置
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// 最大队列长度
    pub max_size: usize,
    /// 预加载数量（提前下载多少首歌）
    pub preload_count: usize,
    /// 是否允许重复歌曲
    pub allow_duplicates: bool,
    /// 自动播放
    pub autoplay: bool,
    /// 随机播放
    pub shuffle: bool,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_size: 100,
            preload_count: 2, // 预加载 2 首歌（当前播放 + 下两首）
            allow_duplicates: false,
            autoplay: true,
            shuffle: false,
        }
    }
}

/// 播放队列管理器
pub struct QueueManager {
    /// 配置
    config: QueueConfig,
    /// 主队列
    queue: Arc<RwLock<VecDeque<QueueItem>>>,
    /// 当前播放索引
    current_index: Arc<RwLock<Option<usize>>>,
    /// 预加载任务通道
    preload_tx: mpsc::Sender<usize>,
    /// 预加载任务接收器
    preload_rx: Arc<Mutex<mpsc::Receiver<usize>>>,
}

impl QueueManager {
    /// 创建新的队列管理器
    pub fn new(config: QueueConfig) -> Self {
        let (preload_tx, preload_rx) = mpsc::channel(config.max_size);

        Self {
            config,
            queue: Arc::new(RwLock::new(VecDeque::with_capacity(100))),
            current_index: Arc::new(RwLock::new(None)),
            preload_tx,
            preload_rx: Arc::new(Mutex::new(preload_rx)),
        }
    }

    /// 添加歌曲到队列
    pub async fn add(&self, music: Music, requested_by: String) -> Result<()> {
        // 检查队列是否已满
        let queue = self.queue.read().await;
        if queue.len() >= self.config.max_size {
            return Err(BotError::ConfigError("播放队列已满".to_string()));
        }
        drop(queue);

        // 检查是否允许重复
        if !self.config.allow_duplicates {
            let queue = self.queue.read().await;
            if queue
                .iter()
                .any(|item| item.music.title == music.title && item.music.author == music.author)
            {
                return Err(BotError::ConfigError("歌曲已在队列中".to_string()));
            }
        }

        // 创建队列项
        let item = QueueItem {
            music,
            requested_by,
            status: QueueItemStatus::Pending,
            priority: 0,
        };

        // 添加到队列
        let mut queue = self.queue.write().await;
        queue.push_back(item);
        let new_index = queue.len() - 1;
        drop(queue);

        // 触发预加载
        let _ = self.preload_tx.send(new_index).await;

        info!("已添加歌曲到队列，当前队列长度: {}", new_index + 1);
        Ok(())
    }

    /// 获取下一首要播放的歌曲（并更新索引）
    pub async fn next(&self) -> Option<QueueItem> {
        let mut current = self.current_index.write().await;

        // 先计算下一首索引
        let next_index = {
            let queue = self.queue.read().await;
            match *current {
                Some(idx) if idx + 1 < queue.len() => idx + 1,
                None if !queue.is_empty() => 0,
                _ => return None,
            }
        };

        // 更新之前歌曲的状态
        if let Some(idx) = *current {
            let mut queue = self.queue.write().await;
            if let Some(item) = queue.get_mut(idx) {
                item.status = QueueItemStatus::Completed;
            }
        }

        // 获取下一首
        let item = {
            let queue = self.queue.read().await;
            queue.get(next_index).cloned()
        };

        // 更新当前索引和状态
        *current = Some(next_index);
        {
            let mut queue = self.queue.write().await;
            if let Some(ref mut item) = queue.get_mut(next_index) {
                item.status = QueueItemStatus::Playing;
            }
        }

        // 触发后续歌曲的预加载
        for i in 1..=self.config.preload_count {
            let preload_idx = next_index + i;
            let _ = self.preload_tx.try_send(preload_idx);
        }

        item
    }

    /// 获取当前播放的歌曲
    pub async fn current(&self) -> Option<QueueItem> {
        let current = self.current_index.read().await;
        let queue = self.queue.read().await;

        (*current).and_then(|idx| queue.get(idx)).cloned()
    }

    /// 获取队列长度
    pub async fn len(&self) -> usize {
        self.queue.read().await.len()
    }

    /// 检查队列是否为空
    pub async fn is_empty(&self) -> bool {
        self.queue.read().await.is_empty()
    }

    /// 清空队列
    pub async fn clear(&self) {
        let mut queue = self.queue.write().await;
        queue.clear();
        *self.current_index.write().await = None;
        info!("播放队列已清空");
    }

    /// 获取队列列表
    pub async fn list(&self) -> Vec<QueueItem> {
        self.queue.read().await.iter().cloned().collect()
    }

    /// 移除指定索引的歌曲
    pub async fn remove(&self, index: usize) -> Result<()> {
        let mut queue = self.queue.write().await;

        if index >= queue.len() {
            return Err(BotError::ConfigError("索引超出范围".to_string()));
        }

        // 不能移除正在播放的歌曲
        if let Some(current) = *self.current_index.read().await {
            if current == index {
                return Err(BotError::ConfigError("不能移除正在播放的歌曲".to_string()));
            }
        }

        queue.remove(index);
        info!("已移除队列中索引 {} 的歌曲", index);
        Ok(())
    }

    /// 启动预加载任务
    pub async fn start_preload_task(
        &self,
        downloader: impl Fn(
                usize,
                QueueItem,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<PathBuf>> + Send + 'static>,
            > + Send
            + Sync
            + 'static,
    ) {
        let preload_rx = Arc::clone(&self.preload_rx);
        let queue = Arc::clone(&self.queue);

        tokio::spawn(async move {
            let mut rx = preload_rx.lock().await;

            while let Some(index) = rx.recv().await {
                let queue_guard = queue.read().await;
                if let Some(item) = queue_guard.get(index) {
                    // 检查是否已就绪
                    if item.is_ready() {
                        continue;
                    }

                    let item_clone = item.clone();
                    drop(queue_guard);

                    // 更新状态为下载中
                    {
                        let mut queue_guard = queue.write().await;
                        if let Some(item) = queue_guard.get_mut(index) {
                            item.status = QueueItemStatus::Downloading;
                        }
                    }

                    // 执行下载
                    match downloader(index, item_clone).await {
                        Ok(path) => {
                            let mut queue_guard = queue.write().await;
                            if let Some(item) = queue_guard.get_mut(index) {
                                item.status = QueueItemStatus::Ready { path };
                            }
                            info!("预加载完成: 索引 {}", index);
                        }
                        Err(e) => {
                            let mut queue_guard = queue.write().await;
                            if let Some(item) = queue_guard.get_mut(index) {
                                item.status = QueueItemStatus::Failed {
                                    error: e.to_string(),
                                };
                            }
                            warn!("预加载失败: 索引 {} - {}", index, e);
                        }
                    }
                }
            }
        });
    }
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new(QueueConfig::default())
    }
}
