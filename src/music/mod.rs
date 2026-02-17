pub mod downloader;
pub mod netease;

pub use downloader::MusicDownloader;
pub use netease::{NeteaseClient, NeteaseSong, NeteasePlaylist};