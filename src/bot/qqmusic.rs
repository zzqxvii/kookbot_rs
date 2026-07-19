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


    /// 播放歌曲文件（在后台线程中运行）
    async fn play_song(
        &self,
        file_path: String,
        ip: String,
        port: u16,
        streaming_info: VoiceStreamingInfo,
    ) -> tokio::task::JoinHandle<()> {
        crate::bot::playback::play_song_file(file_path, ip, port, streaming_info, self.play_state.clone()).await
    }

    /// 处理歌单播放
    ///
    /// 一次 join_voice，FFmpeg 从 stdin 读取逐首喂入的 MP3 数据。
    /// 支持停止和切歌。
    ///
    /// **性能**: 卡片更新通过 mpsc channel 异步完成（不阻塞音频推流）；
    /// 预下载使用 Notify 通知机制，下首歌在上首播放期间后台完成。
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

        let _ = ctx.api_client.send_channel_message(channel_id, &msg).await;

        let voice_channel = {
            match ctx.api_client.get_user_voice_channel(guild_id, user_id).await {
                Ok(ch) => ch,
                Err(e) => {
                    return CommandResult::Error(format!("获取语音频道失败: {}", e));
                }
            }
        };

        let vc = match voice_channel {
            Some(vc) => vc,
            None => {
                return CommandResult::Reply("⚠️ 你当前不在任何语音频道中".to_string());
            }
        };

        let vc_id = vc.id.clone();
        let playlist_name = playlist.name.clone();
        let total_count = playlist.track_ids.len();
        let track_ids: Vec<u64> = playlist.track_ids.clone();

        drop(client);

        // ── 一次性加入语音频道 ──
        let (gateway_ip, gateway_port, streaming_info) = match crate::bot::streaming::join_voice_for_streaming(ctx, &vc.id, channel_id).await {
            Some(info) => info,
            None => return CommandResult::Error("加入语音频道失败".to_string()),
        };
        self.play_state.reset_stats();

        // ── 启动非阻塞卡片更新器 ──
        let card_tx = crate::bot::playback::spawn_card_updater(
            ctx.api_client.clone(),
            self.play_state.clone(),
        );

        // ── 后台任务 ──
        let requester_name = ctx.data.extra.author.nickname.clone();
        let qqmusic_client = self.qqmusic_client.clone();
        let api_client = ctx.api_client.clone();
        let channel_id = channel_id.clone();
        let play_state = self.play_state.clone();

        tokio::spawn(async move {
            let api_cleanup = api_client.clone();
            let ps_cleanup = play_state.clone();
            let ch_cleanup = channel_id.clone();
            let vc_cleanup = vc_id.clone();

            let rt_outer = tokio::runtime::Handle::current();
            let result = tokio::task::spawn_blocking(move || {
                use crate::audio::{FFmpegDirectStreamer, StreamerConfig};
                use crate::bot::playback::{PreDownloadSlot, PreDownloadedSong, music_to_play_music, music_to_queue_music};

                let rt = tokio::runtime::Handle::current();
                let mut idx: usize = 0;

                // 预下载槽位：上一首歌播放期间后台下载下一首
                let next_slot = Arc::new(PreDownloadSlot::new());

                let mut streamer = match FFmpegDirectStreamer::new(
                    StreamerConfig::from(&streaming_info), play_state.clone()
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("创建流处理器失败: {}", e);
                        return Err(format!("创建流处理器失败: {}", e));
                    }
                };

                let mut stdin = match streamer.start_stream_stdin(
                    &gateway_ip, gateway_port, streaming_info.rtcp_port
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("启动 stdin pipe 失败: {}", e);
                        return Err(format!("启动推流失败: {}", e));
                    }
                };

                // 辅助函数：同步下载一首歌（含详情，用于首歌曲）
                let download_with_detail = |tid: u64| -> Option<PreDownloadedSong> {
                    rt.block_on(async {
                        let client = qqmusic_client.read().await;
                        let url = client.get_song_url(tid).await.ok().flatten()?;
                        let song = client.get_song_detail(tid).await.ok()?;
                        let music = client.to_music(&song);
                        let file_path = client.download_song(&url, tid).await.ok()?;
                        Some(PreDownloadedSong { file_path, music })
                    })
                };

                // 后台预下载（不阻塞）
                let spawn_pre_download = |slot: Arc<PreDownloadSlot>, tid: u64, rt_outer: &tokio::runtime::Handle| {
                    let qc = qqmusic_client.clone();
                    rt_outer.spawn(async move {
                        let client = qc.read().await;
                        let result = if let Some(url) = client.get_song_url(tid).await.ok().flatten() {
                            if let Ok(song) = client.get_song_detail(tid).await {
                                let music = client.to_music(&song);
                                client.download_song(&url, tid).await.ok()
                                    .map(|file_path| PreDownloadedSong { file_path, music })
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        slot.store(result);
                    });
                };

                // 发送卡片更新（非阻塞 channel send）
                let send_card = |song: &PreDownloadedSong, remaining: usize, next_songs: &[PreDownloadedSong]| {
                    let queue: Vec<_> = next_songs.iter().map(|s| music_to_queue_music(&s.music)).collect();
                    let _ = card_tx.send(crate::bot::playback::CardUpdateRequest {
                        current: music_to_play_music(&song.music, &requester_name),
                        queue,
                        queue_total: remaining,
                        channel_id: channel_id.clone(),
                        duration_secs: song.music.duration.unwrap_or(0),
                    });
                };

                while idx < track_ids.len() {
                    if play_state.is_stop_requested() {
                        info!("收到停止请求，终止播放");
                        break;
                    }

                    // ── 1. 获取当前歌曲（等待预下载或同步下载） ──
                    let current_song: PreDownloadedSong = if idx == 0 {
                        match download_with_detail(track_ids[0]) {
                            Some(s) => s,
                            None => {
                                warn!("[{}/{}] 首歌曲下载失败", idx + 1, total_count);
                                break;
                            }
                        }
                    } else {
                        match next_slot.take_blocking(&rt) {
                            Some(s) => s,
                            None => {
                                warn!("[{}/{}] 预下载失败，跳过", idx + 1, total_count);
                                idx += 1;
                                continue;
                            }
                        }
                    };

                    // ── 2. 后台预下载下一首（在当前歌曲播放期间进行） ──
                    if idx + 1 < track_ids.len() {
                        spawn_pre_download(next_slot.clone(), track_ids[idx + 1], &rt_outer);
                    }

                    // ── 3. 发送卡片更新（非阻塞！） ──
                    let remaining = total_count.saturating_sub(idx + 1);
                    send_card(&current_song, remaining, &[]);

                    // ── 4. 喂入 stdin 播放 ──
                    info!("[{}/{}] 正在播放: {}", idx + 1, total_count, current_song.file_path);
                    match std::fs::File::open(&current_song.file_path) {
                        Ok(mut f) => {
                            crate::audio::skip_id3_tag(&mut f);
                            crate::audio::feed_file_to_stdin(
                                &mut f,
                                &mut stdin,
                                &play_state,
                                &format!("{}/{}", idx + 1, total_count),
                            );
                        }
                        Err(e) => {
                            error!("打开文件失败: {}: {}", current_song.file_path, e);
                        }
                    }
                    idx += 1;
                }

                drop(stdin);
                let _ = streamer.wait();
                play_state.set_stopped();
                Ok(())
            }).await;

            // ── 清理 ──
            let playback_err = match &result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e.clone()),
                Err(e) => Some(format!("播放线程异常: {}", e)),
            };

            if let Some(old) = ps_cleanup.take_play_msg_id() {
                let _ = api_cleanup.delete_message(&old).await;
            }
            if !ps_cleanup.is_stop_requested() {
                if let Some(err) = &playback_err {
                    let _ = api_cleanup.send_channel_message(&ch_cleanup, &format!("❌ 播放出错: {}", err)).await;
                } else {
                    let _ = api_cleanup.send_channel_message(&ch_cleanup,
                        &format!("✅ QQ歌单 **{}** 播放完成", playlist_name)).await;
                }
            }
            let _ = api_cleanup.leave_voice_channel(&vc_cleanup).await;

            ps_cleanup.reset_stats();
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
            match ctx.api_client.get_user_voice_channel(guild_id, user_id).await {
                Ok(ch) => ch,
                Err(e) => {
                    return CommandResult::Error(format!(
                        "获取语音频道信息失败: {}",
                        e
                    ));
                }
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
                            ctx.api_client.send_card_message(channel_id, &card_json).await
                        {
                            self.play_state.set_play_msg_id(msg_id);
                        }
                    

                        // 加入语音频道并播放
                        if let Some((ip, port, streaming_info)) = crate::bot::streaming::join_voice_for_streaming(ctx, &vc.id, channel_id)
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

                                
                                if let Some(msg_id) = play_state.take_play_msg_id() {
                                    let _ = api_client.delete_message(&msg_id).await;
                                }
                                let _ = api_client.leave_voice_channel(&vc_id).await;
                            
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

    fn usage(&self) -> String {
        "!qqmusic <歌曲链接或关键词>".to_string()
    }

    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        if ctx.args.is_empty() {
            return CommandResult::Reply(
                "❌ 请提供歌曲链接或搜索关键词\n用法: `/qqmusic <歌曲链接或关键词>`".to_string(),
            );
        }
        if self.play_state.is_playing() {
            return CommandResult::Reply("⏸️ 正在播放中，请先停止当前播放".to_string());
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
