//! API 后端进程管理
//!
//! 管理外部 API 服务（NeteaseCloudMusicApi、QQ Music API 等）作为子进程，
//! 实现自动启动、健康检查和优雅关闭。

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::{Child, Command};
use tracing::{info, warn};

/// API 后端配置
#[derive(Debug, Clone)]
pub struct ApiBackendConfig {
    /// 后端名称
    pub name: String,
    /// 启动命令
    pub command: String,
    /// 命令参数
    pub args: Vec<String>,
    /// 工作目录
    pub work_dir: Option<String>,
    /// 健康检查 URL
    pub health_url: String,
    /// 是否启用
    pub enabled: bool,
    /// 启动超时（秒）
    pub startup_timeout_secs: u64,
}

/// API 后端管理器
pub struct ApiBackendManager {
    processes: Mutex<HashMap<String, Child>>,
    backends: Vec<ApiBackendConfig>,
}

impl ApiBackendManager {
    pub fn new(backends: Vec<ApiBackendConfig>) -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            backends,
        }
    }

    /// 启动所有启用的后端
    pub async fn start_all(&self) {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|e| {
                warn!("创建 HTTP 客户端失败: {}, 使用默认配置", e);
                Default::default()
            });

        for backend in &self.backends {
            if !backend.enabled {
                info!("[ApiBackend] {} 已禁用，跳过", backend.name);
                continue;
            }

            // 先检查是否已在运行
            if self.check_health(&http, &backend.health_url).await {
                info!("[ApiBackend] {} 已在运行: {}", backend.name, backend.health_url);
                continue;
            }

            info!("[ApiBackend] 正在启动 {} ...", backend.name);
            match self.start_backend(backend).await {
                Ok(_) => {
                    // 等待健康检查通过
                    let deadline = tokio::time::Instant::now()
                        + Duration::from_secs(backend.startup_timeout_secs);
                    let mut started = false;

                    while tokio::time::Instant::now() < deadline {
                        if self.check_health(&http, &backend.health_url).await {
                            info!("[ApiBackend] {} 启动成功 ✓", backend.name);
                            started = true;
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }

                    if !started {
                        warn!("[ApiBackend] {} 启动超时（{}秒），继续运行...",
                            backend.name, backend.startup_timeout_secs);
                    }
                }
                Err(e) => {
                    warn!("[ApiBackend] {} 启动失败: {}，功能可能不可用", backend.name, e);
                }
            }
        }
    }

    async fn start_backend(&self, backend: &ApiBackendConfig) -> Result<(), String> {
        let mut cmd = Command::new(&backend.command);
        cmd.args(&backend.args)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if let Some(ref dir) = backend.work_dir {
            cmd.current_dir(dir);
        }

        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut processes = self.processes.lock()
            .map_err(|e| format!("锁获取失败: {}", e))?;
        let child = cmd.spawn()
            .map_err(|e| format!("无法启动 {}: {}", backend.name, e))?;
        processes.insert(backend.name.clone(), child);

        Ok(())
    }

    async fn check_health(&self, http: &reqwest::Client, url: &str) -> bool {
        match http.get(url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// 停止所有子进程（优雅关闭：SIGTERM → 等待 5s → SIGKILL）
    pub async fn shutdown(&self) {
        let processes: Vec<_> = {
            let mut guard = self.processes.lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.drain().collect()
        };
        for (name, mut child) in processes {
            info!("[ApiBackend] 正在停止 {} ...", name);

            if let Some(pid) = child.id() {
                tokio::task::spawn_blocking(move || send_terminate_signal(pid))
                    .await
                    .unwrap_or_else(|e| warn!("spawn_blocking failed: {}", e));

                // 等待最多 5 秒让进程优雅退出
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                let mut exited = false;

                while tokio::time::Instant::now() < deadline {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            info!("[ApiBackend] {} 已退出: {:?}", name, status);
                            exited = true;
                            break;
                        }
                        Ok(None) => {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(e) => {
                            warn!("[ApiBackend] {} wait error: {}", name, e);
                            break;
                        }
                    }
                }

                if !exited {
                    warn!("[ApiBackend] {} 未响应终止信号，强制终止", name);
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            } else {
                // 无法获取 PID，直接强杀
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        info!("[ApiBackend] 所有后端已停止");
    }
}

/// 发送终止信号给进程
///
/// Unix: 发送 SIGTERM；Windows: 使用 taskkill（无 /F，允许进程清理）
fn send_terminate_signal(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .arg("/PID")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// 获取默认后端配置
pub fn default_backends(netease_api_dir: &str, qqmusic_api_dir: &str) -> Vec<ApiBackendConfig> {
    vec![
        ApiBackendConfig {
            name: "NeteaseCloudMusicApi".to_string(),
            command: "node".to_string(),
            args: vec!["app.js".to_string()],
            work_dir: Some(netease_api_dir.to_string()),
            health_url: "http://localhost:3000".to_string(),
            enabled: !netease_api_dir.is_empty(),
            startup_timeout_secs: 30,
        },
        ApiBackendConfig {
            name: "QQMusicApi".to_string(),
            command: "node".to_string(),
            args: vec!["app.js".to_string()],
            work_dir: Some(qqmusic_api_dir.to_string()),
            health_url: "http://localhost:3300".to_string(),
            enabled: !qqmusic_api_dir.is_empty(),
            startup_timeout_secs: 30,
        },
    ]
}
