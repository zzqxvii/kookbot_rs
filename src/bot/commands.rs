//! Bot 命令系统 - 模块化命令路由和分发
//! 
//! 提供可扩展的命令系统：
//! - CommandHandler trait: 定义命令处理器接口
//! - CommandRouter: 负责命令路由和分发
//! - CommandContext: 命令执行的上下文
//! 
//! 使用示例：
//! ```rust
//! // 定义自定义命令
//! pub struct MyCommand;
//! 
//! #[async_trait]
//! impl CommandHandler for MyCommand {
//!     fn name(&self) -> &'static str { "hello" }
//!     fn description(&self) -> &'static str { "打招呼" }
//!     
//!     async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
//!         CommandResult::Reply("Hello!".to_string())
//!     }
//! }
//! 
//! // 注册命令
//! router.register(Arc::new(MyCommand));
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info};

use crate::api::KookClient;
use crate::core::config::BotConfig;
use crate::gateway::MessageData;
use crate::music::NeteaseClient;
use crate::player::VoiceManager;

/// 命令上下文
pub struct CommandContext<'a> {
    /// 消息数据
    pub data: &'a MessageData,
    /// 命令参数
    pub args: Vec<&'a str>,
    /// API 客户端
    pub api_client: Arc<RwLock<Option<KookClient>>>,
    /// Bot 配置
    pub config: &'a BotConfig,
    /// 网易云客户端
    pub netease_client: Arc<RwLock<NeteaseClient>>,
    /// 语音管理器
    pub voice_manager: Arc<Mutex<Option<VoiceManager>>>,
}

/// 命令处理结果
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// 成功
    Ok,
    /// 错误信息
    Error(String),
    /// 需要回复的消息
    Reply(String),
}

/// 命令处理器 trait
#[async_trait]
pub trait CommandHandler: Send + Sync {
    /// 命令名称（如 "help", "wyy"）
    fn name(&self) -> &'static str;
    
    /// 命令别名
    fn aliases(&self) -> Vec<&'static str> {
        vec![]
    }
    
    /// 命令描述
    fn description(&self) -> &'static str;
    
    /// 使用方法
    fn usage(&self) -> &'static str;
    
    /// 处理命令
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult;
}

/// 命令路由器
pub struct CommandRouter {
    /// 命令前缀
    prefix: String,
    /// 注册的命令处理器
    handlers: HashMap<String, Arc<dyn CommandHandler>>,
}

impl CommandRouter {
    /// 创建新的命令路由器
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            handlers: HashMap::new(),
        }
    }
    
    /// 注册命令处理器
    pub fn register(&mut self, handler: Arc<dyn CommandHandler>) {
        let name = handler.name().to_lowercase();
        info!("注册命令: {}", name);
        self.handlers.insert(name.clone(), handler.clone());
        
        // 注册别名
        for alias in handler.aliases() {
            let alias = alias.to_lowercase();
            debug!("注册命令别名: {} -> {}", alias, name);
            self.handlers.insert(alias, handler.clone());
        }
    }
    
    /// 注销命令
    pub fn unregister(&mut self, name: &str) {
        let name = name.to_lowercase();
        if let Some(handler) = self.handlers.remove(&name) {
            info!("注销命令: {}", handler.name());
            
            // 同时注销别名
            for alias in handler.aliases() {
                self.handlers.remove(alias);
            }
        }
    }
    
    /// 解析命令
    fn parse_command<'a>(&self, content: &'a str) -> Option<(&'a str, Vec<&'a str>)> {
        if !content.starts_with(&self.prefix) {
            return None;
        }
        
        let content = &content[self.prefix.len()..];
        let parts: Vec<&str> = content.split_whitespace().collect();
        
        if parts.is_empty() {
            return None;
        }
        
        let cmd = parts[0];
        let args = parts[1..].to_vec();
        
        Some((cmd, args))
    }
    
    /// 处理消息
    pub async fn handle_message(
        &self,
        data: &MessageData,
        api_client: Arc<RwLock<Option<KookClient>>>,
        config: &BotConfig,
        netease_client: Arc<RwLock<NeteaseClient>>,
        voice_manager: Arc<Mutex<Option<VoiceManager>>>,
    ) -> Option<CommandResult> {
        // 忽略机器人消息
        if data.is_from_bot() {
            return None;
        }
        
        let (cmd_name, args) = self.parse_command(&data.content)?;
        let cmd_name = cmd_name.to_lowercase();
        
        let handler = self.handlers.get(&cmd_name)?;
        
        info!("[CommandRouter] 执行命令: {}", cmd_name);
        
        let ctx = CommandContext {
            data,
            args,
            api_client,
            config,
            netease_client,
            voice_manager,
        };
        
        Some(handler.execute(ctx).await)
    }
    
    /// 获取帮助信息
    pub fn get_help(&self) -> String {
        let mut help = "🎵 **Kook Music Bot** 🎵\n\n**可用命令：**\n".to_string();
        
        // 收集唯一的命令（去重别名）
        let mut seen = std::collections::HashSet::new();
        for handler in self.handlers.values() {
            let name = handler.name();
            if seen.insert(name) {
                let aliases = if handler.aliases().is_empty() {
                    String::new()
                } else {
                    format!(" ({}", handler.aliases().join(", "))
                };
                help.push_str(&format!(
                    "`{}{}{}` - {}\n",
                    self.prefix, name, aliases, handler.description()
                ));
            }
        }
        
        help
    }
    
    /// 获取所有命令列表
    pub fn list_commands(&self) -> Vec<(&str, &str)> {
        let mut seen = std::collections::HashSet::new();
        self.handlers
            .values()
            .filter_map(|h| {
                let name = h.name();
                if seen.insert(name) {
                    Some((name, h.description()))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct TestCommand;
    
    #[async_trait]
    impl CommandHandler for TestCommand {
        fn name(&self) -> &'static str {
            "test"
        }
        
        fn aliases(&self) -> Vec<&'static str> {
            vec!["t"]
        }
        
        fn description(&self) -> &'static str {
            "测试命令"
        }
        
        fn usage(&self) -> &'static str {
            "!test <参数>"
        }
        
        async fn execute(&self, _ctx: CommandContext<'_>) -> CommandResult {
            CommandResult::Ok
        }
    }
    
    #[test]
    fn test_parse_command() {
        let router = CommandRouter::new("!");
        
        let (cmd, args) = router.parse_command("!test arg1 arg2").unwrap();
        assert_eq!(cmd, "test");
        assert_eq!(args, vec!["arg1", "arg2"]);
        
        assert!(router.parse_command("not a command").is_none());
    }
}
