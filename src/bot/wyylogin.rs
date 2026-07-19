//! 网易云登录命令模块

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::core::config::BotConfig;
use crate::music::NeteaseClient;

pub struct WyyLoginCommand {
    netease_client: Arc<RwLock<NeteaseClient>>,
    config: BotConfig,
    login_in_progress: Arc<AtomicBool>,
    usage: String,
}

impl WyyLoginCommand {
    pub fn new(netease_client: Arc<RwLock<NeteaseClient>>, config: BotConfig) -> Self {
        let usage = format!("{}wyylogin", config.prefix);
        Self { netease_client, config, login_in_progress: Arc::new(AtomicBool::new(false)), usage }
    }
    
    /// 生成二维码图片并上传到 Kook
    async fn generate_and_upload_qrcode(&self, ctx: &CommandContext<'_>, url: &str) -> Option<String> {
        use qrcode::QrCode;
        use image::Luma;

        let code = match QrCode::new(url) {
            Ok(c) => c,
            Err(e) => {
                warn!("生成二维码失败: {}", e);
                return None;
            }
        };

        let image = code.render::<Luma<u8>>().build();
        let mut buffer = std::io::Cursor::new(Vec::new());

        if let Err(e) = image.write_to(&mut buffer, image::ImageFormat::Png) {
            warn!("编码二维码图片失败: {}", e);
            return None;
        }

        let image_data = buffer.into_inner();

        match ctx.api_client.upload_image(&image_data).await {
            Ok(kook_url) => {
                info!("二维码上传成功: {}", kook_url);
                return Some(kook_url);
            }
            Err(e) => {
                warn!("上传二维码图片失败: {}", e);
            }
        }

        None
    }
}

#[async_trait]
impl CommandHandler for WyyLoginCommand {
    fn name(&self) -> &'static str {
        "wyylogin"
    }
    
    fn description(&self) -> &'static str {
        "登录网易云账号（获取完整音质）"
    }
    
    fn usage(&self) -> String {
        self.usage.clone()
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        // 并发守卫：防止重复登录
        if self.login_in_progress.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return CommandResult::Error("登录已在进行中，请等待完成".to_string());
        }

        let channel_id = &ctx.data.target_id;
        
        // 发送初始化消息
        if let Err(e) = ctx.api_client.send_channel_message(
            channel_id,
            "🔑 正在生成网易云登录二维码..."
        ).await {
            warn!("发送初始化消息失败: {}", e);
        };
        
        let netease = self.netease_client.read().await;
        
        // 获取二维码 key
        let key = match netease.get_qr_key().await {
            Ok(key_data) => key_data.unikey,
            Err(e) => {
                self.login_in_progress.store(false, Ordering::Release);
                return CommandResult::Error(format!("获取二维码失败: {}", e));
            }
        };
        
        // 生成二维码
        let qr_code = match netease.create_qr_code(&key).await {
            Ok(qr) => qr,
            Err(e) => {
                self.login_in_progress.store(false, Ordering::Release);
                return CommandResult::Error(format!("生成二维码失败: {}", e));
            }
        };
        
        drop(netease); // 释放锁
        
        // 生成二维码图片并上传
        info!("开始生成并上传二维码图片...");
        let image_url = self.generate_and_upload_qrcode(&ctx, &qr_code.qrurl).await;
        info!("二维码上传结果: {:?}", image_url);
        
        // 使用配置的前缀
        let prefix = ctx.config.prefix.clone();
        
        // 发送二维码
        if let Some(url) = &image_url {
            info!("发送图片消息: {}", url);
            if let Err(e) = ctx.api_client.send_image_message(channel_id, url).await {
                warn!("发送二维码图片失败: {}", e);
            }
            if let Err(e) = ctx.api_client.send_channel_message(channel_id,
                "📱 **请扫描上方二维码登录网易云音乐**\n⏰ 二维码有效期 5 分钟").await {
                warn!("发送扫码提示失败: {}", e);
            };
        } else {
            warn!("二维码上传失败，发送链接");
            if let Err(e) = ctx.api_client.send_channel_message(channel_id,
                &format!(
                    "📱 **网易云登录**\n\n点击链接扫码：{}\n\n⏰ 二维码有效期 5 分钟",
                    qr_code.qrurl
                )).await {
                warn!("发送二维码链接失败: {}", e);
            };
        }
        
        // 轮询检查登录状态
        let api_client = ctx.api_client.clone();
        let netease_api_url = self.config.music.netease_api_url.clone();
        let key_clone = key.clone();
        let channel_id_clone = channel_id.to_string();
        // 从 BotConfig 获取配置路径
        let config_path = ctx.config.config_path.clone().unwrap_or_else(|| std::path::PathBuf::from("config.toml"));
        let login_flag = self.login_in_progress.clone();
        
        tokio::spawn(async move {
            let netease_client = match NeteaseClient::new(&netease_api_url) {
                Ok(c) => c,
                Err(e) => {
                    warn!("创建网易云客户端失败: {}", e);
                    return;
                }
            };
            let mut attempts = 0;
            let max_attempts = 60;
            let key_str = key_clone.clone();
            info!("启动登录检查任务，key: {}", key_str);
            
            struct LoginGuard(Arc<AtomicBool>);
            impl Drop for LoginGuard {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                    info!("登录任务结束，重置状态");
                }
            }
            let _guard = LoginGuard(login_flag.clone());
            
            loop {
                attempts += 1;
                info!("检查登录状态... ({}/{}), key: {}", attempts, max_attempts, key_str);
                
                if attempts > max_attempts {
                    if let Err(e) = api_client.send_channel_message(&channel_id_clone,
                        &format!("⏰ 二维码已过期，请重新发送 `{}wyylogin`", prefix)).await {
                        warn!("发送过期提示失败: {}", e);
                    }
                    break;
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                match netease_client.check_qr_status(&key_clone).await {
                    Ok(result) => {
                        info!("登录状态码: {}", result.code);
                        match result.code {
                            800 => {
                                if let Err(e) = api_client.send_channel_message(&channel_id_clone,
                                    &format!("⏰ 二维码已过期，请重新发送 `{}wyylogin`", prefix)).await {
                                    warn!("发送过期提示失败: {}", e);
                                }
                                break;
                            }
                            801 => {
                                // 等待扫码
                                info!("等待扫码中...");
                            }
                            802 => {
                                info!("已扫码，等待确认");
                                if let Err(e) = api_client.send_channel_message(&channel_id_clone,
                                    "✅ 已扫描，请在手机上确认登录").await {
                                    warn!("发送确认提示失败: {}", e);
                                }
                            }
                            803 => {
                                info!("登录成功! cookie: {:?}", result.cookie);
                                if let Some(cookie) = &result.cookie {
                                    use crate::common::utils::update_netease_cookie;
                                    match update_netease_cookie(&config_path, cookie) {
                                        Ok(_) => {
                                            let nickname = result.nickname.as_deref().unwrap_or("用户");
                                            if let Err(e) = api_client.send_channel_message(&channel_id_clone,
                                                &format!("🎉 登录成功！欢迎 **{}**\nCookie 已保存，请重启机器人后使用 `{}wyy` 播放完整音质", nickname, prefix)).await {
                                                warn!("发送登录成功消息失败: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            error!("保存 cookie 失败: {}", e);
                                            if let Err(e2) = api_client.send_channel_message(&channel_id_clone,
                                                &format!("⚠️ 登录成功，但保存 Cookie 失败: {}", e)).await {
                                                warn!("发送保存失败消息失败: {}", e2);
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                            _ => {
                                warn!("未知的登录状态码: {}", result.code);
                            }
                        }
                    }
                    Err(e) => {
                        error!("检查登录状态失败: {}", e);
                    }
                }
            }
        });
        
        CommandResult::Ok
    }
}
