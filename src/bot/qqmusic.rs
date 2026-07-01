//! QQ 音乐模块 - QQ 音乐播放功能
//!
//! 提供 QQ 音乐播放支持，包括以下命令：
//! - qqmusic: 播放 QQ 音乐（支持搜索、歌曲链接、歌单链接）

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::common::play_state::PlayState;
use crate::core::config::BotConfig;
use crate::music::QQMusicClient;
use crate::player::VoiceStreamingInfo;

/// QQ 音乐播放命令
pub struct QQMusicCommand {
    qqmusic_client: Arc<RwLock<QQMusicClient>>,
    play_state: Arc<PlayState>,
    cache_dir: String,
    max_cache_size_mb: u64,
}

impl QQMusicCommand {
    pub fn new(
        qqmusic_client: Arc<RwLock<QQMusicClient>>,
        play_state: Arc<PlayState>,
        cache_dir: String,
        max_cache_size_mb: u64,
    ) -> Self {
        Self {
            qqmusic_client,
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
            info!("获取新推流地址: {}:{}", ip, port);

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
                    info!("🎵 QQ音乐歌曲播放完成");
                }
                Err(e) => {
                    error!("推流失败: {}", e);
                    play_state.set_stopped();
                }
            }
        })
    }

    /// 处理歌单播放
    async fn handle_playlist(
        &self,
        ctx: &CommandContext<'_>,
        playlist_id: u64,
    ) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        let guild_id = &ctx.data.extra.guild_id;
        let user_id = &ctx.data.author_id;

        let client = self.qqmusic_client.read().await;
        let playlist = match client.get_playlist_detail(playlist_id).await {
            Ok(p) => p,
            Err(e) => {
                return CommandResult::Error(format!("获取QQ音乐歌单失败: {}", e));
            }
        };

        if playlist.track_ids.is_empty() {
            return CommandResult::Reply("❌ 歌单为空".to_string());
        }

        let msg = format!(
            "📋 **QQ歌单：{}**\n共 {} 首歌曲，开始播放...",
            playlist.name,
            playlist.track_ids.len()
        );

        if let Some(api_client) = ctx.api_client.read().await.as_ref() {
            let _ = api_client.send_channel_message(channel_id, &msg).await;
        }

        let voice_channel = {
            if let Some(api_client) = ctx.api_client.read().await.as_ref() {
                match api_client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        return CommandResult::Error(format!("获取语音频道失败: {}", e));
                    }
                }
            } else {
                return CommandResult::Error("API 客户端不可用".to_string());
            }
        };

        let vc = match voice_channel {
            Some(vc) => vc,
            None => {
                return CommandResult::Reply("⚠️ 你当前不在任何语音频道中".to_string());
            }
        };

        let qqmusic_client = self.qqmusic_client.clone();
        let api_client = ctx.api_client.clone();
        let channel_id = channel_id.clone();
        let vc_id = vc.id.clone();
        let playlist_name = playlist.name.clone();
        let total_count = playlist.track_ids.len();
        let track_ids: Vec<u64> = playlist.track_ids.clone();
        let cache_dir = self.cache_dir.clone();
        let max_cache_size_mb = self.max_cache_size_mb;
        let bit_rate = ctx.config.audio.bit_rate;
        let play_state = self.play_state.clone();

        drop(client);




        // 加入语音频道一次，整个歌单共用
        let (voice_ip, voice_port, voice_streaming_info) = {
            if let Some(api_client) = api_client.read().await.as_ref() {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                match api_client.join_voice_channel(&vc_id).await {
                    Ok(conn_info) => {
                        let ip = conn_info.ip.clone().unwrap_or_default();
                        let port = conn_info.port.unwrap_or(0);
                        let info = VoiceStreamingInfo {
                            ip: ip.clone(),
                            port: port as u16,
                            rtcp_port: conn_info
                                .rtcp_port
                                .map(|p| p as u16)
                                .unwrap_or(port as u16 + 1),
                            rtcp_mux: conn_info.rtcp_mux.unwrap_or(true),
                            ssrc: conn_info
                                .audio_ssrc
                                .map(|s| s as u32)
                                .unwrap_or(1111),
                            pt: conn_info.audio_pt.map(|p| p as u8).unwrap_or(111),
                            bit_rate: conn_info.bitrate.unwrap_or(bit_rate),
                            sample_rate: 48000,
                            channels: 2,
                        };
                        (ip, port as u16, info)
                    }
                    Err(e) => {
                        warn!("加入语音失败: {}", e);
                        return CommandResult::Error(format!("加入语音失败: {}", e));
                    }
                }
            } else {
                return CommandResult::Error("API客户端未初始化".to_string());
            }
        };

        tokio::spawn(async move {
            info!("开始播放QQ歌单: {}，共 {} 首", playlist_name, total_count);

            // 预取歌曲详情（后台执行，不阻塞命令响应）
            let all_songs: Vec<Option<crate::player::Music>> = {
                let client = qqmusic_client.read().await;
                let mut songs = Vec::with_capacity(total_count.min(50));
                for tid in &track_ids {
                    match client.get_song_detail(*tid).await {
                        Ok(song) => songs.push(Some(client.to_music(&song))),
                        Err(e) => {
                            warn!("获取QQ歌曲 {} 详情失败: {}", tid, e);
                            songs.push(None);
                        }
                    }
                }
                songs
            };

            play_state.reset_stats();

            for (index, track_id) in track_ids.iter().enumerate() {


                let music = match &all_songs[index] {
                    Some(m) => m.clone(),
                    None => {
                        warn!("QQ音乐歌曲 {} 详情预获取失败，跳过", track_id);
                        continue;
                    }
                };
                info!("准备播放第 {} 首: {}", index + 1, music.title);

                let client = qqmusic_client.read().await;


                let audio_url = match client.get_song_url(*track_id).await {
                    Ok(Some(url)) => url,
                    _ => {
                        warn!("QQ音乐歌曲 {} 无法获取播放链接", music.title);
                        continue;
                    }
                };

                let local_file = match client.download_song(&audio_url, *track_id).await {
                    Ok(path) => path,
                    Err(e) => {
                        warn!("下载QQ音乐歌曲 {} 失败: {}", music.title, e);
                        continue;
                    }
                };

                crate::common::cache::cleanup_cache(&cache_dir, max_cache_size_mb).await;

                drop(client);

                let ip = voice_ip.clone();
                let port = voice_port;
                let streaming_info = voice_streaming_info.clone();

                // 发送播放卡片
                if let Some(api_client) = api_client.read().await.as_ref() {
                    if let Some(old_msg_id) = play_state.take_play_msg_id() {
                        let _ = api_client.delete_message(&old_msg_id).await;
                    }

                    use crate::common::card::{build_play_card, PlayCardData, PlayMusic, QueueMusic, Sender as CardSender};

                    let queue: Vec<QueueMusic> = all_songs[(index + 1)..]
                        .iter()
                        .filter_map(|opt| opt.as_ref())
                        .map(|m| QueueMusic {
                            title: m.title.clone(),
                            author: m.author.clone(),
                            platform: m.platform.clone(),
                            pic_url: m.pic_url.clone(),
                            sender: CardSender {
                                nick_name: "".to_string(),
                                avatar_url: None,
                            },
                        })
                        .collect();

                    let card_data = PlayCardData::new(PlayMusic {
                        title: music.title.clone(),
                        author: music.author.clone(),
                        platform: music.platform.clone(),
                        pic_url: music.pic_url.clone(),
                        sender: CardSender {
                            nick_name: "QQ歌单播放".to_string(),
                            avatar_url: None,
                        },
                    }).with_queue(queue, total_count);

                    let card_json = build_play_card(&card_data);
                    if let Ok(msg_id) = api_client.send_card_message(&channel_id, &card_json).await {
                        play_state.set_play_msg_id(msg_id);
                    }
                }

                play_state.set_playing(0);

                let file = local_file.clone();
                let ip_clone = ip.clone();
                let info = streaming_info.clone();
                let ps = play_state.clone();

                let handle = tokio::task::spawn_blocking(move || {
                    use crate::audio::{FFmpegDirectStreamer, StreamerConfig};

                    let mut streamer =
                        match FFmpegDirectStreamer::new(StreamerConfig::from(&info), ps.clone()) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("创建流处理器失败: {}", e);
                                ps.set_stopped();
                                return;
                            }
                        };

                    if let Err(e) = streamer.start_stream_url(
                        &file,
                        &ip_clone,
                        port,
                        info.rtcp_port,
                    ) {
                        error!("推流失败: {}", e);
                    } else {
                        let _ = streamer.wait();
                    }
                    ps.set_stopped();
                });
                let _ = handle.await;

                if play_state.is_stop_requested() {
                    info!("收到停止请求，终止歌单播放");
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }

            let stopped_by_user = play_state.is_stop_requested();
            play_state.reset_stats();

            if let Some(api_client) = api_client.read().await.as_ref() {
                if let Some(old_msg_id) = play_state.take_play_msg_id() {
                    let _ = api_client.delete_message(&old_msg_id).await;
                }

                if !stopped_by_user {
                    let msg = format!("✅ QQ歌单 **{}** 播放完成，感谢收听！", playlist_name);
                    let _ = api_client.send_channel_message(&channel_id, &msg).await;
                }
            }

            if let Some(api_client) = api_client.read().await.as_ref() {
                let _ = api_client.leave_voice_channel(&vc_id).await;
            }

            info!("QQ歌单播放完成");
        });

        CommandResult::Reply(format!("✅ QQ歌单开始播放，共 {} 首", total_count))
    }

    /// 处理单曲播放
    async fn handle_single(&self, ctx: &CommandContext<'_>, query: &str) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        let guild_id = &ctx.data.extra.guild_id;
        let user_id = &ctx.data.author_id;

        self.play_state.reset_stats();

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

        let client = self.qqmusic_client.read().await;
        match client.get_or_search(query).await {
            Ok((song, url)) => {
                let music = client.to_music(&song);

                if client.has_cookie() {
                    info!("✅ 使用已登录的QQ音乐账号");
                } else {
                    info!("⚠️ 未登录QQ音乐账号，可能只能播放试听版本");
                }

                match url {
                    Some(audio_url) => {
                        info!("获取到QQ音乐歌曲URL: {}", audio_url);

                        let local_file = match client.download_song(&audio_url, song.id).await {
                            Ok(path) => {
                                info!("QQ音乐歌曲下载成功: {}", path);
                                path
                            }
                            Err(e) => {
                                error!("下载QQ音乐歌曲失败: {}", e);
                                return CommandResult::Error(format!(
                                    "下载歌曲失败: {}",
                                    e
                                ));
                            }
                        };

                        crate::common::cache::cleanup_cache(
                            &self.cache_dir,
                            self.max_cache_size_mb,
                        )
                        .await;

                        drop(client);

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
                            self.play_state.set_playing(0);

                            let handle =
                                self.play_song(local_file, ip, port, streaming_info).await;

                            let api_client = ctx.api_client.clone();
                            let vc_id = vc_id_for_leave.clone();
                            let play_state = self.play_state.clone();

                            tokio::spawn(async move {
                                let _ = handle.await;
                                info!("QQ音乐单曲播放完成");

                                if let Some(client) = api_client.read().await.as_ref() {
                                    if let Some(msg_id) = play_state.take_play_msg_id() {
                                        let _ = client.delete_message(&msg_id).await;
                                    }
                                    let _ = client.leave_voice_channel(&vc_id).await;
                                }
                            });

                            CommandResult::Ok
                        } else {
                            CommandResult::Error("加入语音频道失败".to_string())
                        }
                    }
                    None => CommandResult::Reply(format!(
                        "❌ 无法获取 **{}** 的播放链接\n可能需要 VIP 或歌曲已下架",
                        song.name
                    )),
                }
            }
            Err(e) => CommandResult::Error(format!("搜索QQ音乐失败: {}", e)),
        }
    }
}

#[async_trait]
impl CommandHandler for QQMusicCommand {
    fn name(&self) -> &'static str {
        "qqmusic"
    }

    fn aliases(&self) -> Vec<&'static str> {
        vec!["qq", "qq音乐"]
    }

    fn description(&self) -> &'static str {
        "播放QQ音乐"
    }

    fn usage(&self) -> &'static str {
        "!qqmusic <歌曲链接或关键词>"
    }

    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        if ctx.args.is_empty() {
            return CommandResult::Reply(
                "❌ 请提供歌曲链接或搜索关键词\n用法: `/qqmusic <歌曲链接或关键词>`".to_string(),
            );
        }

        let query = ctx.args.join(" ");
        info!("处理 /qqmusic 命令: {}", query);

        // 检查是否是歌单链接
        if let Some(playlist_id) = QQMusicClient::parse_playlist_id(&query) {
            info!("检测到QQ音乐歌单链接，ID: {}", playlist_id);
            return self.handle_playlist(&ctx, playlist_id).await;
        }

        // 单曲处理
        self.handle_single(&ctx, &query).await
    }
}

/// 创建 QQ 音乐模块的所有命令
pub fn create_qqmusic_commands(
    qqmusic_client: Arc<RwLock<QQMusicClient>>,
    config: &BotConfig,
    play_state: Arc<PlayState>,
) -> Vec<Arc<dyn CommandHandler>> {
    vec![Arc::new(QQMusicCommand::new(
        qqmusic_client,
        play_state,
        config.music.cache_dir.clone(),
        config.music.max_cache_size_mb,
    ))]
}
