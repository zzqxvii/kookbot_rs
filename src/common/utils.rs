//! 工具函数集合
//!
//! 提供通用的工具函数：
//! - 时间格式化
//! - 字节大小格式化
//! - URL 验证
//! - 文件名提取
//! - Cookie 更新

/// 时间格式化工具
pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

/// 字节大小格式化
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_index])
}

/// 验证 URL
pub fn is_valid_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// 提取文件名
pub fn extract_filename(path: &str) -> Option<&str> {
    path.rfind('/')
        .or_else(|| path.rfind('\\'))
        .map(|i| &path[i + 1..])
        .or(Some(path))
}

/// 更新配置文件中的网易云 Cookie
///
/// # 参数
/// - `config_path`: 配置文件路径
/// - `cookie`: 新的 cookie 字符串
///
/// # 返回值
/// - `Ok(())`: 更新成功
/// - `Err`: 文件读写错误
///
/// # 说明
/// - 如果配置文件中已存在 `netease_cookie`，则替换第一个匹配项
/// - 如果不存在，则在 `[music]` section 后添加
/// - 如果 `[music]` section 也不存在，则在文件末尾添加
///
/// # 示例
/// ```
/// use std::path::Path;
/// use kook_music_bot::utils::update_netease_cookie;
///
/// // update_netease_cookie(Path::new("config.toml"), "MUSIC_U=xxx;").unwrap();
/// ```
pub fn update_netease_cookie(config_path: &std::path::Path, cookie: &str) -> anyhow::Result<()> {
    use std::fs;
    use tracing::info;

    let content = fs::read_to_string(config_path)?;
    let mut new_lines = Vec::new();
    let mut cookie_replaced = false;
    let mut in_music_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // 检查是否是 [music] section 的开始
        if trimmed == "[music]" {
            in_music_section = true;
            new_lines.push(line.to_string());
            continue;
        }

        // 检查是否是其他 section 的开始（离开 [music] section）
        if trimmed.starts_with('[') && trimmed.ends_with(']') && in_music_section {
            // 如果在离开 [music] section 时还没有替换 cookie，在这里添加
            if !cookie_replaced {
                new_lines.push(format!("netease_cookie = \"{}\"", cookie));
                cookie_replaced = true;
            }
            in_music_section = false;
            new_lines.push(line.to_string());
            continue;
        }

        // 检查是否是 netease_cookie 行（支持前导空格）
        if trimmed.starts_with("netease_cookie") && !cookie_replaced {
            new_lines.push(format!("netease_cookie = \"{}\"", cookie));
            cookie_replaced = true;
            continue;
        }

        // 跳过其他已存在的 netease_cookie 行（重复的）
        if trimmed.starts_with("netease_cookie") && cookie_replaced {
            continue;
        }

        new_lines.push(line.to_string());
    }

    // 如果文件结束还在 [music] section 中且未替换 cookie，在末尾添加
    if in_music_section && !cookie_replaced {
        new_lines.push(format!("netease_cookie = \"{}\"", cookie));
        cookie_replaced = true;
    }

    // 如果没有找到 [music] section，在文件末尾添加
    if !cookie_replaced {
        new_lines.push(String::new());
        new_lines.push("[music]".to_string());
        new_lines.push(format!("netease_cookie = \"{}\"", cookie));
    }

    fs::write(config_path, new_lines.join("\n"))?;
    info!("Cookie 已保存到 {:?}", config_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(61), "1:01");
        assert_eq!(format_duration(59), "0:59");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
    }

    #[test]
    fn test_is_valid_url() {
        assert!(is_valid_url("https://example.com"));
        assert!(is_valid_url("http://example.com"));
        assert!(!is_valid_url("ftp://example.com"));
    }
}
