//! 终端 UI 渲染工具
//!
//! 提供用于启动日志的 box-drawing 宏和辅助函数。
//! 从 `main.rs` 中提取，保持入口文件简洁。

use unicode_width::UnicodeWidthStr;

pub const WIDTH: usize = 52;

pub fn w(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn center(s: &str, width: usize) -> String {
    let s_width = w(s);
    if s_width >= width {
        s.to_string()
    } else {
        let left = (width - s_width) / 2;
        let right = width - s_width - left;
        format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
    }
}

pub fn line(left: &str, content: &str, right: &str, width: usize) -> String {
    let content_width = w(content);
    let total = w(left) + content_width + w(right);
    if total >= width {
        format!("{}{}{}", left, content, right)
    } else {
        format!("{}{}{}{}", left, content, " ".repeat(width - total), right)
    }
}

#[macro_export]
macro_rules! box_title {
    ($title:expr) => {
        tracing::info!("╭{}╮", "─".repeat($crate::common::console::WIDTH - 2));
        tracing::info!("│{}│", $crate::common::console::center($title, $crate::common::console::WIDTH - 2));
        tracing::info!("├{}┤", "─".repeat($crate::common::console::WIDTH - 2));
    };
}

#[macro_export]
macro_rules! box_item {
    ($label:expr, $value:expr) => {
        tracing::info!(
            "│ {}│",
            $crate::common::console::line(
                "",
                &format!("{}: {}", $label, $value),
                "",
                $crate::common::console::WIDTH - 3
            )
        );
    };
}

#[macro_export]
macro_rules! box_end {
    () => {
        tracing::info!("╰{}╯", "─".repeat($crate::common::console::WIDTH - 2));
        tracing::info!("");
    };
}

#[macro_export]
macro_rules! status_ok {
    ($label:expr, $value:expr) => {
        tracing::info!("  ✅ {}: {}", $label, $value);
    };
}

#[macro_export]
macro_rules! status_fail {
    ($label:expr, $msg:expr) => {
        tracing::info!("  ❌ {}: {}", $label, $msg);
    };
}
