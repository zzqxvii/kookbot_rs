//! 语音频道命令模块 - 加入/离开语音频道

use async_trait::async_trait;
use tracing::{error, info};

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};

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
        let vm = ctx.voice_manager.lock().await;
        if let Some(voice_manager) = vm.as_ref() {
            match voice_manager.leave_channel().await {
                Ok(_) => CommandResult::Reply("✅ 已离开语音频道".to_string()),
                Err(e) => CommandResult::Error(format!("离开语音频道失败: {}", e)),
            }
        } else {
            CommandResult::Reply("⚠️ 当前不在任何语音频道中".to_string())
        }
    }
}
