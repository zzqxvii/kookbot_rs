//! 音乐模块 - 网易云音乐播放功能
//! 
//! 这是 Bot 的一个内置功能模块，提供网易云音乐播放支持。
//! 包括以下命令：
//! - wyy: 播放网易云音乐（支持搜索、歌曲链接、歌单链接）
//! - wyylogin: 网易云账号登录（获取完整音质）
//! 
//! 这是一个模块化的命令实现示例，展示了如何使用 CommandHandler trait
//! 创建自己的功能模块。

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::common::play_state::PlayState;
use crate::core::config::BotConfig;
use crate::music::{NeteaseClient, QQMusicClient, BilibiliClient};
use crate::player::VoiceStreamingInfo;
use crate::audio::{FFmpegDirectStreamer, StreamerConfig};

/// 帮助命令
pub struct HelpCommand;

#[async_trait]
impl CommandHandler for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }
    
    fn aliases(&self) -> Vec<&'static str> {
        vec!["h"]
    }
    
    fn description(&self) -> &'static str {
        "显示帮助信息"
    }
    
    fn usage(&self) -> &'static str {
        "!help"
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        let content = r#"🎵 **Kook Music Bot** 🎵

**可用命令：**
`{}help` - 显示此帮助
`{}join` - 加入你的语音频道
`{}leave` - 离开语音频道
`{}wyy <链接或关键词>` - 播放网易云音乐
`{}wyylogin` - 登录网易云账号（获取完整音质）

**支持：**
- 网易云音乐链接/分享链接
- 歌曲ID
- 歌曲名称搜索
"#;
        
        let content = content.replace("{}", &ctx.config.prefix);
        CommandResult::Reply(content)
    }
}

/// 加入语音频道命令
pub struct JoinCommand;

#[async_trait]
impl CommandHandler for JoinCommand {
    fn name(&self) -> &'static str {
        "join"
    }
    
    fn aliases(&self) -> Vec<&'static str> {
        vec!["j"]
    }
    
    fn description(&self) -> &'static str {
        "加入你的语音频道"
    }
    
    fn usage(&self) -> &'static str {
        "!join"
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        let guild_id = &ctx.data.extra.guild_id;
        let user_id = &ctx.data.author_id;
        
        let voice_channel = {
            if let Some(client) = ctx.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        return CommandResult::Error(format!("获取语音频道信息失败: {}", e));
                    }
                }
            } else {
                return CommandResult::Error("API 客户端不可用".to_string());
            }
        };
        
        match voice_channel {
            Some(vc) => {
                info!("用户 {} 在语音频道: {} ({})", user_id, vc.name, vc.id);
                
                if let Some(client) = ctx.api_client.read().await.as_ref() {
                    match client.join_voice_channel(&vc.id).await {
                        Ok(conn_info) => {
                            info!("成功加入语音频道: {}:{}", conn_info.ip(), conn_info.port());
                            CommandResult::Reply(format!("✅ 已加入语音频道 **{}**", vc.name))
                        }
                        Err(e) => {
                            error!("加入语音频道失败: {}", e);
                            CommandResult::Error(format!("加入语音频道失败: {}", e))
                        }
                    }
                } else {
                    CommandResult::Error("API 客户端不可用".to_string())
                }
            }
            None => {
                CommandResult::Reply("⚠️ 你当前不在任何语音频道中\n请先加入一个语音频道".to_string())
            }
        }
    }
}

/// 离开语音频道命令
pub struct LeaveCommand;

#[async_trait]
impl CommandHandler for LeaveCommand {
    fn name(&self) -> &'static str {
        "leave"
    }
    
    fn aliases(&self) -> Vec<&'static str> {
        vec!["l"]
    }
    
    fn description(&self) -> &'static str {
        "离开语音频道"
    }
    
    fn usage(&self) -> &'static str {
        "!leave"
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        let mut vm = ctx.voice_manager.lock().await;
        if let Some(ref mut voice_manager) = *vm {
            match voice_manager.leave_channel().await {
                Ok(_) => CommandResult::Reply("✅ 已离开语音频道".to_string()),
                Err(e) => CommandResult::Error(format!("离开语音频道失败: {}", e)),
            }
        } else {
            CommandResult::Reply("⚠️ 当前不在任何语音频道中".to_string())
        }
    }
}

/// 网易云音乐播放命令
pub struct WyyCommand {
    netease_client: Arc<RwLock<NeteaseClient>>,
    play_state: Arc<PlayState>,
    cache_dir: String,
    max_cache_size_mb: u64,
}

impl WyyCommand {

    pub fn new(netease_client: Arc<RwLock<NeteaseClient>>, play_state: Arc<PlayState>, cache_dir: String, max_cache_size_mb: u64) -> Self {
        Self { netease_client, play_state, cache_dir, max_cache_size_mb }
    }
    
    /// 加入语音频道并返回流信息
    async fn join_voice_for_streaming(
        &self,
        ctx: &CommandContext<'_>,
        channel_id: &str,
        text_channel: &str,
    ) -> Option<(String, u16, VoiceStreamingInfo)> {
        if let Some(api_client) = ctx.api_client.read().await.as_ref() {
            // 先离开频道，确保获取新的推流地址
            // let _ = api_client.leave_voice_channel(channel_id).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // 重新加入获取新的推流地址
            let conn_info = match api_client.join_voice_channel(channel_id).await {
                Ok(info) => info,
                Err(e) => {
                    warn!("加入语音失败: {}", e);
                    let _ = api_client.send_channel_message(text_channel,
                        &format!("❌ 加入语音频道失败: {}", e)).await;
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
                rtcp_port: conn_info.rtcp_port.map(|p| p as u16).unwrap_or(port as u16 + 1),
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

    /// 播放歌曲文件（在后台线程中运行，返回 JoinHandle 供调用者等待）
    async fn play_song(
        &self,
        file_path: String,
        ip: String,
        port: u16,
        streaming_info: VoiceStreamingInfo,
    ) -> tokio::task::JoinHandle<()> {
        let play_state = self.play_state.clone();
        tokio::task::spawn_blocking(move || {
            let mut streamer = match FFmpegDirectStreamer::new(StreamerConfig::from(&streaming_info), play_state.clone()) {
                Ok(s) => s,
                Err(e) => {
                    error!("创建流处理器失败: {}", e);
                    return;
                }
            };

            match streamer.start_stream_url(&file_path, &ip, port, streaming_info.rtcp_port) {
                Ok(_) => {
                    let _ = streamer.wait();
                    play_state.set_stopped();
                    info!("🎵 歌曲播放完成");
                }
                Err(e) => {
                    error!("推流失败: {}", e);
                    play_state.set_stopped();
                }
            }
        })
    }
    
    /// 处理歌单播放 (stdin pipe 模式)
    ///
    /// 一次 join_voice，FFmpeg 从 stdin 读取逐首喂入的 MP3 数据。
    /// 支持停止和切歌：切歌时杀 FFmpeg → rejoin → 重启管道继续剩余歌曲。
    async fn handle_playlist(
        &self,
        ctx: &CommandContext<'_>,
        playlist_id: u64,
    ) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        let guild_id = &ctx.data.extra.guild_id;
        let user_id = &ctx.data.author_id;

        // ── 获取歌单元数据 ──
        let (playlist_name, track_ids, total_count) = {
            let netease = self.netease_client.read().await;
            let playlist = match netease.get_playlist_detail(playlist_id).await {
                Ok(p) => p,
                Err(e) => return CommandResult::Error(format!("获取歌单失败: {}", e)),
            };
            if playlist.track_ids.is_empty() {
                return CommandResult::Reply("❌ 歌单为空".to_string());
            }
            (playlist.name.clone(), playlist.track_ids.clone(), playlist.track_ids.len())
        };



        // 发送歌单信息
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            let msg = format!("📋 **歌单：{}**\n共 {} 首，stdin pipe 模式...", playlist_name, total_count);
            let _ = client.send_channel_message(channel_id, &msg).await;
        }

        // 获取用户语音频道
        let voice_channel = {
            if let Some(client) = ctx.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => return CommandResult::Error(format!("获取语音频道失败: {}", e)),
                }
            } else {
                return CommandResult::Error("API 客户端不可用".to_string());
            }
        };
        let vc = match voice_channel {
            Some(vc) => vc,
            None => return CommandResult::Reply("⚠️ 你当前不在任何语音频道中".to_string()),
        };

        // ── 一次性加入语音频道 ──
        let (gateway_ip, gateway_port, streaming_info) = match self.join_voice_for_streaming(ctx, &vc.id, channel_id).await {
            Some(info) => info,
            None => return CommandResult::Error("加入语音频道失败".to_string()),
        };
        let vc_id = vc.id.clone();
        self.play_state.reset_stats();

        // ── 后台任务 ──
        let netease_client = self.netease_client.clone();
        let api_client = ctx.api_client.clone();
        let channel_id = channel_id.clone();
        let play_state = self.play_state.clone();

        tokio::spawn(async move {
            let api_cleanup = api_client.clone();
            let ps_cleanup = play_state.clone();
            let ch_cleanup = channel_id.clone();
            let vc_cleanup = vc_id.clone();

            // ── 预取歌曲详情（后台执行，不阻塞命令响应） ──
            let all_song_details: Vec<(String, String, String)> = {
                let netease = netease_client.read().await;
                let mut details = Vec::with_capacity(track_ids.len().min(50));
                for &tid in &track_ids {
                    if let Ok(song) = netease.get_song_detail(tid).await {
                        let author = song.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
                        let pic_url = song.album.pic_url.clone();
                        details.push((song.name.clone(), author, pic_url));
                    } else {
                        details.push(("未知".into(), "未知".into(), String::new()));
                    }
                }
                details
            };

            let result = tokio::task::spawn_blocking(move || {
                use crate::audio::{FFmpegDirectStreamer, StreamerConfig};

                let rt = tokio::runtime::Handle::current();
                let mut idx: usize = 0;
                let cur_ip = gateway_ip.clone();
                let cur_port = gateway_port;
                let cur_info = streaming_info.clone();

                'playback: loop {
                    if play_state.is_stop_requested() || idx >= track_ids.len() {
                        break;
                    }

                    let mut streamer = match FFmpegDirectStreamer::new(
                        StreamerConfig::from(&cur_info), play_state.clone()
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("创建流处理器失败: {}", e);
                            return Err(format!("创建流处理器失败: {}", e));
                        }
                    };

                    let mut stdin = match streamer.start_stream_stdin(
                        &cur_ip, cur_port, cur_info.rtcp_port
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("启动 stdin pipe 失败: {}", e);
                            return Err(format!("启动推流失败: {}", e));
                        }
                    };

                    let mut pipe_ok = true;
                    while idx < track_ids.len() {
                        if play_state.is_stop_requested() {
                            info!("收到停止请求，终止播放");
                            break 'playback;
                        }

                        let tid = track_ids[idx];
                        info!("[{}/{}] 下载中...", idx + 1, total_count);

                        let file_path = match rt.block_on(async {
                            let netease = netease_client.read().await;
                            let url = netease.get_song_url(tid).await.ok().flatten()?;
                            netease.download_song(&url, tid).await.ok()
                        }) {
                            Some(p) => p,
                            None => {
                                warn!("[{}/{}] 下载失败，跳过", idx + 1, total_count);
                                idx += 1;
                                continue;
                            }
                        };

                        // 更新卡片
                        rt.block_on(async {
                            if let Some(client) = api_client.read().await.as_ref() {
                                let netease = netease_client.read().await;
                                if let Ok(song) = netease.get_song_detail(tid).await {
                                    let music = netease.to_music(&song);
                                    play_state.set_current_song_duration(music.duration.unwrap_or(0));
                                    use crate::common::card::{build_play_card, PlayCardData, PlayMusic, QueueMusic, Sender as CardSender};
                                    let mut data = PlayCardData::new(PlayMusic {
                                        title: music.title,
                                        author: music.author,
                                        platform: music.platform,
                                        pic_url: music.pic_url,
                                        sender: CardSender {
                                            nick_name: format!("{}/{}", idx + 1, total_count),
                                            avatar_url: None,
                                        },
                                    });
                                    let queue: Vec<QueueMusic> = all_song_details[idx+1..].iter()
                                        .map(|(title, author, pic_url)| QueueMusic {
                                            title: title.clone(),
                                            author: author.clone(),
                                            platform: "网易云".to_string(),
                                            pic_url: pic_url.clone(),
                                            sender: CardSender { nick_name: "".to_string(), avatar_url: None },
                                        })
                                        .collect();
                                    data = data.with_queue(queue, total_count.saturating_sub(idx + 1));
                                    let json = build_play_card(&data);
                                    if let Some(old) = play_state.take_play_msg_id() {
                                        let _ = client.delete_message(&old).await;
                                    }
                                    if let Ok(msg_id) = client.send_card_message(&channel_id, &json).await {
                                        play_state.set_play_msg_id(msg_id);
                                    }
                                }
                            }
                        });

                        // 喂入 stdin (剥离 ID3v2)
                        info!("[{}/{}] 正在播放: {}", idx + 1, total_count, file_path);
                        match std::fs::File::open(&file_path) {
                            Ok(mut f) => {
                                let mut hdr = [0u8; 10];
                                if std::io::Read::read_exact(&mut f, &mut hdr).is_ok()
                                    && &hdr[0..3] == b"ID3"
                                {
                                    let skip = 10
                                        + ((hdr[6] as u64 & 0x7F) << 21)
                                        + ((hdr[7] as u64 & 0x7F) << 14)
                                        + ((hdr[8] as u64 & 0x7F) << 7)
                                        + (hdr[9] as u64 & 0x7F);
                                    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(skip)).ok();
                                } else {
                                    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0)).ok();
                                }
                                if let Err(e) = std::io::copy(&mut f, &mut stdin) {
                                    error!("喂入 stdin 失败: {}", e);
                                    pipe_ok = false;
                                    if play_state.is_next_requested() {
                                        play_state.clear_next_request();
                                        info!("切歌 → 下一首");
                                        idx += 1;
                                        let _ = streamer.wait();
                                        break;
                                    }
                                    break 'playback;
                                }
                            }
                            Err(e) => {
                                error!("打开文件失败: {}: {}", file_path, e);
                                idx += 1;
                                continue;
                            }
                        }
                        idx += 1;
                    }

                    if pipe_ok {
                        drop(stdin);
                    }
                    let _ = streamer.wait();

                    // 无需重新加入语音频道，stdin pipe 模式下连接复用
                }

                play_state.set_stopped();
                Ok(())
            }).await;

            // ── 清理 ──
            let playback_err = match &result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e.clone()),
                Err(e) => Some(format!("播放线程异常: {}", e)),
            };
            if let Some(client) = api_cleanup.read().await.as_ref() {
                if let Some(old) = ps_cleanup.take_play_msg_id() {
                    let _ = client.delete_message(&old).await;
                }
                if !ps_cleanup.is_stop_requested() {
                    if let Some(err) = &playback_err {
                        let _ = client.send_channel_message(&ch_cleanup, &format!("❌ 播放出错: {}", err)).await;
                    } else {
                        let _ = client.send_channel_message(&ch_cleanup,
                            &format!("✅ 歌单 **{}** 播放完成", playlist_name)).await;
                    }
                }
                let _ = client.leave_voice_channel(&vc_cleanup).await;
            }
            ps_cleanup.reset_stats();
            info!("歌单播放完成");
        });

        CommandResult::Reply(format!("✅ 歌单开始播放，共 {} 首 (stdin pipe)", total_count))
    }
    
    /// 处理单曲播放
    async fn handle_single(
        &self,
        ctx: &CommandContext<'_>,
        query: &str,
    ) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        let guild_id = &ctx.data.extra.guild_id;
        let user_id = &ctx.data.author_id;
        
        // 获取用户语音频道
        let voice_channel = {
            if let Some(client) = ctx.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
                    Ok(ch) => ch,
                    Err(e) => {
                        return CommandResult::Error(format!("获取语音频道信息失败: {}", e));
                    }
                }
            } else {
                return CommandResult::Error("API 客户端不可用".to_string());
            }
        };
        
        let vc = match voice_channel {
            Some(vc) => vc,
            None => {
                return CommandResult::Reply("⚠️ 你当前不在任何语音频道中\n请先加入一个语音频道".to_string());
            }
        };

        // 保存语音频道ID，用于播放结束后退出
        let vc_id_for_leave = vc.id.clone();

        let netease = self.netease_client.read().await;
        match netease.get_or_search(query).await {
            Ok((song, url)) => {
                let music = netease.to_music(&song);
                
                if netease.has_cookie() {
                    info!("✅ 使用已登录的网易云账号");
                } else {
                    info!("⚠️ 未登录网易云账号，可能只能播放试听版本");
                }
                
                match url {
                    Some(audio_url) => {
                        info!("获取到歌曲URL: {}", audio_url);
                        
                        let local_file = match netease.download_song(&audio_url, song.id).await {
                            Ok(path) => {
                                info!("歌曲下载成功: {}", path);
                                path
                            }
                            Err(e) => {
                                error!("下载歌曲失败: {}", e);
                                return CommandResult::Error(format!("下载歌曲失败: {}", e));
                            }
                        };
                        
                        // 下载后清理缓存
                        crate::common::cache::cleanup_cache(&self.cache_dir, self.max_cache_size_mb).await;
                        
                        drop(netease); // 释放锁
                        
                        // 发送播放卡片
                        if let Some(client) = ctx.api_client.read().await.as_ref() {
                            use crate::common::card::{build_play_card, PlayCardData, PlayMusic, Sender as CardSender};
                            
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
                            if let Ok(msg_id) = client.send_card_message(channel_id, &card_json).await {
                                self.play_state.set_play_msg_id(msg_id);
                            }
                        }
                        
                        // 记录歌曲时长，用于进度显示
                        self.play_state.set_current_song_duration(music.duration.unwrap_or(0));
                        
                        // 加入语音频道并播放
                        if let Some((ip, port, streaming_info)) = self.join_voice_for_streaming(ctx, &vc.id, channel_id).await {
                            let handle = self.play_song(local_file, ip, port, streaming_info).await;
                            
                            let api_client = ctx.api_client.clone();
                            let vc_id = vc_id_for_leave.clone();
                            let play_state = self.play_state.clone();
                            
                            tokio::spawn(async move {
                                let _ = handle.await;
                                info!("单曲播放完成");
                                
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
                    None => {
                        CommandResult::Reply(format!(
                            "❌ 无法获取 **{}** 的播放链接\n可能需要 VIP 或歌曲已下架",
                            song.name
                        ))
                    }
                }
            }
            Err(e) => {
                CommandResult::Error(format!("搜索失败: {}", e))
            }
        }
    }
}

#[async_trait]
impl CommandHandler for WyyCommand {
    fn name(&self) -> &'static str {
        "wyy"
    }
    
    fn description(&self) -> &'static str {
        "播放网易云音乐"
    }
    
    fn usage(&self) -> &'static str {
        "!wyy <歌曲链接或关键词>"
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        if ctx.args.is_empty() {
            return CommandResult::Reply(
                "❌ 请提供歌曲链接或搜索关键词\n用法: `/wyy <歌曲链接或关键词>`".to_string()
            );
        }
        
        let query = ctx.args.join(" ");
        info!("处理 /wyy 命令: {}", query);
        
        // 检查是否是歌单链接
        if let Some(playlist_id) = NeteaseClient::parse_playlist_id(&query) {
            info!("检测到歌单链接，ID: {}", playlist_id);
            return self.handle_playlist(&ctx, playlist_id).await;
        }
        
        // 单曲处理
        self.handle_single(&ctx, &query).await
    }
}

/// 网易云登录命令
pub struct WyyLoginCommand {
    netease_client: Arc<RwLock<NeteaseClient>>,
    config: BotConfig,
}

impl WyyLoginCommand {
    pub fn new(netease_client: Arc<RwLock<NeteaseClient>>, config: BotConfig) -> Self {
        Self { netease_client, config }
    }
    
    /// 生成二维码图片并上传到 Kook
    async fn generate_and_upload_qrcode(&self, ctx: &CommandContext<'_>, url: &str) -> Option<String> {
        use qrcode::QrCode;
        use image::Luma;

        let code = match QrCode::new(url) {
            Ok(c) => c,
            Err(e) => {
                warn!("生成二维码失败: {}", e);
                return None;
            }
        };

        let image = code.render::<Luma<u8>>().build();
        let mut buffer = std::io::Cursor::new(Vec::new());

        if let Err(e) = image.write_to(&mut buffer, image::ImageFormat::Png) {
            warn!("编码二维码图片失败: {}", e);
            return None;
        }

        let image_data = buffer.into_inner();

        if let Some(client) = ctx.api_client.read().await.as_ref() {
            match client.upload_image(&image_data).await {
                Ok(kook_url) => {
                    info!("二维码上传成功: {}", kook_url);
                    return Some(kook_url);
                }
                Err(e) => {
                    warn!("上传二维码图片失败: {}", e);
                }
            }
        }

        None
    }
}

#[async_trait]
impl CommandHandler for WyyLoginCommand {
    fn name(&self) -> &'static str {
        "wyylogin"
    }
    
    fn description(&self) -> &'static str {
        "登录网易云账号（获取完整音质）"
    }
    
    fn usage(&self) -> &'static str {
        "!wyylogin"
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        
        // 发送初始化消息
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(
                channel_id,
                "🔑 正在生成网易云登录二维码..."
            ).await;
        }
        
        let netease = self.netease_client.read().await;
        
        // 获取二维码 key
        let key = match netease.get_qr_key().await {
            Ok(key_data) => key_data.unikey,
            Err(e) => {
                return CommandResult::Error(format!("获取二维码失败: {}", e));
            }
        };
        
        // 生成二维码
        let qr_code = match netease.create_qr_code(&key).await {
            Ok(qr) => qr,
            Err(e) => {
                return CommandResult::Error(format!("生成二维码失败: {}", e));
            }
        };
        
        drop(netease); // 释放锁
        
        // 生成二维码图片并上传
        info!("开始生成并上传二维码图片...");
        let image_url = self.generate_and_upload_qrcode(&ctx, &qr_code.qrurl).await;
        info!("二维码上传结果: {:?}", image_url);
        
        // 发送二维码
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            if let Some(ref url) = image_url {
                info!("发送图片消息: {}", url);
                let _ = client.send_image_message(channel_id, url).await;
                let _ = client.send_channel_message(channel_id,
                    "📱 **请扫描上方二维码登录网易云音乐**\n⏰ 二维码有效期 5 分钟").await;
            } else {
                warn!("二维码上传失败，发送链接");
                let _ = client.send_channel_message(channel_id,
                    &format!(
                        "📱 **网易云登录**\n\n点击链接扫码：{}\n\n⏰ 二维码有效期 5 分钟",
                        qr_code.qrurl
                    )).await;
            }
        }
        
        // 轮询检查登录状态
        let api_client = ctx.api_client.clone();
        let netease_api_url = self.config.music.netease_api_url.clone();
        let key_clone = key.clone();
        let channel_id_clone = channel_id.to_string();
        let config_path = std::path::PathBuf::from("config.toml");
        
        tokio::spawn(async move {
            let netease_client = NeteaseClient::new(&netease_api_url);
            let mut attempts = 0;
            let max_attempts = 60;
            let key_str = key_clone.clone();
            info!("启动登录检查任务，key: {}", key_str);
            
            loop {
                attempts += 1;
                info!("检查登录状态... ({}/{}), key: {}", attempts, max_attempts, key_str);
                
                if attempts > max_attempts {
                    if let Some(client) = api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(&channel_id_clone,
                            "⏰ 二维码已过期，请重新发送 `/wyylogin`").await;
                    }
                    break;
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                match netease_client.check_qr_status(&key_clone).await {
                    Ok(result) => {
                        info!("登录状态码: {}", result.code);
                        match result.code {
                            800 => {
                                if let Some(client) = api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(&channel_id_clone,
                                        "⏰ 二维码已过期，请重新发送 `/wyylogin`").await;
                                }
                                break;
                            }
                            801 => {
                                // 等待扫码
                                info!("等待扫码中...");
                            }
                            802 => {
                                info!("已扫码，等待确认");
                                if let Some(client) = api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(&channel_id_clone,
                                        "✅ 已扫描，请在手机上确认登录").await;
                                }
                            }
                            803 => {
                                info!("登录成功! cookie: {:?}", result.cookie);
                                if let Some(ref cookie) = result.cookie {
                                    use crate::common::utils::update_netease_cookie;
                                    match update_netease_cookie(&config_path, cookie) {
                                        Ok(_) => {
                                            if let Some(client) = api_client.read().await.as_ref() {
                                                let nickname = result.nickname.as_deref().unwrap_or("用户");
                                                let _ = client.send_channel_message(&channel_id_clone,
                                                    &format!("🎉 登录成功！欢迎 **{}**\nCookie 已保存，请重启机器人后使用 `/wyy` 播放完整音质", nickname)).await;
                                            }
                                        }
                                        Err(e) => {
                                            error!("保存 cookie 失败: {}", e);
                                            if let Some(client) = api_client.read().await.as_ref() {
                                                let _ = client.send_channel_message(&channel_id_clone,
                                                    &format!("⚠️ 登录成功，但保存 Cookie 失败: {}", e)).await;
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                            _ => {
                                warn!("未知的登录状态码: {}", result.code);
                            }
                        }
                    }
                    Err(e) => {
                        error!("检查登录状态失败: {}", e);
                    }
                }
            }
        });
        
        CommandResult::Ok
    }
}

/// Bot 状态命令
pub struct BotStatusCommand {
    play_state: Arc<PlayState>,
    cache_dir: String,
}

impl BotStatusCommand {
    pub fn new(play_state: Arc<PlayState>, cache_dir: String) -> Self {
        Self { play_state, cache_dir }
    }
}

#[async_trait]
impl CommandHandler for BotStatusCommand {
    fn name(&self) -> &'static str {
        "状态"
    }
    
    fn aliases(&self) -> Vec<&'static str> {
        vec!["status", "zt", "bot"]
    }
    
    fn description(&self) -> &'static str {
        "查看 Bot 当前播放状态和统计信息"
    }
    
    fn usage(&self) -> &'static str {
        "/状态"
    }
    
    async fn execute(&self, _ctx: CommandContext<'_>) -> CommandResult {
        let is_playing = self.play_state.is_playing();
        let play_count = self.play_state.get_play_count();
        let play_duration = self.play_state.get_play_duration();
        let cache_size = crate::common::cache::get_cache_size_mb(&self.cache_dir);
        
        let duration_str = crate::common::utils::format_duration(play_duration);
        let cache_str = crate::common::utils::format_bytes(cache_size * 1024 * 1024);
        
        let status = if is_playing {
            let progress = self.play_state.progress_bar()
                .map(|p| format!("\n⏳ {}\n", p))
                .unwrap_or_default();
            format!(
                "🎵 **Bot 运行状态**\n\n\
                 ▶️ **正在播放**{}\
                 📊 本次已播放: {} 首\n\
                 ⏱️  播放时长: {}\n\
                 💾 缓存占用: {}\n\
                 \n---\n\
                 使用 `/wyy 歌名` 点歌",
                progress, play_count, duration_str, cache_str
            )
        } else {
            format!(
                "🎵 **Bot 运行状态**\n\n\
                 ⏸️ **当前空闲**\n\
                 📊 本次已播放: {} 首\n\
                 💾 缓存占用: {}\n\
                 \n---\n\
                 使用 `/wyy 歌名` 开始点歌",
                play_count, cache_str
            )
        };
        
        CommandResult::Reply(status)
    }
}


/// 跨平台统一搜索命令
pub struct UnifiedSearchCommand {
    netease_client: Arc<RwLock<NeteaseClient>>,
    qqmusic_client: Arc<RwLock<QQMusicClient>>,
    bilibili_client: Arc<RwLock<BilibiliClient>>,
}

impl UnifiedSearchCommand {
    pub fn new(
        netease_client: Arc<RwLock<NeteaseClient>>,
        qqmusic_client: Arc<RwLock<QQMusicClient>>,
        bilibili_client: Arc<RwLock<BilibiliClient>>,
    ) -> Self {
        Self { netease_client, qqmusic_client, bilibili_client }
    }
}

#[async_trait]
impl CommandHandler for UnifiedSearchCommand {
    fn name(&self) -> &'static str {
        "搜索"
    }

    fn aliases(&self) -> Vec<&'static str> {
        vec!["search", "搜", "s"]
    }

    fn description(&self) -> &'static str {
        "跨平台搜索歌曲（网易云 + QQ音乐 + B站）"
    }

    fn usage(&self) -> &'static str {
        "/搜索 <关键词>"
    }

    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        if ctx.args.is_empty() {
            return CommandResult::Reply("❌ 请提供搜索关键词\n用法: `/搜索 <关键词>`".to_string());
        }

        let keyword = ctx.args.join(" ");
        info!("跨平台搜索: {}", keyword);

        // 并行搜索三个平台
        let (netease_results, qqmusic_results, bilibili_results) = tokio::join!(
            async {
                let client = self.netease_client.read().await;
                client.search(&keyword, 5).await.unwrap_or_default()
            },
            async {
                let client = self.qqmusic_client.read().await;
                client.search(&keyword, 5).await.unwrap_or_default()
            },
            async {
                let client = self.bilibili_client.read().await;
                client.search(&keyword, 5).await.unwrap_or_default()
            }
        );

        if netease_results.is_empty() && qqmusic_results.is_empty() && bilibili_results.is_empty() {
            return CommandResult::Reply(format!("🔍 未找到与 **{}** 相关的歌曲", keyword));
        }

        let mut lines = vec![format!("🔍 **{}** 的搜索结果：", keyword), String::new()];

        if !netease_results.is_empty() {
            lines.push("**🎵 网易云音乐**".to_string());
            for (i, song) in netease_results.iter().take(5).enumerate() {
                lines.push(format!(
                    "  {}. **{}** - {}  |  `/wyy {}`",
                    i + 1,
                    truncate(&song.name, 25),
                    song.artists.first().map(|a| a.name.as_str()).unwrap_or("未知"),
                    song.id
                ));
            }
            lines.push(String::new());
        }

        if !qqmusic_results.is_empty() {
            lines.push("**🎶 QQ音乐**".to_string());
            for (i, song) in qqmusic_results.iter().take(5).enumerate() {
                lines.push(format!(
                    "  {}. **{}** - {}  |  `/qqmusic {}`",
                    i + 1,
                    truncate(&song.name, 25),
                    song.artists.first().map(|a| a.name.as_str()).unwrap_or("未知"),
                    song.id
                ));
            }
        }

        if !bilibili_results.is_empty() {
            lines.push(String::new());
            lines.push("**📺 B站**".to_string());
            for (i, song) in bilibili_results.iter().take(5).enumerate() {
                lines.push(format!(
                    "  {}. **{}** - {}  |  `/bilibili {}`",
                    i + 1,
                    truncate(&song.name, 25),
                    song.author.name,
                    song.bvid
                ));
            }
        }

        CommandResult::Reply(lines.join("\n"))
    }
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > max {
        format!("{}...", chars.iter().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

/// 歌词查询命令
pub struct LyricCommand {
    netease_client: Arc<RwLock<NeteaseClient>>,
}

impl LyricCommand {
    pub fn new(netease_client: Arc<RwLock<NeteaseClient>>) -> Self {
        Self { netease_client }
    }
}

#[async_trait]
impl CommandHandler for LyricCommand {
    fn name(&self) -> &'static str {
        "歌词"
    }

    fn aliases(&self) -> Vec<&'static str> {
        vec!["lyric", "lrc", "gc"]
    }

    fn description(&self) -> &'static str {
        "查询歌曲歌词"
    }

    fn usage(&self) -> &'static str {
        "/歌词 <歌曲ID>"
    }

    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        if ctx.args.is_empty() {
            return CommandResult::Reply("❌ 请提供歌曲ID\n用法: `/歌词 <歌曲ID>`".to_string());
        }

        let song_id: u64 = match ctx.args[0].parse() {
            Ok(id) => id,
            Err(_) => return CommandResult::Reply("❌ 无效的歌曲ID".to_string()),
        };

        let client = self.netease_client.read().await;
        match client.get_lyric(song_id).await {
            Ok(Some(lyric)) => {
                // 取前20行
                let lines: Vec<&str> = lyric.lines().take(20).collect();
                if lines.is_empty() {
                    return CommandResult::Reply("📝 暂无歌词".to_string());
                }
                CommandResult::Reply(format!("📝 **歌词** (ID: {})\n\n{}", song_id, lines.join("\n")))
            }
            Ok(None) => CommandResult::Reply("📝 暂无歌词".to_string()),
            Err(e) => CommandResult::Error(format!("获取歌词失败: {}", e)),
        }
    }
}
/// 创建音乐模块的所有命令
pub fn create_music_commands(
    netease_client: Arc<RwLock<NeteaseClient>>,
    config: BotConfig,
    play_state: Arc<PlayState>,
) -> Vec<Arc<dyn CommandHandler>> {
    vec![
        Arc::new(BotStatusCommand::new(
            play_state.clone(),
            config.music.cache_dir.clone(),
        )),
        Arc::new(LyricCommand::new(netease_client.clone())),
        Arc::new(WyyCommand::new(
            netease_client.clone(), 
            play_state.clone(),
            config.music.cache_dir.clone(),
            config.music.max_cache_size_mb,
        )),
        Arc::new(WyyLoginCommand::new(netease_client, config)),
    ]
}
