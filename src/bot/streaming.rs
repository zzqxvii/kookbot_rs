//! 共享的语音频道推流辅助函数
//!
//! 为所有音乐平台模块提供统一的"加入语音频道+推流+清理"逻辑。

use crate::bot::commands::CommandContext;
use crate::common::play_state::PlayState;
use crate::player::VoiceStreamingInfo;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use tracing::{error, info, warn};

/// 加入 Kook 语音频道并获取 RTP 推流信息。
pub async fn join_voice_for_streaming(
    ctx: &CommandContext<'_>,
    channel_id: &str,
    text_channel: &str,
) -> Option<(String, u16, VoiceStreamingInfo)> {
    let api_client = &ctx.api_client;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let conn_info = match api_client.join_voice_channel(channel_id).await {
        Ok(info) => info,
        Err(e) => {
            warn!("加入语音失败: {}", e);
            let _ = api_client
                .send_channel_message(text_channel, &format!("❌ 加入语音频道失败: {}", e))
                .await;
            return None;
        }
    };

    let ip = conn_info.ip.clone().unwrap_or_default();
    let port = conn_info.port.unwrap_or(0);
    if port == 0 {
        warn!("连接信息中端口为 0，无法推流");
        let _ = api_client
            .send_channel_message(text_channel, "❌ 获取语音连接信息失败：端口无效")
            .await;
        return None;
    }

    let bit_rate = conn_info.bitrate.unwrap_or(ctx.config.audio.bit_rate);
    info!("获取推流地址: {}:{}, bitrate={}, ssrc={:?}, pt={:?}, rtcp_port={:?}, rtcp_mux={:?}", 
        ip, port, bit_rate, conn_info.audio_ssrc, conn_info.audio_pt, conn_info.rtcp_port, conn_info.rtcp_mux);

    let streaming_info = VoiceStreamingInfo::from_conn(&conn_info, bit_rate);
    info!("推流参数: ssrc={}, pt={}, bitrate={}", streaming_info.ssrc, streaming_info.pt, streaming_info.bit_rate);

    Some((ip, port as u16, streaming_info))
}

/// 将音乐文件通过异步 I/O 喂入 FFmpeg stdin（共享实现）。
///
/// 跳过 ID3 标签头，分块读取并写入。每块间检查播放控制信号。
pub async fn feed_file_to_stdin(
    file_path: &str,
    stdin: &mut tokio::process::ChildStdin,
    play_state: &PlayState,
) {
    let mut file = match tokio::fs::File::open(file_path).await {
        Ok(f) => f,
        Err(e) => {
            error!("打开文件失败: {}: {}", file_path, e);
            return;
        }
    };

    // 跳过 ID3 标签
    let mut hdr = [0u8; 10];
    if file.read_exact(&mut hdr).await.is_ok() && &hdr[0..3] == b"ID3" {
        let skip = 10
            + ((hdr[6] as u64 & 0x7F) << 21)
            + ((hdr[7] as u64 & 0x7F) << 14)
            + ((hdr[8] as u64 & 0x7F) << 7)
            + (hdr[9] as u64 & 0x7F);
        file.seek(std::io::SeekFrom::Start(skip)).await.ok();
    } else {
        file.seek(std::io::SeekFrom::Start(0)).await.ok();
    }

    let mut buf = [0u8; 65536];
    loop {
        if play_state.is_next_requested() || play_state.is_stop_requested() {
            break;
        }
        match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if stdin.write_all(&buf[..n]).await.is_err() {
                    error!("写入 stdin 失败");
                    break;
                }
            }
            Err(e) => {
                error!("读取文件失败: {}", e);
                break;
            }
        }
    }
    stdin.flush().await.ok();
}

/// 歌单播放完成后的清理（共享实现）。
///
/// 删除播放卡片、发送完成/错误消息、离开语音频道。
pub async fn send_playlist_cleanup(
    api_client: &crate::api::KookClient,
    channel_id: &str,
    vc_id: &str,
    play_state: &Arc<PlayState>,
    success_msg: &str,
    error_msg: Option<&str>,
) {
    if let Some(old) = play_state.take_play_msg_id() {
        let _ = api_client.delete_message(&old).await;
    }
    if !play_state.is_stop_requested() {
        if let Some(err) = error_msg {
            let _ = api_client.send_channel_message(channel_id, &format!("❌ 播放出错: {}", err)).await;
        } else {
            let _ = api_client.send_channel_message(channel_id, success_msg).await;
        }
    }
    let _ = api_client.leave_voice_channel(vc_id).await;
}
