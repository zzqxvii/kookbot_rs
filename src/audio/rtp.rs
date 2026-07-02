use crate::core::error::{BotError, Result};
use std::net::UdpSocket;
use std::time::Instant;
use tracing::{info, trace, warn};

/// RTP 包结构
#[derive(Debug, Clone)]
pub struct RtpPacket {
    /// 版本号 (2 bits), 固定为 2
    version: u8,
    /// 填充位 (1 bit)
    padding: bool,
    /// 扩展位 (1 bit)
    extension: bool,
    /// CSRC 计数 (4 bits)
    csrc_count: u8,
    /// 标记位 (1 bit)
    marker: bool,
    /// 负载类型 (7 bits)
    payload_type: u8,
    /// 序列号 (16 bits)
    sequence_number: u16,
    /// 时间戳 (32 bits)
    timestamp: u32,
    /// SSRC (32 bits)
    ssrc: u32,
    /// 负载数据
    payload: Vec<u8>,
}

impl RtpPacket {
    /// 创建新的 RTP 包
    pub fn new(
        payload_type: u8,
        sequence_number: u16,
        timestamp: u32,
        ssrc: u32,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            version: 2,
            padding: false,
            extension: false,
            csrc_count: 0,
            marker: false,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            payload,
        }
    }

    /// 设置标记位
    pub fn with_marker(mut self, marker: bool) -> Self {
        self.marker = marker;
        self
    }

    /// 序列化为新 Vec（测试/兼容性用途，生产代码使用 `write_to` 复用缓冲区）
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12 + self.payload.len());
        self.write_to(&mut buf);
        buf
    }

    /// 序列化到已有缓冲区（复用分配，不清除原内容）
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        buf.reserve(12 + self.payload.len());

        // 字节 0: 版本(2) | 填充(1) | 扩展(1) | CSRC 计数(4)
        let byte0 = (self.version << 6)
            | (if self.padding { 0x20 } else { 0 })
            | (if self.extension { 0x10 } else { 0 })
            | (self.csrc_count & 0x0F);
        buf.push(byte0);

        // 字节 1: 标记(1) | 负载类型(7)
        let byte1 = (if self.marker { 0x80 } else { 0 }) | (self.payload_type & 0x7F);
        buf.push(byte1);

        // 字节 2-3: 序列号
        buf.extend_from_slice(&self.sequence_number.to_be_bytes());

        // 字节 4-7: 时间戳
        buf.extend_from_slice(&self.timestamp.to_be_bytes());

        // 字节 8-11: SSRC
        buf.extend_from_slice(&self.ssrc.to_be_bytes());

        // 负载
        buf.extend_from_slice(&self.payload);
    }
}

/// RTP 发送器
pub struct RtpSender {
    socket: UdpSocket,
    dest_addr: String,
    ssrc: u32,
    sequence_number: u16,
    timestamp: u32,
    payload_type: u8,
    sample_rate: u32,
    frame_duration_ms: u32,
    last_send_time: Option<Instant>,
    packets_sent: u64,
    bytes_sent: u64,
    /// 预分配发送缓冲区，避免每帧重复分配
    send_buf: Vec<u8>,
}

impl RtpSender {
    /// 创建新的 RTP 发送器
    pub fn new(
        dest_addr: impl Into<String>,
        ssrc: u32,
        payload_type: u8,
        sample_rate: u32,
    ) -> Result<Self> {
        let dest_addr = dest_addr.into();
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| {
            BotError::NetworkError(format!("无法绑定 UDP 套接字: {}", e))
        })?;

        socket.connect(&dest_addr).map_err(|e| {
            BotError::NetworkError(format!("无法连接到目标地址 {}: {}", dest_addr, e))
        })?;

        // 设置非阻塞模式（可选）
        socket.set_nonblocking(true).map_err(|e| {
            BotError::NetworkError(format!("设置非阻塞模式失败: {}", e))
        })?;

        info!(
            "RTP 发送器创建成功: 目标={}, SSRC={}, 采样率={}Hz",
            dest_addr, ssrc, sample_rate
        );

        Ok(Self {
            socket,
            dest_addr,
            ssrc,
            sequence_number: 0,
            timestamp: 0,
            payload_type,
            sample_rate,
            frame_duration_ms: 20,
            last_send_time: None,
            packets_sent: 0,
            bytes_sent: 0,
            send_buf: Vec::with_capacity(1500), // 预分配 MTU 大小
        })
    }

    /// 设置帧持续时间（毫秒）
    pub fn with_frame_duration(mut self, ms: u32) -> Self {
        self.frame_duration_ms = ms;
        self
    }

    /// 发送 Opus 帧（复用预分配缓冲区，避免每帧分配）
    pub fn send_opus_frame(
        &mut self,
        opus_data: &[u8],
    ) -> Result<()> {
        // 复用预分配缓冲区序列化 RTP 包（避免每帧 Vec 分配）
        self.send_buf.clear();
        let packet = RtpPacket::new(
            self.payload_type,
            self.sequence_number,
            self.timestamp,
            self.ssrc,
            opus_data.to_vec(),
        );
        packet.write_to(&mut self.send_buf);

        // 发送
        match self.socket.send(&self.send_buf) {
            Ok(sent) => {
                self.packets_sent += 1;
                self.bytes_sent += sent as u64;
                trace!(
                    "发送 RTP 包: seq={}, ts={}, size={}",
                    self.sequence_number,
                    self.timestamp,
                    sent
                );
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::WouldBlock {
                    warn!("发送 RTP 包失败: {}", e);
                }
            }
        }

        // 更新序列号和时间戳
        self.sequence_number = self.sequence_number.wrapping_add(1);
        let samples_per_frame = (self.sample_rate as u64 * self.frame_duration_ms as u64) / 1000;
        self.timestamp = self.timestamp.wrapping_add(samples_per_frame as u32);

        self.last_send_time = Some(std::time::Instant::now());

        Ok(())
    }

    /// 获取统计信息
    pub fn stats(&self) -> RtpStats {
        RtpStats {
            packets_sent: self.packets_sent,
            bytes_sent: self.bytes_sent,
            sequence_number: self.sequence_number,
            timestamp: self.timestamp,
        }
    }

    /// 获取目标地址
    pub fn dest_addr(&self) -> &str {
        &self.dest_addr
    }
}

/// RTP 统计信息
#[derive(Debug, Clone, Copy)]
pub struct RtpStats {
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub sequence_number: u16,
    pub timestamp: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtp_packet_serialization() {
        let packet = RtpPacket::new(
            111,    // payload type (opus)
            12345,  // sequence number
            67890,  // timestamp
            0x12345678, // ssrc
            vec![0x01, 0x02, 0x03, 0x04], // payload
        );

        let bytes = packet.to_bytes();

        // 验证 RTP 头
        assert_eq!(bytes[0], 0x80); // V=2, P=0, X=0, CC=0
        assert_eq!(bytes[1], 0x6F); // M=0, PT=111
        assert_eq!(u16::from_be_bytes([bytes[2], bytes[3]]), 12345);
        assert_eq!(u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 67890);
        assert_eq!(u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 0x12345678);
    }
}