//! Webhook 消息解密
//!
//! 支持 AES-256-CBC 加密的消息解密

use crate::core::error::{BotError, Result};

/// 解密器
pub struct MessageDecryptor {
    /// 加密密钥 (需要补齐到 32 字节)
    key: String,
}

impl MessageDecryptor {
    /// 创建新的解密器
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
        }
    }

    /// 解密消息
    ///
    /// 解密流程：
    /// 1. Base64 解码密文
    /// 2. 截取前 16 字节作为 IV
    /// 3. 剩余部分 Base64 解码得到实际密文
    /// 4. 补齐 key 到 32 字节
    /// 5. AES-256-CBC 解密
    pub fn decrypt(&self, encrypted_data: &str) -> Result<String> {
        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
        use base64::Engine;

        // 1. Base64 解码整个密文
        let encrypted_bytes = base64::engine::general_purpose::STANDARD
            .decode(encrypted_data)
            .map_err(|e| BotError::WebhookError(format!("Base64 解码失败: {}", e)))?;

        if encrypted_bytes.len() < 16 {
            return Err(BotError::WebhookError("密文太短".to_string()));
        }

        // 2. 截取前 16 字节作为 IV
        let iv = &encrypted_bytes[0..16];

        // 3. 剩余部分作为实际的加密数据 (需要再次 Base64 解码)
        let encrypted_content = std::str::from_utf8(&encrypted_bytes[16..])
            .map_err(|e| BotError::WebhookError(format!("无效的 UTF-8 编码: {}", e)))?;

        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(encrypted_content)
            .map_err(|e| BotError::WebhookError(format!("内容 Base64 解码失败: {}", e)))?;

        // 4. 补齐 key 到 32 字节 (使用 \0 填充)
        let mut key_bytes = self.key.clone().into_bytes();
        while key_bytes.len() < 32 {
            key_bytes.push(0);
        }
        if key_bytes.len() > 32 {
            key_bytes.truncate(32);
        }

        // 5. AES-256-CBC 解密
        type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

        let decryptor = Aes256CbcDec::new(
            key_bytes.as_slice().into(),
            iv.into(),
        );

        let mut buf = ciphertext.clone();
        let decrypted = decryptor
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .map_err(|e| BotError::WebhookError(format!("解密失败: {:?}", e)))?;

        let plaintext = String::from_utf8(decrypted.to_vec())
            .map_err(|e| BotError::WebhookError(format!("解密结果不是有效的 UTF-8: {}", e)))?;

        Ok(plaintext)
    }
}

/// 解密 Webhook 请求
pub fn decrypt_webhook_request(encrypt_key: &str, encrypted_data: &str) -> Result<String> {
    let decryptor = MessageDecryptor::new(encrypt_key);
    decryptor.decrypt(encrypted_data)
}
