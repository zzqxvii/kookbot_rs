//! 帮助命令模块

use async_trait::async_trait;

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};

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
