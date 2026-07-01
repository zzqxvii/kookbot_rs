//! 静音 RTP 发送器
//!
//! 在歌曲间隙发送 Opus 静音帧作为 RTP 包，
//! 保持 Kook 语音服务器的 UDP 连接活跃。
//!
//! ## 原理
//! - Kook 语音服务器要求持续的 RTP 流量
//! - FFmpeg 退出后 UDP socket 关闭，服务器超时断连
//! - 本模块以 ~650 bytes/s 的极低开销维持连接
//! - 静音流使用独立的 UDP 端口和随机 SSRC，服务器需接受多源端口
//!
//! ## Opus 静音帧
//! - 采样率: 48kHz, 立体声, 20ms 帧长 (960 samples/channel)
//! - 标准 3 字节 Opus 静音帧: [0xF8, 0xFF, 0xFE]
//! - 每帧约 3 bytes → 50 帧/秒 → ~150 bytes/s 实际载荷

use crate::core::error::{BotError, Result};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};

// ── 常量 ──────────────────────────────────────────────

/// 标准 3 字节 Opus 静音帧
/// TOC byte 0xF8: config 0 (SILK NB), stereo, 1 frame, CBR
/// 后两字节 0xFF 0xFE 构成有效的 Opus 静音载荷
const OPUS_SILENCE_FRAME: &[u8] = &[0xF8, 0xFF, 0xFE];
/// RTP 固定头长度 (12 bytes, 无 CSRC, 无扩展)
const RTP_HEADER_SIZE: usize = 12;

/// 48kHz 20ms = 960 samples
const SAMPLES_PER_FRAME: u32 = 960;

/// 帧间隔 (20ms)
const FRAME_INTERVAL_MS: u64 = 20;

// ── SilenceSender ──────────────────────────────────────

/// 静音 RTP 发送器
///
/// 在后台线程中循环发送静音 RTP 包，保持语音连接活跃。
///
/// # 使用
/// ```ignore
/// let mut sender = SilenceSender::new(&ip, port, pt)?;
/// sender.start();
/// // ... 下载下一首歌 ...
/// sender.stop();  // 停止静音
/// // 启动 FFmpeg 播放下一首
/// ```
pub struct SilenceSender {
    payload_type: u8,
    running: Arc<AtomicBool>,
    /// 目标地址 (IP, port)
    socket_addr: (String, u16),
}

impl SilenceSender {
    /// 创建静音发送器
    ///
    /// 静音流使用独立的随机 SSRC，与 FFmpeg 的 RTP 流区分，
    /// 避免 seq/timestamp 重置导致的接收端不连续。
    ///
    /// # Arguments
    /// * `dest_ip` - Kook 语音服务器 IP
    /// * `dest_port` - Kook 语音服务器端口
    /// * `payload_type` - RTP 负载类型 (Opus = PT)
    pub fn new(dest_ip: &str, dest_port: u16, payload_type: u8) -> Self {
        Self {
            payload_type,
            running: Arc::new(AtomicBool::new(false)),
            socket_addr: (dest_ip.to_string(), dest_port),
        }
    }
    /// 启动静音发送 (后台线程)
    ///
    /// 绑定独立的 UDP 端口并生成随机 SSRC。
    /// 静音流与 FFmpeg 的 RTP 流使用不同的源端口和 SSRC，
    /// Kook 语音服务器将其视为新的 RTP 源。
    pub fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            warn!("SilenceSender 已在运行");
            return Ok(());
        }

        let dest = format!("{}:{}", self.socket_addr.0, self.socket_addr.1);
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| BotError::IoError(e))?;
        socket
            .connect(&dest)
            .map_err(|e| BotError::IoError(e))?;

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let ssrc: u32 = rand::random::<u32>();
        let pt = self.payload_type;

        thread::spawn(move || {
            let mut seq: u16 = 0;
            let mut ts: u32 = 0;
            let packet = build_silence_rtp();

            while running.load(Ordering::SeqCst) {
                // 动态构建包头 (seq + timestamp 每帧变化)
                let mut buf = Vec::with_capacity(RTP_HEADER_SIZE + OPUS_SILENCE_FRAME.len());
                push_rtp_header(&mut buf, pt, seq, ts, ssrc);
                buf.extend_from_slice(&packet);

                if socket.send(&buf).is_err() {
                    debug!("SilenceSender: UDP send 失败, 停止");
                    break;
                }

                seq = seq.wrapping_add(1);
                ts = ts.wrapping_add(SAMPLES_PER_FRAME);
                thread::sleep(Duration::from_millis(FRAME_INTERVAL_MS));
            }
            debug!("SilenceSender: 线程退出");
        });

        info!(
            "🔇 SilenceSender 启动 → {}:{}, PT={}, SSRC={:#010x}",
            self.socket_addr.0, self.socket_addr.1, self.payload_type, ssrc
        );
        Ok(())
    }

    /// 停止静音发送
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        info!("🔇 SilenceSender 已停止");
    }

    /// 是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for SilenceSender {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── 辅助函数 ──────────────────────────────────────────

/// 返回预编码的 Opus 静音帧载荷
fn build_silence_rtp() -> Vec<u8> {
    OPUS_SILENCE_FRAME.to_vec()
}

/// 推入 RTP 包头到 buffer
fn push_rtp_header(buf: &mut Vec<u8>, pt: u8, seq: u16, timestamp: u32, ssrc: u32) {
    // Byte 0: V=2, P=0, X=0, CC=0
    buf.push(0x80);
    // Byte 1: M=0, PT
    buf.push(pt & 0x7F);
    // Bytes 2-3: sequence number (big-endian)
    buf.extend_from_slice(&seq.to_be_bytes());
    // Bytes 4-7: timestamp (big-endian)
    buf.extend_from_slice(&timestamp.to_be_bytes());
    // Bytes 8-11: SSRC (big-endian)
    buf.extend_from_slice(&ssrc.to_be_bytes());
}

// ── 测试 ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silence_sender_new_and_drop() {
        let sender = SilenceSender::new("127.0.0.1", 9000, 111);
        assert!(!sender.is_running());
        drop(sender);
    }

    #[test]
    fn test_rtp_header_size() {
        let mut buf = Vec::new();
        push_rtp_header(&mut buf, 111, 0, 0, 12345);
        assert_eq!(buf.len(), 12);
        // V=2, P=0, X=0, CC=0
        assert_eq!(buf[0], 0x80);
        // M=0, PT=111
        assert_eq!(buf[1], 111);
    }

    #[test]
    fn test_silence_frame() {
        // 标准 3 字节 Opus 静音帧
        assert_eq!(OPUS_SILENCE_FRAME.len(), 3);
        assert_eq!(OPUS_SILENCE_FRAME[0], 0xF8);
        assert_eq!(OPUS_SILENCE_FRAME[1], 0xFF);
        assert_eq!(OPUS_SILENCE_FRAME[2], 0xFE);
    }
}
