//! B站模块 - 哔哩哔哩音乐播放功能
//!
//! 提供 B站音乐播放支持，包括以下命令：
//! - bilibili: 播放B站音乐（支持搜索、BV号、视频链接）

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::common::play_state::PlayState;
use crate::core::config::BotConfig;
use crate::music::BilibiliClient;
use crate::player::VoiceStreamingInfo;

/// B站音乐播放命令
pub struct BilibiliCommand {
    bilibili_client: Arc<RwLock<BilibiliClient>>,
    play_state: Arc<PlayState>,
    cache_dir: String,
    max_cache_size_mb: u64,
}

impl BilibiliCommand {
    pub fn new(
        bilibili_client: Arc<RwLock<BilibiliClient>>,
        play_state: Arc<PlayState>,
        cache_dir: String,
        max_cache_size_mb: u64,
    ) -> Self {
        Self {
            bilibili_client,
            play_state,
            cache_dir,
            max_cache_size_mb,
        }
    }

    /// 加入语音频道并返回流信息
    async fn join_voice_for_streaming(
        &self,
        ctx: &CommandContext<'_>,
        channel_id: &str,
        text_channel: &str,
    ) -> Option<(String, u16, VoiceStreamingInfo)> {
        if let Some(api_client) = ctx.api_client.read().await.as_ref() {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let conn_info = match api_client.join_voice_channel(channel_id).await {
                Ok(info) => info,
                Err(e) => {
                    warn!("加入语音失败: {}", e);
                    let _ = api_client
                        .send_channel_message(
                            text_channel,
                            &format!("❌ 加入语音频道失败: {}", e),
                        )
                        .await;
                    return None;
                }
            };

            let ip = conn_info.ip.clone().unwrap_or_default();
            let port = conn_info.port.unwrap_or(0);
            if port == 0 {
                warn!("连接信息中端口为 0，无法推流");
                let _ = api_client
                    .send_channel_message(
                        text_channel,
                        "❌ 获取语音连接信息失败：端口无效",
                    )
                    .await;
                return None;
            }
            info!("获取B站推流地址: {}:{}", ip, port);

            let bit_rate = conn_info.bitrate.unwrap_or(ctx.config.audio.bit_rate);

            let streaming_info = VoiceStreamingInfo {
                ip: ip.clone(),
                port: port as u16,
                rtcp_port: conn_info
                    .rtcp_port
                    .map(|p| p as u16)
                    .unwrap_or(port as u16 + 1),
                rtcp_mux: conn_info.rtcp_mux.unwrap_or(true),
                ssrc: conn_info.audio_ssrc.map(|s| s as u32).unwrap_or(1111),
                pt: conn_info.audio_pt.map(|p| p as u8).unwrap_or(111),
                bit_rate,
                sample_rate: 48000,
                channels: 2,
            };

            return Some((ip, port as u16, streaming_info));
        }
        None
    }

    /// 播放歌曲文件（在后台线程中运行）
    async fn play_song(
        &self,
        file_path: String,
        ip: String,
        port: u16,
        streaming_info: VoiceStreamingInfo,
    ) -> tokio::task::JoinHandle<()> {
        let play_state = self.play_state.clone();
        tokio::task::spawn_blocking(move || {
            use crate::audio::{FFmpegDirectStreamer, StreamerConfig};

            let mut streamer = match FFmpegDirectStreamer::new(
                StreamerConfig::from(&streaming_info),
                play_state.clone(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    error!("创建流处理器失败: {}", e);
                    return;
                }
            };

            match streamer.start_stream_url(
                &file_path,
                &ip,
                port,
                streaming_info.rtcp_port,
            ) {
                Ok(_) => {
                    let _ = streamer.wait();
                    play_state.set_stopped();
                    info!("🎵 B站歌曲播放完成");
                }
                Err(e) => {
                    error!("推流失败: {}", e);
                    play_state.set_stopped();
                }
            }
        })
    }

    /// 处理单曲播放
    async fn handle_single(&self, ctx: &CommandContext<'_>, query: &str) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        let guild_id = &ctx.data.extra.guild_id;
        let user_id = &ctx.data.author_id;

        let voice_channel = {
            if let Some(api_client) = ctx.api_client.read().await.as_ref() {
                match api_client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        return CommandResult::Error(format!(
                            "获取语音频道信息失败: {}",
                            e
                        ));
                    }
                }
            } else {
                return CommandResult::Error("API 客户端不可用".to_string());
            }
        };

        let vc = match voice_channel {
            Some(vc) => vc,
            None => {
                return CommandResult::Reply(
                    "⚠️ 你当前不在任何语音频道中\n请先加入一个语音频道".to_string(),
                );
            }
        };

        let vc_id_for_leave = vc.id.clone();

        let (song, url, music) = {
            let client = self.bilibili_client.read().await;
            match client.get_or_search(query).await {
                Ok((song, url)) => {
                    let music = client.to_music(&song);

                    if client.has_cookie() {
                        info!("✅ 使用已登录的B站账号");
                    } else {
                        info!("⚠️ 未登录B站账号，可能只能播放试听版本");
                    }
                    (song, url, music)
                }
                Err(e) => return CommandResult::Error(format!("搜索B站失败: {}", e)),
            }
        }; // read lock dropped here

        match url {
            Some(audio_url) => {
                info!("获取到B站歌曲URL: {}", audio_url);

                // TODO: download_song hardcodes "./cache" instead of using config.cache_dir.
                // BilibiliClient::download_song should accept a cache_dir parameter.
                let client = self.bilibili_client.read().await;
                let local_file = match client.download_song(&audio_url, &song.bvid).await {
                    Ok(path) => {
                        info!("B站歌曲下载成功: {}", path);
                        path
                    }
                    Err(e) => {
                        error!("下载B站歌曲失败: {}", e);
                        return CommandResult::Error(format!(
                            "下载歌曲失败: {}",
                            e
                        ));
                    }
                };
                drop(client);

                crate::common::cache::cleanup_cache(
                    &self.cache_dir,
                    self.max_cache_size_mb,
                )
                .await;

                // 发送播放卡片
                if let Some(api_client) = ctx.api_client.read().await.as_ref() {
                    use crate::common::card::{
                        build_play_card, PlayCardData, PlayMusic, Sender as CardSender,
                    };

                    let card_data = PlayCardData::new(PlayMusic {
                        title: music.title.clone(),
                        author: music.author.clone(),
                        platform: music.platform.clone(),
                        pic_url: music.pic_url.clone(),
                        sender: CardSender {
                            nick_name: ctx.data.extra.author.nickname.clone(),
                            avatar_url: None,
                        },
                    });

                    let card_json = build_play_card(&card_data);
                    if let Ok(msg_id) =
                        api_client.send_card_message(channel_id, &card_json).await
                    {
                        self.play_state.set_play_msg_id(msg_id);
                    }
                }

                // 加入语音频道并播放
                if let Some((ip, port, streaming_info)) = self
                    .join_voice_for_streaming(ctx, &vc.id, channel_id)
                    .await
                {
                    let handle =
                        self.play_song(local_file, ip, port, streaming_info).await;

                    let api_client = ctx.api_client.clone();
                    let vc_id = vc_id_for_leave.clone();
                    let play_state = self.play_state.clone();

                    tokio::spawn(async move {
                        let _ = handle.await;
                        info!("B站单曲播放完成");

                        if let Some(client) = api_client.read().await.as_ref() {
                            if let Some(msg_id) = play_state.take_play_msg_id() {
                                let _ = client.delete_message(&msg_id).await;
                            }
                            let _ = client.leave_voice_channel(&vc_id).await;
                        } else {
                            warn!("B站单曲播放完成，但 API 客户端不可用，无法清理");
                        }
                    });

                    CommandResult::Ok
                } else {
                    CommandResult::Error("加入语音频道失败".to_string())
                }
            }
            None => CommandResult::Reply(format!(
                "❌ 无法获取 **{}** 的播放链接\n可能需要会员或视频已下架",
                song.name
            )),
        }
    }
}

#[async_trait]
impl CommandHandler for BilibiliCommand {
    fn name(&self) -> &'static str {
        "bilibili"
    }

    fn aliases(&self) -> Vec<&'static str> {
        vec!["bili", "b站"]
    }

    fn description(&self) -> &'static str {
        "播放B站音乐"
    }

    fn usage(&self) -> &'static str {
        "!bilibili <BV号/视频链接/搜索关键词>"
    }

    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        if ctx.args.is_empty() {
            return CommandResult::Reply(
                "❌ 请提供BV号、视频链接或搜索关键词\n用法: `/bilibili <BV号/视频链接/关键词>`".to_string(),
            );
        }

        let query = ctx.args.join(" ");
        info!("处理 /bilibili 命令: {}", query);

        // 单曲处理
        self.handle_single(&ctx, &query).await
    }
}

/// 创建 B站模块的所有命令
pub fn create_bilibili_commands(
    bilibili_client: Arc<RwLock<BilibiliClient>>,
    config: &BotConfig,
    play_state: Arc<PlayState>,
) -> Vec<Arc<dyn CommandHandler>> {
    vec![Arc::new(BilibiliCommand::new(
        bilibili_client,
        play_state,
        config.music.cache_dir.clone(),
        config.music.max_cache_size_mb,
    ))]
}
