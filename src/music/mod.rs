pub mod downloader;
pub mod netease;

pub use downloader::MusicDownloader;
pub use netease::{NeteaseClient, NeteaseSong, NeteaseArtist, NeteaseAlbum, QrKeyData, QrCodeData, LoginResult};