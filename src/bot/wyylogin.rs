//! 网易云登录命令模块

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::bot::commands::{CommandContext, CommandHandler, CommandResult};
use crate::core::config::BotConfig;
use crate::music::NeteaseClient;

/// 网易云登录命令
pub struct WyyLoginCommand {
    netease_client: Arc<RwLock<NeteaseClient>>,
    config: BotConfig,
}

impl WyyLoginCommand {
    pub fn new(netease_client: Arc<RwLock<NeteaseClient>>, config: BotConfig) -> Self {
        Self { netease_client, config }
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

        if let Some(client) = ctx.api_client.read().await.as_ref() {
            match client.upload_image(&image_data).await {
                Ok(kook_url) => {
                    info!("二维码上传成功: {}", kook_url);
                    return Some(kook_url);
                }
                Err(e) => {
                    warn!("上传二维码图片失败: {}", e);
                }
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
    
    fn usage(&self) -> &'static str {
        "!wyylogin"
    }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        let channel_id = &ctx.data.target_id;
        
        // 发送初始化消息
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            let _ = client.send_channel_message(
                channel_id,
                "🔑 正在生成网易云登录二维码..."
            ).await;
        }
        
        let netease = self.netease_client.read().await;
        
        // 获取二维码 key
        let key = match netease.get_qr_key().await {
            Ok(key_data) => key_data.unikey,
            Err(e) => {
                return CommandResult::Error(format!("获取二维码失败: {}", e));
            }
        };
        
        // 生成二维码
        let qr_code = match netease.create_qr_code(&key).await {
            Ok(qr) => qr,
            Err(e) => {
                return CommandResult::Error(format!("生成二维码失败: {}", e));
            }
        };
        
        drop(netease); // 释放锁
        
        // 生成二维码图片并上传
        info!("开始生成并上传二维码图片...");
        let image_url = self.generate_and_upload_qrcode(&ctx, &qr_code.qrurl).await;
        info!("二维码上传结果: {:?}", image_url);
        
        // 发送二维码
        if let Some(client) = ctx.api_client.read().await.as_ref() {
            if let Some(ref url) = image_url {
                info!("发送图片消息: {}", url);
                let _ = client.send_image_message(channel_id, url).await;
                let _ = client.send_channel_message(channel_id,
                    "📱 **请扫描上方二维码登录网易云音乐**\n⏰ 二维码有效期 5 分钟").await;
            } else {
                warn!("二维码上传失败，发送链接");
                let _ = client.send_channel_message(channel_id,
                    &format!(
                        "📱 **网易云登录**\n\n点击链接扫码：{}\n\n⏰ 二维码有效期 5 分钟",
                        qr_code.qrurl
                    )).await;
            }
        }
        
        // 轮询检查登录状态
        let api_client = ctx.api_client.clone();
        let netease_api_url = self.config.music.netease_api_url.clone();
        let key_clone = key.clone();
        let channel_id_clone = channel_id.to_string();
        let config_path = std::path::PathBuf::from("config.toml");
        
        tokio::spawn(async move {
            let netease_client = NeteaseClient::new(&netease_api_url);
            let mut attempts = 0;
            let max_attempts = 60;
            let key_str = key_clone.clone();
            info!("启动登录检查任务，key: {}", key_str);
            
            loop {
                attempts += 1;
                info!("检查登录状态... ({}/{}), key: {}", attempts, max_attempts, key_str);
                
                if attempts > max_attempts {
                    if let Some(client) = api_client.read().await.as_ref() {
                        let _ = client.send_channel_message(&channel_id_clone,
                            "⏰ 二维码已过期，请重新发送 `/wyylogin`").await;
                    }
                    break;
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                match netease_client.check_qr_status(&key_clone).await {
                    Ok(result) => {
                        info!("登录状态码: {}", result.code);
                        match result.code {
                            800 => {
                                if let Some(client) = api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(&channel_id_clone,
                                        "⏰ 二维码已过期，请重新发送 `/wyylogin`").await;
                                }
                                break;
                            }
                            801 => {
                                // 等待扫码
                                info!("等待扫码中...");
                            }
                            802 => {
                                info!("已扫码，等待确认");
                                if let Some(client) = api_client.read().await.as_ref() {
                                    let _ = client.send_channel_message(&channel_id_clone,
                                        "✅ 已扫描，请在手机上确认登录").await;
                                }
                            }
                            803 => {
                                info!("登录成功! cookie: {:?}", result.cookie);
                                if let Some(ref cookie) = result.cookie {
                                    use crate::common::utils::update_netease_cookie;
                                    match update_netease_cookie(&config_path, cookie) {
                                        Ok(_) => {
                                            if let Some(client) = api_client.read().await.as_ref() {
                                                let nickname = result.nickname.as_deref().unwrap_or("用户");
                                                let _ = client.send_channel_message(&channel_id_clone,
                                                    &format!("🎉 登录成功！欢迎 **{}**\nCookie 已保存，请重启机器人后使用 `/wyy` 播放完整音质", nickname)).await;
                                            }
                                        }
                                        Err(e) => {
                                            error!("保存 cookie 失败: {}", e);
                                            if let Some(client) = api_client.read().await.as_ref() {
                                                let _ = client.send_channel_message(&channel_id_clone,
                                                    &format!("⚠️ 登录成功，但保存 Cookie 失败: {}", e)).await;
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
