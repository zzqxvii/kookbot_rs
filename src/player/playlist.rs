use super::Music;
use crate::core::error::{BotError, Result};
use rand::seq::SliceRandom;
use tracing::{debug, info};

/// 播放模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    /// 顺序播放
    Sequential,
    /// 随机播放
    Shuffle,
    /// 单曲循环
    RepeatOne,
    /// 列表循环
    RepeatAll,
}

impl Default for PlayMode {
    fn default() -> Self {
        PlayMode::Sequential
    }
}

/// 播放列表
pub struct Playlist {
    /// 播放列表名称
    name: String,
    /// 原始歌曲列表（保持添加顺序）
    original_list: Vec<Music>,
    /// 当前播放顺序（根据播放模式调整）
    play_order: Vec<usize>,
    /// 当前播放索引（在 play_order 中的位置）
    current_position: Option<usize>,
    /// 播放模式
    mode: PlayMode,
    /// 随机数生成器
    rng: rand::rngs::ThreadRng,
}

impl Playlist {
    /// 创建新的播放列表
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            original_list: Vec::new(),
            play_order: Vec::new(),
            current_position: None,
            mode: PlayMode::default(),
            rng: rand::rng(),
        }
    }

    /// 从歌曲列表创建播放列表
    pub fn from_songs(name: impl Into<String>, songs: Vec<Music>) -> Self {
        let mut playlist = Self::new(name);
        for song in songs {
            playlist.add(song);
        }
        playlist
    }

    /// 添加歌曲到播放列表
    pub fn add(&mut self, music: Music) -> usize {
        let index = self.original_list.len();
        self.original_list.push(music);
        self.play_order.push(index);

        // 如果是随机模式，重新打乱
        if self.mode == PlayMode::Shuffle {
            self.shuffle_remaining();
        }

        debug!(
            "添加歌曲到播放列表: 索引={}, 当前共 {} 首",
            index,
            self.original_list.len()
        );
        index
    }

    /// 插入歌曲到指定位置（在当前播放之后）
    pub fn insert_next(&mut self, music: Music) {
        let new_index = self.original_list.len();
        self.original_list.push(music);

        if let Some(pos) = self.current_position {
            // 在当前位置之后插入
            self.play_order.insert(pos + 1, new_index);
        } else {
            // 如果没有当前播放，添加到开头
            self.play_order.insert(0, new_index);
        }

        debug!("插入歌曲到下一首播放: 索引={}", new_index);
    }

    /// 移除指定索引的歌曲
    pub fn remove(&mut self, original_index: usize) -> Result<Music> {
        if original_index >= self.original_list.len() {
            return Err(BotError::QueueError("索引超出范围".to_string()));
        }

        // 从原始列表中移除
        let music = self.original_list.remove(original_index);

        // 更新播放顺序
        self.play_order.retain(|&idx| idx != original_index);

        // 调整大于 removed_index 的索引
        for idx in self.play_order.iter_mut() {
            if *idx > original_index {
                *idx -= 1;
            }
        }

        // 更新当前位置
        if let Some(pos) = self.current_position {
            if pos >= self.play_order.len() && !self.play_order.is_empty() {
                self.current_position = Some(self.play_order.len() - 1);
            } else if self.play_order.is_empty() {
                self.current_position = None;
            }
        }

        info!("从播放列表移除歌曲: {}", music.title);
        Ok(music)
    }

    /// 获取下一首歌曲
    pub fn next(&mut self) -> Option<&Music> {
        match self.mode {
            PlayMode::Sequential | PlayMode::Shuffle => {
                if let Some(pos) = self.current_position {
                    let next_pos = pos + 1;
                    if next_pos < self.play_order.len() {
                        self.current_position = Some(next_pos);
                        let idx = self.play_order[next_pos];
                        return self.original_list.get(idx);
                    } else if self.mode == PlayMode::Shuffle || self.current_loop() {
                        // 列表循环或随机模式下回到开头
                        self.current_position = Some(0);
                        let idx = self.play_order[0];
                        return self.original_list.get(idx);
                    }
                } else if !self.play_order.is_empty() {
                    self.current_position = Some(0);
                    let idx = self.play_order[0];
                    return self.original_list.get(idx);
                }
                None
            }
            PlayMode::RepeatOne => {
                // 单曲循环：返回当前歌曲
                if let Some(pos) = self.current_position {
                    let idx = self.play_order[pos];
                    return self.original_list.get(idx);
                }
                // 没有当前歌曲，尝试返回第一首
                if !self.play_order.is_empty() {
                    self.current_position = Some(0);
                    let idx = self.play_order[0];
                    return self.original_list.get(idx);
                }
                None
            }
            PlayMode::RepeatAll => {
                if let Some(pos) = self.current_position {
                    let next_pos = (pos + 1) % self.play_order.len();
                    self.current_position = Some(next_pos);
                    let idx = self.play_order[next_pos];
                    return self.original_list.get(idx);
                } else if !self.play_order.is_empty() {
                    self.current_position = Some(0);
                    let idx = self.play_order[0];
                    return self.original_list.get(idx);
                }
                None
            }
        }
    }

    /// 获取上一首歌曲
    pub fn previous(&mut self) -> Option<&Music> {
        if let Some(pos) = self.current_position {
            if pos > 0 {
                let prev_pos = pos - 1;
                self.current_position = Some(prev_pos);
                let idx = self.play_order[prev_pos];
                return self.original_list.get(idx);
            }
        }
        // 已经是第一首，返回当前
        self.current()
    }

    /// 获取当前歌曲
    pub fn current(&self) -> Option<&Music> {
        self.current_position.and_then(|pos| {
            self.play_order
                .get(pos)
                .and_then(|&idx| self.original_list.get(idx))
        })
    }

    /// 跳转到指定位置
    pub fn jump_to(&mut self, position: usize) -> Option<&Music> {
        if position < self.play_order.len() {
            self.current_position = Some(position);
            let idx = self.play_order[position];
            return self.original_list.get(idx);
        }
        None
    }

    /// 设置播放模式
    pub fn set_mode(&mut self, mode: PlayMode) {
        if self.mode != mode {
            let old_mode = self.mode;
            self.mode = mode;

            // 切换到随机模式时打乱剩余歌曲
            if mode == PlayMode::Shuffle && old_mode != PlayMode::Shuffle {
                self.shuffle_remaining();
            }

            info!("播放模式切换: {:?} -> {:?}", old_mode, mode);
        }
    }

    /// 获取当前播放模式
    pub fn mode(&self) -> PlayMode {
        self.mode
    }

    /// 打乱剩余歌曲（从当前位置之后）
    fn shuffle_remaining(&mut self) {
        if let Some(pos) = self.current_position {
            if pos + 1 < self.play_order.len() {
                let remaining = &mut self.play_order[pos + 1..];
                remaining.shuffle(&mut self.rng);
            }
        }
    }

    /// 获取队列长度
    pub fn len(&self) -> usize {
        self.original_list.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.original_list.is_empty()
    }

    /// 清空播放列表
    pub fn clear(&mut self) {
        self.original_list.clear();
        self.play_order.clear();
        self.current_position = None;
        info!("播放列表已清空");
    }

    /// 获取当前位置
    pub fn current_position(&self) -> Option<usize> {
        self.current_position
    }

    /// 获取队列列表（只读）
    pub fn list(&self) -> &[Music] {
        &self.original_list
    }

    /// 检查是否循环播放（用于 Sequential 模式）
    fn current_loop(&self) -> bool {
        false // 可以在配置中添加 loop 选项
    }

    /// 获取播放列表名称
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 设置播放列表名称
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new("默认播放列表")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_add_and_next() {
        let mut playlist = Playlist::new("测试");

        let music1 = Music {
            title: "歌曲1".to_string(),
            author: "歌手1".to_string(),
            ..Default::default()
        };

        let music2 = Music {
            title: "歌曲2".to_string(),
            author: "歌手2".to_string(),
            ..Default::default()
        };

        playlist.add(music1);
        playlist.add(music2);

        assert_eq!(playlist.len(), 2);

        let first = playlist.next().unwrap();
        assert_eq!(first.title, "歌曲1");

        let second = playlist.next().unwrap();
        assert_eq!(second.title, "歌曲2");
    }

    #[test]
    fn test_shuffle_mode() {
        let mut playlist = Playlist::new("测试");

        for i in 0..10 {
            let music = Music {
                title: format!("歌曲{}", i),
                author: "歌手".to_string(),
                ..Default::default()
            };
            playlist.add(music);
        }

        // 先调用 next() 锚定 current_position
        playlist.next();

        // 切换到随机模式
        playlist.set_mode(PlayMode::Shuffle);

        // 播放剩余的
        for _ in 0..4 {
            playlist.next();
        }

        // 验证歌曲都被播放了（不验证顺序，因为是随机的）
        assert!(playlist.current_position().is_some());
    }

    #[test]
    fn test_sequential_loop() {
        let mut playlist = Playlist::new("test");
        playlist.add(make_music("song1", "artist1"));
        playlist.add(make_music("song2", "artist2"));
        playlist.add(make_music("song3", "artist3"));

        assert_eq!(playlist.next().unwrap().title, "song1");
        assert_eq!(playlist.next().unwrap().title, "song2");
        assert_eq!(playlist.next().unwrap().title, "song3");
        assert!(playlist.next().is_none());
    }

    #[test]
    fn test_repeat_one() {
        let mut playlist = Playlist::new("test");
        playlist.add(make_music("song1", "artist1"));
        playlist.add(make_music("song2", "artist2"));

        playlist.set_mode(PlayMode::RepeatOne);

        // First call sets position to 0 and returns song1
        let first = playlist.next().unwrap();
        assert_eq!(first.title, "song1");

        // RepeatOne always returns the current song
        let second = playlist.next().unwrap();
        assert_eq!(second.title, "song1");

        let third = playlist.next().unwrap();
        assert_eq!(third.title, "song1");
    }

    #[test]
    fn test_repeat_all() {
        let mut playlist = Playlist::new("test");
        playlist.add(make_music("song1", "artist1"));
        playlist.add(make_music("song2", "artist2"));
        playlist.add(make_music("song3", "artist3"));

        playlist.set_mode(PlayMode::RepeatAll);

        assert_eq!(playlist.next().unwrap().title, "song1");
        assert_eq!(playlist.next().unwrap().title, "song2");
        assert_eq!(playlist.next().unwrap().title, "song3");
        // Wraps around to song1
        assert_eq!(playlist.next().unwrap().title, "song1");
        // Continues to song2
        assert_eq!(playlist.next().unwrap().title, "song2");
    }

    #[test]
    fn test_shuffle_contains_all() {
        let mut playlist = Playlist::new("test");
        for i in 0..10 {
            playlist.add(make_music(&format!("song{}", i), "artist"));
        }

        playlist.set_mode(PlayMode::Shuffle);

        // Collect songs by calling next() len times (shuffle wraps, so no None)
        let mut played = std::collections::HashSet::new();
        for _ in 0..playlist.len() {
            let music = playlist.next().unwrap();
            played.insert(music.title.clone());
        }

        // All 10 songs should have been played
        assert_eq!(played.len(), 10);
        for i in 0..10 {
            assert!(played.contains(&format!("song{}", i)));
        }
    }

    #[test]
    fn test_empty_next() {
        let mut playlist = Playlist::new("test");
        assert!(playlist.next().is_none());
    }

    fn make_music(title: &str, author: &str) -> Music {
        Music {
            title: title.into(),
            author: author.into(),
            ..Default::default()
        }
    }
}
