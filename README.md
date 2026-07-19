# 🎵 KookBot.rs (RKM)

[![Rust](https://img.shields.io/badge/Rust-1.93+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)]()

用 Rust 编写的高性能 Kook 机器人框架，采用模块化设计，支持多种音乐平台和功能扩展。

> **注意**: 本项目定位为通用的 Kook Bot 平台，音乐播放只是内置的其中一个功能模块。通过命令系统，可以轻松添加更多功能模块。

## ✨ 核心特性

- 🧩 **模块化命令系统**: 可插拔的命令处理器，支持动态注册/注销
- 🎶 **多平台音乐支持**:
  - **网易云音乐** — 搜索、歌单、链接、ID 播放
  - **QQ 音乐** — 搜索、歌单、链接播放
  - **哔哩哔哩** — 搜索、BV号、视频链接播放
- 🔍 **跨平台搜索**: 同时搜索网易云 + QQ + B站，一键选择播放
- 📝 **歌词查询**: 支持通过歌曲 ID 查询歌词
- 🎙️ **语音支持**: 完整的语音频道加入/离开，RTP 流式推流
- 🎛️ **播放控制卡片**: 下一首、停止按钮（支持管理员权限控制）
- 🔄 **双模式连接**:
  - **WebSocket 模式** — 主动连接到 Kook Gateway（支持自动重连）
  - **Webhook 模式** — 被动接收 Kook 推送的事件
- 🔐 **Webhook 加密**: 支持 Kook Webhook 加密消息解密验证
- ⚡ **高性能**: Rust 原生实现，内存占用低 (~50MB)，启动快速
- ⚙️ **灵活配置**: 完善的 TOML 配置系统
- 🌐 **API 后端管理**: 自动启动/管理多个音乐 API 后端

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
| 二维码生成 | [qrcode](https://crates.io/crates/qrcode) / [image](https://crates.io/crates/image) |

## 📦 依赖项

- **Rust** 1.93+ （推荐最新稳定版）
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
prefix = "/"

# 管理员用户ID列表（为空则所有人可操作控制按钮）
admins = []

[audio]
volume = 0.5
bit_rate = 128000

[music]
cache_dir = "./cache"
max_cache_size_mb = 1024
netease_api_url = "http://localhost:3000"
qqmusic_api_url = "http://localhost:3300"
bilibili_api_url = "http://localhost:3400"

[player]
max_queue_size = 100
allow_duplicates = false
preload_count = 2
```

> **注意**: 需要先启动对应的音乐 API 后端服务：
> - 网易云: [NeteaseCloudMusicApi](https://github.com/Binaryify/NeteaseCloudMusicApi) (端口 3000)
> - QQ音乐: [QQMusicApi](https://github.com/jsososo/QQMusicApi) (端口 3300)
> - B站: BilibiliMusicApi (端口 3400)

或者设置环境变量让 Bot 自动管理后端：

```bash
# 设置 API 项目目录，Bot 会自动启动
export NETEASE_API_DIR=/path/to/NeteaseCloudMusicApi
export QQMUSIC_API_DIR=/path/to/QQMusicApi
```

### 5. 邀请机器人进服务器

1. 在 Kook 开发者平台的应用页面
2. 点击"邀请链接"
3. 选择你的服务器并授权

### 6. 运行机器人

```bash
# 开发模式 (带日志)
cargo run

# 生产模式 (优化编译)
cargo run --release

# 指定配置文件
cargo run --release -- --config /path/to/config.toml
```

## 💬 命令列表

在 Kook 中发送命令（默认前缀 `/`）：

| 命令 | 别名 | 说明 | 示例 |
|------|------|------|------|
| `/help` | `/h` | 显示帮助信息 | `/help` |
| `/join` | `/j` | 加入你的语音频道 | `/join` |
| `/leave` | `/l` | 离开语音频道 | `/leave` |
| `/wyy` | - | 播放网易云音乐 | `/wyy 晴天` |
| `/wyylogin` | - | 登录网易云账号 | `/wyylogin` |
| `/qqmusic` | `/qq`, `/qq音乐` | 播放 QQ 音乐 | `/qqmusic 七里香` |
| `/bilibili` | `/bili`, `/b站` | 播放 B 站音乐 | `/bilibili BV1xx` |
| `/搜索` | `/search`, `/搜`, `/s` | 跨平台搜索歌曲 | `/搜索 晴天` |
| `/歌词` | `/lyric`, `/lrc`, `/gc` | 查询歌曲歌词 | `/歌词 188755` |
| `/状态` | `/status`, `/zt`, `/bot` | 查看播放状态 | `/状态` |

### 播放支持

每个音乐平台命令均支持多种输入方式：

- 🎵 **歌曲名称搜索**: `/wyy 晴天`
- 🔗 **歌曲链接**: `/wyy https://music.163.com/song?id=xxx`
- 📋 **歌单链接**: `/wyy https://music.163.com/playlist?id=xxx`
- 🔢 **歌曲 ID**: `/wyy 188755`
- 📼 **B站 BV 号**: `/bilibili BV1xx411P7pC`

### 播放控制

播放歌曲时会自动发送控制卡片，包含：
- ⏭️ **下一首** — 跳过当前歌曲
- ⏹️ **停止** — 停止播放并离开语音频道

> 设置 `admins` 列表后，仅管理员可操作控制按钮。

## 🧩 模块化架构

项目采用模块化设计，命令系统支持动态扩展：

```rust
use async_trait::async_trait;
use kook_music_bot::bot::commands::{CommandContext, CommandHandler, CommandResult};
use std::sync::Arc;

// 创建自定义命令
pub struct MyCommand;

#[async_trait]
impl CommandHandler for MyCommand {
    fn name(&self) -> &'static str { "mycmd" }
    fn aliases(&self) -> Vec<&'static str> { vec!["m"] }
    fn description(&self) -> &'static str { "我的自定义命令" }
    fn usage(&self) -> &'static str { "/mycmd" }

    async fn execute(&self, ctx: CommandContext<'_>) -> CommandResult {
        CommandResult::Reply("Hello!".to_string())
    }
}

// 注册命令
router.register(Arc::new(MyCommand));
```

### 模块列表

| 模块 | 说明 |
|------|------|
| `bot/commands` | 命令路由系统 (Router + Handler trait) |
| `bot/music` | 网易云音乐播放模块 |
| `bot/qqmusic` | QQ 音乐播放模块 |
| `bot/bilibili` | 哔哩哔哩音乐播放模块 |
| `bot/search` | 跨平台统一搜索 |
| `bot/lyric` | 歌词查询模块 |
| `bot/status` | Bot 状态查看 |
| `bot/voice` | 语音频道加入/离开 |
| `bot/wyylogin` | 网易云扫码登录 |
| `bot/playback` | 播放控制模块 |
| `bot/streaming` | 共享推流辅助逻辑 |
| `api` | Kook REST API 客户端 |
| `gateway` | WebSocket Gateway 客户端（含自动重连） |
| `webhook` | Webhook 服务器（含加密解密） |
| `audio` | 音频处理 (Symphonia解码 / FFmpeg编码 / RTP推流) |
| `player` | 播放控制 (队列 / 播放列表 / 预加载 / 语音管理) |
| `music` | 音乐源 API 客户端 (网易云 / QQ / B站) |
| `common` | 公共工具 (缓存 / 日志 / 卡片消息 / 后端管理) |
| `core` | 核心 (配置管理 / 错误定义) |

## 📁 项目结构

```
RKM/
├── Cargo.toml                  # Rust 项目配置
├── config.example.toml         # 配置示例
├── config.toml                 # 本地配置文件 (gitignored)
├── src/
│   ├── main.rs                 # 程序入口 / 启动流程
│   ├── lib.rs                  # 模块导出
│   ├── api/
│   │   ├── mod.rs
│   │   └── client.rs           # Kook REST API 客户端
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── decoder.rs          # Symphonia 音频解码
│   │   ├── ffmpeg_encoder.rs   # FFmpeg 编码器封装
│   │   ├── ffmpeg_streamer.rs  # FFmpeg 流式编码
│   │   ├── rtp.rs             # RTP 推流
│   │   ├── silence.rs         # 静音帧生成
│   │   └── streamer.rs        # 流式推流器
│   ├── bot/
│   │   ├── mod.rs             # Bot 主逻辑 / 事件处理
│   │   ├── commands.rs        # 命令系统 (Router / Handler trait)
│   │   ├── help.rs            # 帮助命令
│   │   ├── voice.rs           # join / leave 命令
│   │   ├── music.rs           # 网易云播放命令
│   │   ├── qqmusic.rs         # QQ音乐播放命令
│   │   ├── bilibili.rs        # B站音乐播放命令
│   │   ├── search.rs          # 跨平台搜索命令
│   │   ├── lyric.rs           # 歌词查询命令
│   │   ├── status.rs          # 状态查看命令
│   │   ├── wyylogin.rs        # 网易云登录命令
│   │   ├── playback.rs        # 播放控制模块
│   │   └── streaming.rs       # 共享推流辅助
│   ├── common/
│   │   ├── mod.rs
│   │   ├── cache.rs           # 缓存管理
│   │   ├── card.rs            # Kook 卡片消息
│   │   ├── console.rs         # 控制台格式化输出
│   │   ├── logging.rs         # 日志系统
│   │   ├── models.rs          # 数据模型
│   │   ├── play_state.rs      # 播放状态管理
│   │   ├── backend.rs         # API 后端进程管理
│   │   └── utils.rs           # 工具函数
│   ├── core/
│   │   ├── mod.rs
│   │   ├── config.rs          # 配置管理 (TOML)
│   │   └── error.rs           # 错误定义
│   ├── gateway/
│   │   ├── mod.rs
│   │   ├── client.rs          # Gateway WebSocket 客户端
│   │   ├── events.rs          # 事件定义
│   │   └── protocol.rs        # 协议实现
│   ├── music/
│   │   ├── mod.rs
│   │   ├── downloader.rs      # 音乐下载
│   │   ├── netease.rs         # 网易云 API 客户端
│   │   ├── qqmusic.rs         # QQ音乐 API 客户端
│   │   └── bilibili.rs        # B站 API 客户端
│   ├── player/
│   │   ├── mod.rs
│   │   ├── manager.rs         # 语音连接管理
│   │   ├── playlist.rs       # 播放列表
│   │   ├── preloader.rs      # 预加载
│   │   └── queue.rs          # 队列管理
│   └── webhook/
│       ├── mod.rs
│       ├── handler.rs        # 事件处理
│       ├── server.rs         # HTTP 服务器
│       ├── verifier.rs       # 签名验证
│       └── decrypt.rs        # 消息解密
└── tests/
    └── integration_test.rs
```

## ⚙️ 配置详解

### 连接模式

**WebSocket 模式** (默认):
- 机器人主动连接到 Kook 服务器
- 支持自动重连（指数退避，最大 60 秒）
- 适合大多数场景，无需公网 IP

```toml
mode = "websocket"
```

**Webhook 模式**:
- Kook 服务器主动推送事件到机器人
- 需要机器人可从公网访问
- 支持加密消息验证和解密

```toml
mode = "webhook"

[webhook]
host = "0.0.0.0"
port = 8080
path = "/webhook"
verify_token = "你的验证令牌"
```

### 音频配置

```toml
[audio]
# 音量 (0.0 - 1.0)
volume = 0.5

# 比特率 (bps)
# Kook 支持: 16000, 32000, 64000, 96000, 128000
bit_rate = 128000
```

### 音乐源配置

```toml
[music]
# 缓存目录和大小限制
cache_dir = "./cache"
max_cache_size_mb = 1024

# 网易云音乐 API
netease_api_url = "http://localhost:3000"
netease_cookie = ""      # 可选，登录后填入 Cookie 获取完整音质

# QQ 音乐 API
qqmusic_api_url = "http://localhost:3300"
qqmusic_cookie = ""      # 可选

# B站 API
bilibili_api_url = "http://localhost:3400"
bilibili_cookie = ""     # 可选
```

### 播放器配置

```toml
[player]
max_queue_size = 100       # 最大队列长度
allow_duplicates = false   # 是否允许重复歌曲
preload_count = 2          # 预加载歌曲数量
```

### 网络配置

```toml
[network]
timeout = 30               # 连接超时（秒）
```

### 管理员配置

```toml
# 管理员用户ID列表，为空则所有人可操作控制按钮
admins = ["user_id_1", "user_id_2"]
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
```

Bot 启动时会自动检测 FFmpeg，未安装将直接报错退出。

### 音乐 API 不可用

1. 确保对应的 API 后端服务已启动：
   - 网易云: [NeteaseCloudMusicApi](https://github.com/Binaryify/NeteaseCloudMusicApi) → 端口 3000
   - QQ音乐: [QQMusicApi](https://github.com/jsososo/QQMusicApi) → 端口 3300
   - B站: BilibiliMusicApi → 端口 3400
2. 检查 `config.toml` 中对应的 `*_api_url` 配置正确
3. 或设置环境变量 `NETEASE_API_DIR` / `QQMUSIC_API_DIR` 让 Bot 自动管理后端进程

### 音频播放异常

1. **检查 FFmpeg 编码器支持**:
   ```bash
   ffmpeg -encoders | grep opus
   ```

2. **调整音频配置**: 尝试降低比特率
   ```toml
   [audio]
   bit_rate = 64000
   ```

### Webhook 模式无法接收事件

1. **公网访问**: 确保机器人服务器可从公网访问
2. **端口开放**: 检查防火墙是否开放了配置的端口
3. **URL 配置**: 确认 Kook 开发者平台配置的 Webhook URL 正确
4. **验证令牌**: 检查 `verify_token` 是否与平台配置一致

## 📜 开源协议

本项目采用 [MIT 许可证](LICENSE)

## 🙏 致谢

- [Kook](https://kookapp.cn/) — 优秀的聊天平台
- [NeteaseCloudMusicApi](https://github.com/Binaryify/NeteaseCloudMusicApi) — 网易云音乐 API
- [QQMusicApi](https://github.com/jsososo/QQMusicApi) — QQ 音乐 API
- [Symphonia](https://github.com/pdeljanov/symphonia) — 优秀的 Rust 音频解码库
- [Tokio](https://tokio.rs/) — 强大的异步运行时

---

<p align="center">
  如果这个项目对你有帮助，请给个 ⭐ Star！
</p>
