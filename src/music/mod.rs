pub mod downloader;
pub mod netease;
pub mod bilibili;
pub mod qqmusic;

pub use downloader::MusicDownloader;
pub use netease::{NeteaseClient, NeteaseSong, NeteaseArtist, NeteaseAlbum, QrKeyData, QrCodeData, LoginResult, PlaylistDetail};
pub use bilibili::{BilibiliClient, BilibiliSong, BilibiliAuthor};
pub use qqmusic::{QQMusicClient, QQMusicSong, QQMusicArtist, QQMusicAlbum, QQPlaylistDetail};