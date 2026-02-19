# Kook Bot (RKM) 🤖

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)]()

用 Rust 编写的高性能 Kook 机器人框架，采用模块化设计，支持多种功能扩展。

> **注意**: 本项目定位为通用的 Kook Bot 平台，音乐播放只是内置的其中一个功能模块。通过命令系统，可以轻松添加更多功能模块。

## ✨ 核心特性

- 🧩 **模块化命令系统**: 可插拔的命令处理器，支持动态注册/注销
- 🎶 **音乐模块** (内置): 支持网易云音乐搜索和播放
- 🎙️ **语音支持**: 完整的语音频道加入/离开功能
- 🔄 **双模式连接**:
  - **WebSocket 模式** - 主动连接到 Kook Gateway
  - **Webhook 模式** - 被动接收 Kook 推送的事件
- ⚡ **高性能**: Rust 原生实现，内存占用低 (~50MB)，启动快速
- ⚙️ **灵活配置**: 完善的 TOML 配置系统

## 🛠️ 技术栈

| 类别 | 技术 |
|------|------|
| 异步运行时 | [Tokio](https://tokio.rs/) |
| 音频解码 | [Symphonia](https://github.com/pdeljanov/symphonia) (MP3, FLAC, WAV, AAC) |
| 音频编码 | FFmpeg (Opus) |
| HTTP/WebSocket | [reqwest](https://github.com/seanmonstar/reqwest) / [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) |
| Webhook 服务器 | [Axum](https://github.com/tokio-rs/axum) |
| 配置管理 | TOML |
| 日志 | [tracing](https://github.com/tokio-rs/tracing) |

## 📦 依赖项

- **Rust** 1.75+ （推荐最新稳定版）
- **FFmpeg** （音乐模块需要，用于 Opus 音频编码）

### 安装 FFmpeg

**Windows:**
```powershell
# 使用 winget
winget install FFmpeg

# 或使用 chocolatey
choco install ffmpeg
```

**macOS:**
```bash
brew install ffmpeg
```

**Linux (Ubuntu/Debian):**
```bash
sudo apt update
sudo apt install ffmpeg
```

## 🚀 快速开始

### 1. 克隆并构建

```bash
cd RKM
cargo build --release
```

### 2. 创建配置文件

```bash
cp config.example.toml config.toml
```

### 3. 获取 Kook Bot Token

1. 访问 [Kook 开发者平台](https://developer.kookapp.cn/)
2. 登录后点击"创建应用"
3. 填写应用名称和描述
4. 进入应用管理页面，复制 **Token**

### 4. 编辑配置文件

编辑 `config.toml`:

```toml
token = "你的 Kook Bot Token"
prefix = "!"
admins = ["你的用户ID"]

[audio]
volume = 0.5
bit_rate = 64000
sample_rate = 48000
channels = 2

[music]
cache_dir = "./cache"
max_cache_size = 1024
```

### 5. 邀请机器人进服务器

1. 在 Kook 开发者平台的应用页面
2. 点击"邀请链接"
3. 选择你的服务器并授权

### 6. 运行机器人

```bash
# 开发模式 (带日志)
cargo run

# 生产模式 (优化)
cargo run --release

# 指定配置文件
cargo run --release -- --config /path/to/config.toml
```

## 💬 使用方法

在 Kook 中发送命令（默认前缀 `!`）：

| 命令 | 别名 | 说明 | 示例 |
|------|------|------|------|
| `!help` | `!h` | 显示帮助信息 | `!help` |
| `!join` | `!j` | 加入你的语音频道 | `!join` |
| `!leave` | `!l` | 离开语音频道 | `!leave` |
| `!wyy` | - | 播放网易云音乐 | `!wyy 晴天` |
| `!wyylogin` | - | 登录网易云账号 | `!wyylogin` |

## 🧩 模块化架构

项目采用模块化设计，命令系统支持动态扩展：

```rust
// 创建自定义命令
pub struct MyCommand;

#[async_trait]
impl CommandHandler for MyCommand {
    fn name(&self) -> &'static str { "mycmd" }
    fn description(&self) -> &'static str { "我的自定义命令" }
    
    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        CommandResult::Reply("Hello!".to_string())
    }
}

// 注册命令
router.register(Arc::new(MyCommand));
```

### 模块列表

| 模块 | 状态 | 说明 |
|------|------|------|
| `bot/commands` | ✅ 核心 | 命令路由系统 |
| `bot/music` | ✅ 内置 | 网易云音乐播放 |
| `api` | ✅ 核心 | Kook API 客户端 |
| `gateway` | ✅ 核心 | WebSocket 连接 |
| `webhook` | ✅ 核心 | Webhook 服务器 |
| `audio` | ✅ 核心 | 音频处理 |
| `player` | ✅ 核心 | 播放控制 |

## 📁 项目结构

```
RKM/
├── Cargo.toml              # Rust 项目配置
├── config.example.toml     # 配置示例
├── config.toml             # 本地配置文件 (gitignored)
├── src/
│   ├── main.rs             # 程序入口（仅启动逻辑）
│   ├── lib.rs              # 模块导出
│   ├── bot/                # Bot 核心模块
│   │   ├── mod.rs          # Bot 主逻辑
│   │   ├── commands.rs     # 命令系统
│   │   └── music.rs        # 音乐模块命令
│   ├── api/                # Kook REST API
│   ├── audio/              # 音频处理
│   ├── gateway/            # WebSocket 网关
│   ├── music/              # 音乐源 API
│   ├── player/             # 播放控制
│   ├── webhook/            # Webhook 服务器
│   ├── config.rs           # 配置管理
│   ├── error.rs            # 错误定义
│   ├── logging.rs          # 日志系统
│   ├── models.rs           # 数据模型
│   └── utils.rs            # 工具函数
├── target/                 # 编译输出
└── cache/                  # 音乐缓存
```

## ⚙️ 配置详解

### 连接模式

**WebSocket 模式** (默认):
- 机器人主动连接到 Kook 服务器
- 适合大多数场景
- 配置简单，无需公网 IP

```toml
mode = "websocket"
```

**Webhook 模式**:
- Kook 服务器主动推送事件到机器人
- 需要机器人可从公网访问
- 适合大规模部署

```toml
mode = "webhook"

[webhook]
host = "0.0.0.0"
port = 8080
path = "/webhook"
verify_token = "你的验证令牌"
use_ssl = false
```

### 音频配置

```toml
[audio]
# 音量 (0.0 - 1.0)
volume = 0.5

# 比特率，Kook 支持: 16000, 32000, 64000, 96000, 128000
bit_rate = 64000

# 采样率，Kook 只支持 48000
sample_rate = 48000

# 声道数: 1=单声道, 2=立体声
channels = 2
```

## 🐛 故障排除

### 机器人无法连接到 Kook

1. **检查 Token**: 确认 `config.toml` 中的 token 正确无误
2. **网络连接**: 检查网络是否正常，能否访问 `kookapp.cn`
3. **防火墙**: 检查防火墙是否阻止了 WebSocket 连接 (端口 443)

### FFmpeg 未找到

```bash
# 验证 FFmpeg 安装
ffmpeg -version

# 如果不在 PATH 中，在 config.toml 中指定路径
[music]
ffmpeg_path = "/usr/bin/ffmpeg"
```

### 音频播放异常

1. **检查 FFmpeg 编码器支持**:
   ```bash
   ffmpeg -encoders | grep opus
   ```

2. **调整音频配置**: 尝试降低比特率或改为单声道
   ```toml
   [audio]
   bit_rate = 32000
   channels = 1
   ```

### Webhook 模式无法接收事件

1. **公网访问**: 确保机器人服务器可从公网访问
2. **端口开放**: 检查防火墙是否开放了配置的端口
3. **URL 配置**: 确认 Kook 开发者平台配置的 Webhook URL 正确
4. **验证令牌**: 检查 `verify_token` 是否与平台配置一致

## 📜 开源协议

本项目采用 [MIT 许可证](LICENSE)

## 🙏 致谢

- [Kook](https://kookapp.cn/) - 优秀的聊天平台
- [kookbc](https://github.com/KookBC/KookBC) - Java SDK 参考
- [symphonia](https://github.com/pdeljanov/symphonia) - 优秀的 Rust 音频库
- [tokio](https://tokio.rs/) - 强大的异步运行时

---

<p align="center">
  如果这个项目对你有帮助，请给个 ⭐ Star！
</p>
