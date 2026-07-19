//! QQ 音乐模块 - QQ 音乐播放功能
//!
//! 提供 QQ 音乐播放支持，包括以下命令：
//! - qqmusic: 播放 QQ 音乐（支持搜索、歌曲链接、歌单链接）

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

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
                use std::sync::Mutex;

                let rt = tokio::runtime::Handle::current();
                let mut idx: usize = 0;
                // 预下载：当前歌播放时后台下载下一首
                let next_file: Arc<Mutex<Option<Option<String>>>> = Arc::new(Mutex::new(None));

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

                while idx < track_ids.len() {
                    if play_state.is_stop_requested() {
                        info!("收到停止请求，终止播放");
                        break;
                    }

                    // 获取文件路径：优先用预下载
                    let file_path = {
                        let mut guard = next_file.lock().expect("next_file lock poisoned");
                        match guard.take() {
                            Some(Some(p)) => p,
                            Some(None) => {
                                warn!("[{}/{}] 预下载失败，跳过", idx + 1, total_count);
                                idx += 1;
                                continue;
                            }
                            None => {
                                drop(guard);
                                let tid = track_ids[idx];
                                debug!("[{}/{}] 下载中...", idx + 1, total_count);
                                match rt.block_on(async {
                                    let client = qqmusic_client.read().await;
                                    let url = client.get_song_url(tid).await.ok().flatten()?;
                                    client.download_song(&url, tid).await.ok()
                                }) {
                                    Some(p) => p,
                                    None => {
                                        warn!("[{}/{}] 下载失败，跳过", idx + 1, total_count);
                                        idx += 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    };

                    // 后台预下载下一首
                    if idx + 1 < track_ids.len() {
                        let nf = next_file.clone();
                        let qc = qqmusic_client.clone();
                        let next_tid = track_ids[idx + 1];
                        let rt_outer = rt_outer.clone();
                        rt_outer.spawn(async move {
                            let client = qc.read().await;
                            let url = match client.get_song_url(next_tid).await.ok().flatten() {
                                Some(u) => u,
                                None => return,
                            };
                            let result = client.download_song(&url, next_tid).await.ok();
                            if let Ok(mut g) = nf.lock() { *g = Some(result) }
                        });
                    }


                    let cur_tid = track_ids[idx];
                    // 更新卡片
                    rt.block_on(async {
                        
                        let qqmusic = qqmusic_client.read().await;
                        if let Ok(song) = qqmusic.get_song_detail(cur_tid).await {
                            let music = qqmusic.to_music(&song);
                            play_state.set_current_song_duration(music.duration.unwrap_or(0));
                            use crate::common::card::{build_play_card, PlayCardData, PlayMusic, QueueMusic, Sender as CardSender};
                            let mut data = PlayCardData::new(PlayMusic {
                                title: music.title,
                                author: music.author,
                                platform: music.platform,
                                pic_url: music.pic_url,
                                sender: CardSender {
                                    nick_name: requester_name.clone(),
                                    avatar_url: None,
                                },
                            });
                            let remaining = total_count.saturating_sub(idx + 1);
                            let mut queue = Vec::new();
                            if remaining > 0 {
                                for i in 1..=2.min(remaining) {
                                    let next_tid = track_ids[idx + i];
                                    if let Ok(next_song) = qqmusic.get_song_detail(next_tid).await {
                                        let qm = qqmusic.to_music(&next_song);
                                        queue.push(QueueMusic {
                                            title: qm.title,
                                            author: qm.author,
                                            platform: "QQ音乐".to_string(),
                                            pic_url: qm.pic_url,
                                            sender: CardSender { nick_name: "".to_string(), avatar_url: None },
                                        });
                                    }
                                }
                            }
                            data = data.with_queue(queue, remaining);
                            let json = build_play_card(&data);
                            if let Some(old) = play_state.take_play_msg_id() {
                                let _ = api_client.delete_message(&old).await;
                            }
                            if let Ok(msg_id) = api_client.send_card_message(&channel_id, &json).await {
                                play_state.set_play_msg_id(msg_id);
                            }
                        }
                    
                    });

                    // 分块喂入 stdin（ID3 跳过 + 切歌/停止检查内置于 feed_file_to_stdin）
                    info!("[{}/{}] 正在播放: {}", idx + 1, total_count, file_path);
                    match std::fs::File::open(&file_path) {
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
                            error!("打开文件失败: {}: {}", file_path, e);
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
