//! 歌词查询命令模块

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::music::NeteaseClient;

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
