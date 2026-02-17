//! Gateway 心跳管理
//!
//! 负责定期发送心跳包，检测连接状态

use crate::gateway::protocol::GatewayPayload;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, error, trace, warn};

/// 心跳管理器
pub struct HeartbeatManager {
    /// 心跳间隔 (毫秒)
    interval_ms: u64,
    /// 最后一次收到心跳确认的时间
    last_ack: Arc<RwLock<Instant>>,
    /// 最后一次发送的序列号
    last_seq: Arc<RwLock<Option<u64>>>,
    /// 心跳发送通道
    heartbeat_tx: mpsc::Sender<GatewayPayload>,
    /// 运行状态
    running: Arc<RwLock<bool>>,
    /// 超时检测任务句柄
    timeout_handle: Option<tokio::task::JoinHandle<()>>,
    /// 心跳发送任务句柄
    heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
}

impl HeartbeatManager {
    /// 创建新的心跳管理器
    pub fn new(interval_ms: u64, heartbeat_tx: mpsc::Sender<GatewayPayload>) -> Self {
        Self {
            interval_ms,
            last_ack: Arc::new(RwLock::new(Instant::now())),
            last_seq: Arc::new(RwLock::new(None)),
            heartbeat_tx,
            running: Arc::new(RwLock::new(false)),
            timeout_handle: None,
            heartbeat_handle: None,
        }
    }

    /// 启动心跳管理
    pub async fn start(&mut self) {
        let mut running = self.running.write().await;
        if *running {
            debug!("心跳管理器已在运行");
            return;
        }
        *running = true;
        drop(running);

        debug!("启动心跳管理器，间隔: {}ms", self.interval_ms);

        // 启动心跳发送任务
        let heartbeat_tx = self.heartbeat_tx.clone();
        let interval_ms = self.interval_ms;
        let running_clone = self.running.clone();
        let last_seq = self.last_seq.clone();

        self.heartbeat_handle = Some(tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(interval_ms));

            // 第一次立即发送心跳
            let seq = *last_seq.read().await;
            let payload = GatewayPayload::heartbeat(seq);

            if let Err(e) = heartbeat_tx.send(payload).await {
                error!("发送心跳失败: {}", e);
                return;
            }
            debug!("发送心跳，seq: {:?}", seq);

            loop {
                ticker.tick().await;

                // 检查是否仍在运行
                if !*running_clone.read().await {
                    debug!("心跳发送任务停止");
                    break;
                }

                let seq = *last_seq.read().await;
                let payload = GatewayPayload::heartbeat(seq);

                match heartbeat_tx.send(payload).await {
                    Ok(_) => {
                        trace!("发送心跳，seq: {:?}", seq);
                    }
                    Err(e) => {
                        error!("发送心跳失败: {}", e);
                        break;
                    }
                }
            }
        }));

        // 启动超时检测任务
        let last_ack = self.last_ack.clone();
        let running_clone = self.running.clone();
        let timeout_ms = self.interval_ms * 2 + 1000; // 2个心跳间隔 + 1秒缓冲

        self.timeout_handle = Some(tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_secs(5));

            loop {
                check_interval.tick().await;

                // 检查是否仍在运行
                if !*running_clone.read().await {
                    debug!("超时检测任务停止");
                    break;
                }

                let elapsed = last_ack.read().await.elapsed().as_millis() as u64;
                if elapsed > timeout_ms {
                    warn!("心跳超时: 上次确认 {}ms 前，阈值 {}ms", elapsed, timeout_ms);
                    // 这里可以触发重连逻辑
                }
            }
        }));
    }

    /// 停止心跳管理
    pub async fn stop(&mut self) {
        let mut running = self.running.write().await;
        if !*running {
            return;
        }
        *running = false;
        drop(running);

        debug!("停止心跳管理器");

        // 取消超时检测任务
        if let Some(handle) = self.timeout_handle.take() {
            handle.abort();
        }

        // 取消心跳发送任务
        if let Some(handle) = self.heartbeat_handle.take() {
            handle.abort();
        }
    }

    /// 更新心跳确认时间
    pub async fn ack(&self) {
        let mut last_ack = self.last_ack.write().await;
        *last_ack = Instant::now();
        trace!("收到心跳确认");
    }

    /// 更新序列号
    pub async fn update_seq(&self, seq: u64) {
        let mut last_seq = self.last_seq.write().await;
        *last_seq = Some(seq);
    }

    /// 获取当前序列号
    pub async fn get_seq(&self) -> Option<u64> {
        *self.last_seq.read().await
    }

    /// 检查是否在运行
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// 获取心跳间隔
    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

impl Drop for HeartbeatManager {
    fn drop(&mut self) {
        // 尝试停止任务，但不阻塞
        if let Some(handle) = self.timeout_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.heartbeat_handle.take() {
            handle.abort();
        }
    }
}
