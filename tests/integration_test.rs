//! 集成测试 — 验证核心命令流程

use std::sync::Arc;
use kook_music_bot::bot::commands::{CommandRouter, CommandHandler, CommandContext, CommandResult};
use kook_music_bot::core::config::BotConfig;
use async_trait::async_trait;

/// 测试用的假命令处理器
struct TestHandler {
    name: &'static str,
    aliases: Vec<&'static str>,
    response: &'static str,
}

#[async_trait]
impl CommandHandler for TestHandler {
    fn name(&self) -> &'static str { self.name }
    fn aliases(&self) -> Vec<&'static str> { self.aliases.clone() }
    fn description(&self) -> &'static str { "测试命令" }
    fn usage(&self) -> String { "!test".to_string() }

    async fn execute(&self, _ctx: CommandContext<'_>) -> CommandResult {
        CommandResult::Reply(self.response.to_string())
    }
}


#[tokio::test]
async fn test_command_router_registration() {
    let mut router = CommandRouter::new("/");
    let handler = Arc::new(TestHandler {
        name: "test",
        aliases: vec!["t"],
        response: "OK",
    });
    router.register(handler);

    // 验证精确匹配通过名称查找（通过 list_commands 间接验证）
    let cmds = router.list_commands();
    assert!(cmds.iter().any(|(name, _)| *name == "test"));
}

#[tokio::test]
async fn test_play_state_lifecycle() {
    use kook_music_bot::common::play_state::PlayState;

    let state = PlayState::new();
    assert!(!state.is_playing());
    assert_eq!(state.get_pid(), 0);
    assert_eq!(state.get_play_count(), 0);

    state.set_playing(1234);
    assert!(state.is_playing());
    assert_eq!(state.get_pid(), 1234);
    assert_eq!(state.get_play_count(), 1);

    state.request_stop();
    assert!(state.is_stop_requested());
    assert!(state.is_playing());

    state.set_stopped();
    assert!(!state.is_playing());
    assert_eq!(state.get_pid(), 0);
}

#[tokio::test]
async fn test_play_state_message_id() {
    use kook_music_bot::common::play_state::PlayState;

    let state = PlayState::new();
    assert!(state.get_play_msg_id().is_none());

    state.set_play_msg_id("msg-123".to_string());
    assert_eq!(state.get_play_msg_id(), Some("msg-123".to_string()));

    let taken = state.take_play_msg_id();
    assert_eq!(taken, Some("msg-123".to_string()));
    assert!(state.get_play_msg_id().is_none());
}

#[tokio::test]
async fn test_play_state_reset_stats() {
    use kook_music_bot::common::play_state::PlayState;

    let state = PlayState::new();
    state.set_playing(1);
    state.set_playing(2);
    assert_eq!(state.get_play_count(), 2);
    assert!(state.is_playing());

    state.reset_stats();
    assert_eq!(state.get_play_count(), 0);
    // reset_stats 只重置统计，不改变播放状态
    assert!(state.is_playing());
}

#[test]
fn test_rtp_packet_roundtrip() {
    use kook_music_bot::audio::rtp::RtpPacket;

    let packet = RtpPacket::new(
        111,
        42,
        1000,
        0xDEADBEEF,
        vec![0xAA, 0xBB, 0xCC],
    );

    let bytes = packet.to_bytes();
    // RTP 固定头 12 字节 + 3 字节payload = 15 字节
    assert_eq!(bytes.len(), 15);
    // V=2, P=0, X=0, CC=0
    assert_eq!(bytes[0], 0x80);
    // M=0, PT=111 (=0x6F)
    assert_eq!(bytes[1], 0x6F);
    // Sequence 42
    assert_eq!(u16::from_be_bytes([bytes[2], bytes[3]]), 42);
    // Timestamp 1000
    assert_eq!(u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]), 1000);
    // SSRC 0xDEADBEEF
    assert_eq!(u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 0xDEADBEEF);
    // Payload
    assert_eq!(&bytes[12..], &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn test_format_duration_edge_cases() {
    use kook_music_bot::common::utils::format_duration;

    assert_eq!(format_duration(0), "0:00");
    assert_eq!(format_duration(59), "0:59");
    assert_eq!(format_duration(60), "1:00");
    assert_eq!(format_duration(3661), "1:01:01");
    assert_eq!(format_duration(7200), "2:00:00");
}

#[test]
fn test_format_bytes() {
    use kook_music_bot::common::utils::format_bytes;

    assert_eq!(format_bytes(0), "0.00 B");
    assert_eq!(format_bytes(1023), "1023.00 B");
    assert_eq!(format_bytes(1024), "1.00 KB");
    assert_eq!(format_bytes(1048576), "1.00 MB");
}

#[test]
fn test_card_json_structure() {
    use kook_music_bot::common::card::{build_play_card, PlayCardData, PlayMusic, Sender};

    let data = PlayCardData::new(PlayMusic {
        title: "测试歌曲".to_string(),
        author: "测试歌手".to_string(),
        platform: "netease".to_string(),
        pic_url: "https://example.com/cover.jpg".to_string(),
        sender: Sender {
            nick_name: "测试用户".to_string(),
            avatar_url: None,
        },
    });

    let card = build_play_card(&data);
    let arr = card.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let card_obj = &arr[0];
    assert_eq!(card_obj["type"], "card");
    assert_eq!(card_obj["theme"], "info");
    assert_eq!(card_obj["size"], "lg");
    assert!(card_obj["modules"].is_array());
}

#[test]
fn test_config_defaults() {
    let config = BotConfig {
        token: "t".to_string(),
        mode: Default::default(),
        prefix: "/".to_string(),
        admins: vec![],
        webhook: Default::default(),
        audio: Default::default(),
        network: Default::default(),
        music: Default::default(),
        player: Default::default(),
        config_path: None,
    };

    assert!(config.is_admin("anyone")); // 空列表 = 所有人是管理员
    assert_eq!(config.prefix, "/");
    assert_eq!(config.audio.volume, 0.5);
    assert_eq!(config.music.cache_dir, "./cache");
    assert_eq!(config.music.netease_api_url, "http://localhost:3000");
}

#[test]
fn test_config_admin_check() {
    let config = BotConfig {
        token: "t".to_string(),
        mode: Default::default(),
        prefix: "/".to_string(),
        admins: vec!["admin1".to_string(), "admin2".to_string()],
        webhook: Default::default(),
        audio: Default::default(),
        network: Default::default(),
        music: Default::default(),
        player: Default::default(),
        config_path: None,
    };

    assert!(config.is_admin("admin1"));
    assert!(config.is_admin("admin2"));
    assert!(!config.is_admin("user3"));
}
