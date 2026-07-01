//! Bot 状态命令模块

use async_trait::async_trait;
use std::sync::Arc;

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::common::play_state::PlayState;

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
