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
use crate::core::config::BotConfig;
use crate::music::NeteaseClient;
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
}

impl WyyCommand {
    pub fn new(netease_client: Arc<RwLock<NeteaseClient>>) -> Self {
        Self { netease_client }
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
            let _ = api_client.leave_voice_channel(channel_id).await;
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

    /// 播放歌曲文件（在后台线程中运行，不阻塞事件循环）
    async fn play_song(
        &self,
        file_path: String,
        ip: String,
        port: u16,
        streaming_info: VoiceStreamingInfo,
    ) {
        // 使用 spawn_blocking 在后台线程运行，不阻塞 tokio 运行时
        tokio::task::spawn_blocking(move || {
            let mut streamer = match FFmpegDirectStreamer::new(StreamerConfig::from(&streaming_info)) {
                Ok(s) => s,
                Err(e) => {
                    error!("创建流处理器失败: {}", e);
                    return;
                }
            };

            match streamer.start_stream_url(&file_path, &ip, port, streaming_info.rtcp_port) {
                Ok(_) => {
                    let _ = streamer.wait();
                    crate::common::play_state::set_stopped();
                    info!("🎵 歌曲播放完成");
                }
                Err(e) => {
                    error!("推流失败: {}", e);
                    crate::common::play_state::set_stopped();
                }
            }
        });
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
        
        let netease = self.netease_client.read().await;
        let playlist = match netease.get_playlist_detail(playlist_id).await {
            Ok(p) => p,
            Err(e) => {
                return CommandResult::Error(format!("获取歌单失败: {}", e));
            }
        };
        
        if playlist.track_ids.is_empty() {
            return CommandResult::Reply("❌ 歌单为空".to_string());
        }
        
        let msg = format!(
            "📋 **歌单：{}**\n共 {} 首歌曲，开始播放...",
            playlist.name,
            playlist.track_ids.len()
        );
        
        // 发送歌单信息
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(channel_id, &msg).await;
        }
        
        // 获取用户语音频道
        let voice_channel = {
            if let Some(client) = ctx.api_client.read().await.as_ref() {
                match client.get_user_voice_channel(guild_id, user_id).await {
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
        
        // 在后台任务中播放歌单，不阻塞事件循环
        let netease_client = self.netease_client.clone();
        let api_client = ctx.api_client.clone();
        let channel_id = channel_id.clone();
        let vc_id = vc.id.clone();
        let playlist_name = playlist.name.clone();
        let total_count = playlist.track_ids.len();
        let track_ids: Vec<u64> = playlist.track_ids.clone();
        
        // 重置播放统计
        crate::common::play_state::reset_stats();
        
        // 只获取前3首歌曲信息（用于第一张卡片显示）
        info!("获取前3首歌曲信息用于初始卡片...");
        let initial_songs: Vec<_> = match netease.get_songs_detail(&track_ids.iter().take(3).cloned().collect::<Vec<_>>()).await {
            Ok(songs) => songs,
            Err(_) => Vec::new(),
        };
        
        // 构建初始队列信息
        let initial_queue: Vec<(String, String, String)> = initial_songs.iter().skip(1)
            .map(|song| {
                let author = song.artists.iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let pic_url = song.album.pic_url.clone();
                (song.name.clone(), author, pic_url)
            })
            .collect();
        
        info!("获取完成，初始队列 {} 首（歌单共 {} 首）", initial_queue.len(), total_count);
        
        drop(netease);
        
        let initial_queue = Arc::new(tokio::sync::RwLock::new(initial_queue));
        let current_song_info = Arc::new(tokio::sync::RwLock::new(None::<(String, String, String)>));
        
        tokio::spawn(async move {
            info!("开始播放歌单: {}，共 {} 首", playlist_name, total_count);
            
            for (index, track_id) in track_ids.iter().enumerate() {
                // 检查停止请求
                if crate::common::play_state::is_stop_requested() {
                    info!("收到停止请求，终止歌单播放");
                    break;
                }
                
                let netease = netease_client.read().await;
                
                // 获取当前歌曲信息
                let song = match netease.get_song_detail(*track_id).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("获取歌曲 {} 详情失败: {}", track_id, e);
                        continue;
                    }
                };
                
                info!("当前播放: {}, 队列状态: {:?}", song.name, initial_queue.read().await);
                
                let music = netease.to_music(&song);
                info!("准备播放第 {} 首: {}", index + 1, song.name);
                
                // 获取下两首歌曲信息（用于更新队列）
                let mut next_songs_info: Vec<(String, String, String)> = Vec::new(); // (歌名, 歌手, 封面)
                for i in 1..=2 {
                    if index + i < track_ids.len() {
                        match netease.get_song_detail(track_ids[index + i]).await {
                            Ok(next_song) => {
                                let author = next_song.artists.iter()
                                    .map(|a| a.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let pic_url = next_song.album.pic_url.clone();
                                next_songs_info.push((next_song.name.clone(), author, pic_url));
                            }
                            _ => {}
                        }
                    }
                }
                
                // 更新队列：移除已播放的，添加新获取的
                {
                    let mut queue = initial_queue.write().await;
                    if !queue.is_empty() {
                        queue.remove(0);
                    }
                    for info in next_songs_info {
                        queue.push(info);
                    }
                }
                
                // 构建当前队列显示
                let queue_songs: Vec<(String, String, String)> = initial_queue.read().await.clone();
                
                // 获取歌曲 URL（这个必须实时获取，因为会过期）
                let audio_url = match netease.get_song_url(*track_id).await {
                    Ok(Some(url)) => url,
                    _ => {
                        warn!("歌曲 {} 无法获取播放链接", song.name);
                        continue;
                    }
                };
                
                // 下载歌曲
                let local_file = match netease.download_song(&audio_url, *track_id).await {
                    Ok(path) => path,
                    Err(e) => {
                        warn!("下载歌曲 {} 失败: {}", song.name, e);
                        continue;
                    }
                };
                
                drop(netease);
                
                // 获取推流地址
                let (ip, port, streaming_info) = {
                    if let Some(client) = api_client.read().await.as_ref() {
                        let _ = client.leave_voice_channel(&vc_id).await;
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        
                        match client.join_voice_channel(&vc_id).await {
                            Ok(conn_info) => {
                                let ip = conn_info.ip.clone().unwrap_or_default();
                                let port = conn_info.port.unwrap_or(0);
                                let streaming_info = VoiceStreamingInfo {
                                    ip: ip.clone(),
                                    port: port as u16,
                                    rtcp_port: conn_info.rtcp_port.map(|p| p as u16).unwrap_or(port as u16 + 1),
                                    rtcp_mux: conn_info.rtcp_mux.unwrap_or(true),
                                    ssrc: conn_info.audio_ssrc.map(|s| s as u32).unwrap_or(1111),
                                    pt: conn_info.audio_pt.map(|p| p as u8).unwrap_or(111),
                                    bit_rate: conn_info.bitrate.unwrap_or(128000),
                                    sample_rate: 48000,
                                    channels: 2,
                                };
                                (ip, port as u16, streaming_info)
                            }
                            Err(e) => {
                                warn!("加入语音失败: {}", e);
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                };
                
                // 发送播放卡片
                info!("准备发送播放卡片, channel_id: {}", channel_id);
                if let Some(client) = api_client.read().await.as_ref() {
                    // 删除旧卡片
                    if let Some(old_msg_id) = crate::common::play_state::take_play_msg_id() {
                        let _ = client.delete_message(&old_msg_id).await;
                    }
                    
                    use crate::common::card::{build_play_card, PlayCardData, PlayMusic, QueueMusic, Sender as CardSender};
                    
                    // 构建队列歌曲
                    let queue: Vec<QueueMusic> = queue_songs.iter()
                        .map(|(title, author, pic_url)| QueueMusic {
                            title: title.clone(),
                            author: author.clone(),
                            platform: "网易云".to_string(),
                            pic_url: pic_url.clone(),
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
                            nick_name: "歌单播放".to_string(),
                            avatar_url: None,
                        },
                    }).with_queue(queue, track_ids.len() - index);
                    
                    let card_json = build_play_card(&card_data);
                    info!("卡片 JSON: {}", card_json);
                    
                    match client.send_card_message(&channel_id, &card_json).await {
                        Ok(msg_id) => {
                            info!("卡片发送成功, msg_id: {}", msg_id);
                            crate::common::play_state::set_play_msg_id(msg_id);
                        }
                        Err(e) => {
                            error!("卡片发送失败: {}", e);
                        }
                    }
                }
                
                // 在后台线程播放（阻塞等待完成）
                let file = local_file.clone();
                let ip_clone = ip.clone();
                let info = streaming_info.clone();
                
                let handle = tokio::task::spawn_blocking(move || {
                    use crate::audio::{FFmpegDirectStreamer, StreamerConfig};
                    
                    let mut streamer = match FFmpegDirectStreamer::new(StreamerConfig::from(&info)) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("创建流处理器失败: {}", e);
                            return;
                        }
                    };
                    
                    if let Err(e) = streamer.start_stream_url(&file, &ip_clone, port, info.rtcp_port) {
                        error!("推流失败: {}", e);
                    } else {
                        let _ = streamer.wait();
                    }
                    crate::common::play_state::set_stopped();
                });
                
                // 等待播放完成
                let _ = handle.await;
                
                // 短暂等待
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            
            // 检查是否是因停止请求而退出
            let stopped_by_user = crate::common::play_state::is_stop_requested();
            
            // 重置播放状态
            crate::common::play_state::reset_stats();
            
            // 发送听歌报告
            let play_count = crate::common::play_state::get_play_count();
            let duration = crate::common::play_state::get_play_duration();
            let duration_min = duration / 60;
            let duration_sec = duration % 60;
            
            if let Some(client) = api_client.read().await.as_ref() {
                // 删除最后的卡片
                if let Some(old_msg_id) = crate::common::play_state::take_play_msg_id() {
                    let _ = client.delete_message(&old_msg_id).await;
                }
                
                // 发送听歌报告
                let report = if stopped_by_user {
                    format!(
                        "📊 **本次听歌报告**\n\
                        ────────────────\n\
                        🎵 播放歌曲: {} 首\n\
                        ⏱️ 听歌时长: {}分{}秒\n\
                        📀 歌单: {}\n\
                        ────────────────\n\
                        ⏹️ 播放已停止",
                        play_count,
                        duration_min,
                        duration_sec,
                        playlist_name
                    )
                } else {
                    format!(
                        "📊 **本次听歌报告**\n\
                        ────────────────\n\
                        🎵 播放歌曲: {} 首\n\
                        ⏱️ 听歌时长: {}分{}秒\n\
                        📀 歌单: {}\n\
                        ────────────────\n\
                        感谢收听！",
                        play_count,
                        duration_min,
                        duration_sec,
                        playlist_name
                    )
                };
                let _ = client.send_channel_message(&channel_id, &report).await;
            }
            
            info!("歌单播放完成");
        });
        
        CommandResult::Reply(format!("✅ 歌单开始播放，共 {} 首", total_count))
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
        
        // 发送搜索中消息
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(
                channel_id,
                &format!("🔍 正在搜索: **{}**", query)
            ).await;
        }
        
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
                                crate::common::play_state::set_play_msg_id(msg_id);
                            }
                        }
                        
                        // 加入语音频道并播放
                        if let Some((ip, port, streaming_info)) = self.join_voice_for_streaming(ctx, &vc.id, channel_id).await {
                            if let Some(client) = ctx.api_client.read().await.as_ref() {
                                let _ = client.send_channel_message(channel_id,
                                    &format!("🎵 正在播放: **{}** - {}", music.title, music.author)).await;
                            }
                            self.play_song(local_file, ip, port, streaming_info).await;
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

/// 创建音乐模块的所有命令
pub fn create_music_commands(
    netease_client: Arc<RwLock<NeteaseClient>>,
    config: BotConfig,
) -> Vec<Arc<dyn CommandHandler>> {
    vec![
        Arc::new(WyyCommand::new(netease_client.clone())),
        Arc::new(WyyLoginCommand::new(netease_client, config)),
    ]
}
