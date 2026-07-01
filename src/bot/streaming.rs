//! 共享的语音频道推流辅助函数
//!
//! 为所有音乐平台模块提供统一的"加入语音频道并获取推流信息"逻辑。

use crate::bot::commands::CommandContext;
use crate::player::VoiceStreamingInfo;
use tracing::{info, warn};

/// 加入 Kook 语音频道并获取 RTP 推流信息。
///
/// 所有音乐平台（网易云、QQ音乐、B站）共享此逻辑。
/// 包含 500ms 延迟确保频道状态就绪，以及端口有效性检查。
///
/// 返回 `(服务器IP, 端口, 推流信息)` 或 `None`（失败时已发送错误消息）。
pub async fn join_voice_for_streaming(
    ctx: &CommandContext<'_>,
    channel_id: &str,
    text_channel: &str,
) -> Option<(String, u16, VoiceStreamingInfo)> {
    let api_guard = ctx.api_client.read().await;
    let api_client = api_guard.as_ref()?;
    // 等待 500ms 确保频道状态就绪
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

    info!("获取推流地址: {}:{}", ip, port);

    let bit_rate = conn_info.bitrate.unwrap_or(ctx.config.audio.bit_rate);
    let streaming_info = VoiceStreamingInfo::from_conn(&conn_info, bit_rate);

    Some((ip, port as u16, streaming_info))
}
