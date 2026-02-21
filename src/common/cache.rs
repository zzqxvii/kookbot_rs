use std::fs;
use std::path::Path;
use std::time::SystemTime;
use tracing::{debug, info, warn};

/// 清理缓存目录
/// 超过限制时按修改时间删除最旧的文件
pub fn cleanup_cache(cache_dir: &str, max_size_mb: u64) {
    let cache_path = Path::new(cache_dir);
    if !cache_path.exists() {
        return;
    }

    let mut entries = Vec::new();
    let mut total_size: u64 = 0;

    if let Ok(dir) = fs::read_dir(cache_path) {
        for entry in dir.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    let size = metadata.len();
                    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    entries.push((entry.path(), size, modified));
                    total_size += size;
                }
            }
        }
    }

    let max_bytes = max_size_mb * 1024 * 1024;

    if total_size <= max_bytes {
        debug!(
            "缓存大小正常: {} / {} MB",
            total_size / 1024 / 1024,
            max_size_mb
        );
        return;
    }

    entries.sort_by(|a, b| a.2.cmp(&b.2));

    let mut freed: u64 = 0;
    let mut deleted = 0;

    for (path, size, _) in entries {
        if total_size - freed <= max_bytes {
            break;
        }

        match fs::remove_file(&path) {
            Ok(_) => {
                freed += size;
                deleted += 1;
            }
            Err(e) => {
                warn!("删除缓存文件失败: {:?} - {}", path, e);
            }
        }
    }

    if deleted > 0 {
        info!(
            "缓存清理: 删除 {} 个文件，释放 {} MB",
            deleted,
            freed / 1024 / 1024
        );
    }
}

/// 获取缓存大小 (MB)
pub fn get_cache_size_mb(cache_dir: &str) -> u64 {
    let cache_path = Path::new(cache_dir);
    if !cache_path.exists() {
        return 0;
    }

    let mut total_size: u64 = 0;

    if let Ok(dir) = fs::read_dir(cache_path) {
        for entry in dir.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    total_size += metadata.len();
                }
            }
        }
    }

    total_size / 1024 / 1024
}
