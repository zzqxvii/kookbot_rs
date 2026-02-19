//! 日志系统 - 自定义日志格式化
//!
//! 提供对齐的日志格式：
//! 格式: [级别] [时间] [文件:行号] 消息
//! 示例: INF 2024/01/15 10:30:45 main.rs:42 启动机器人

use owo_colors::OwoColorize;
use std::fmt;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// 对齐格式化器 - 为日志提供统一的格式
///
/// 格式: [级别] [时间] [文件:行号] 消息
/// 示例: INF 2024/01/15 10:30:45 main.rs:42 启动机器人
pub struct AlignedFormatter;

impl<S, N> FormatEvent<S, N> for AlignedFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        // 级别 (3字符缩写) + 颜色
        let level_str = match *meta.level() {
            tracing::Level::TRACE => "TRC".dimmed().to_string(),
            tracing::Level::DEBUG => "DBG".blue().to_string(),
            tracing::Level::INFO => "INF".green().to_string(),
            tracing::Level::WARN => "WRN".yellow().to_string(),
            tracing::Level::ERROR => "ERR".red().to_string(),
        };
        write!(writer, "{} ", level_str)?;

        // 时间 (日期用/分隔)
        let timestamp = chrono::Local::now().format("%Y/%m/%d %H:%M:%S");
        write!(writer, "{} ", timestamp)?;

        // 文件和行号 (路径固定宽度18字符)
        let file = meta.file().unwrap_or("unknown");
        let line = meta.line().unwrap_or(0);
        let location = if file.len() > 18 {
            format!("{}:{}", &file[file.len() - 18..], line)
        } else {
            format!("{}:{}", file, line)
        };
        write!(writer, "{:<22} ", location.dimmed())?;

        // 消息
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// 初始化日志系统
///
/// # 示例
/// ```
/// use kook_music_bot::logging::init_logging;
///
/// init_logging(tracing::Level::INFO);
/// ```
pub fn init_logging(level: tracing::Level) {
    tracing_subscriber::fmt()
        .with_max_level(level)
        .event_format(AlignedFormatter)
        .init();
}
