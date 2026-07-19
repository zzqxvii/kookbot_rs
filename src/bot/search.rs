//! 跨平台统一搜索命令模块

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::music::{BilibiliClient, NeteaseClient, QQMusicClient};

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

    fn usage(&self) -> String {
        "/搜索 <关键词>".to_string()
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
