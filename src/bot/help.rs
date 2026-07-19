//! 帮助命令模块

use async_trait::async_trait;

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};

/// 帮助命令
pub struct HelpCommand {
    help_text: String,
}

impl HelpCommand {
    pub fn new(help_text: String) -> Self {
        Self { help_text }
    }
}

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
    
    fn usage(&self) -> String {
        "help".to_string()
    }
    
    async fn execute(&self, _ctx: CommandContext<'_>) -> CommandResult {
        CommandResult::Reply(self.help_text.clone())
    }
}
