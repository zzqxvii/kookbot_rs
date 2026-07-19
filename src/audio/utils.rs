//! 音频工具函数 — ID3 跳过、stdin 分块喂入等共享逻辑

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use tracing::{error, info, warn};

use crate::common::play_state::PlayState;

/// 跳过 MP3 文件的 ID3 标签头，将文件指针定位到实际音频数据起始处。
///
/// 支持 ID3v2.2（3 字节大端序大小）和 ID3v2.3/2.4（4 字节 synchsafe 大小）。
/// 如果不是 ID3 文件，指针回到文件开头。
pub fn skip_id3_tag(file: &mut File) {
    let mut hdr = [0u8; 10];
    if std::io::Read::read_exact(file, &mut hdr).is_ok() && &hdr[0..3] == b"ID3" {
        let major_version = hdr[3];
        let skip = if major_version == 2 {
            // ID3v2.2: 3 字节大端序大小
            10 + ((hdr[6] as u64) << 16) + ((hdr[7] as u64) << 8) + (hdr[8] as u64)
        } else {
            // ID3v2.3/2.4: 4 字节 synchsafe 大小
            10 + ((hdr[6] as u64 & 0x7F) << 21)
                + ((hdr[7] as u64 & 0x7F) << 14)
                + ((hdr[8] as u64 & 0x7F) << 7)
                + (hdr[9] as u64 & 0x7F)
        };
        Seek::seek(file, SeekFrom::Start(skip)).ok();
    } else {
        // 非 ID3 文件，回到开头
        Seek::seek(file, SeekFrom::Start(0)).ok();
    }
}

/// 将文件分块喂入 writer（通常是 FFmpeg stdin pipe），每块间检查停止/切歌标志。
///
/// 返回 `true` 表示正常读完文件，返回 `false` 表示被停止或切歌中断。
pub fn feed_file_to_stdin(
    file: &mut File,
    stdin: &mut impl Write,
    play_state: &PlayState,
    label: &str,
) -> bool {
    let mut buf = [0u8; 65536];
    loop {
        if play_state.is_next_requested() {
            play_state.clear_next_request();
            info!("切歌 → 下一首 ({})", label);
            return false;
        }
        if play_state.is_stop_requested() {
            info!("收到停止请求，终止 ({})", label);
            return false;
        }
        match Read::read(file, &mut buf) {
            Ok(0) => {
                info!("文件读取完毕 ({})", label);
                return true;
            }
            Ok(n) => {
                if Write::write_all(stdin, &buf[..n]).is_err() {
                    error!("写入 stdin 失败 ({})", label);
                    return false;
                }
            }
            Err(e) => {
                error!("读取文件失败: {} ({})", e, label);
                return false;
            }
        }
    }
}

/// 发送 Opus 静音握手包，建立 UDP 连接。
///
/// 在正式推流前发送静音帧，帮助网关建立 NAT 映射和 UDP 通路。
pub fn send_silence_handshake(ip: &str, port: u16) {
    match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(sock) => {
            sock.connect((ip, port)).ok();
            let silence = [0xF8, 0xFF, 0xFE]; // Opus 静音帧
            let _ = sock.send(&silence);
            info!("🔇 已发送静音握手包到 {}:{}", ip, port);
        }
        Err(e) => {
            warn!("发送静音握手包失败: {}", e);
        }
    }
}
