use std::path::PathBuf;
use crate::cli::CLIOption;
use crate::error::Result;
pub mod base;
pub mod googledrive;
pub mod local;

// TODO make dyn type instead of this
pub enum FS {
    Local(local::LocalFileSystem),
    GoogleDrive(googledrive::GoogleDriveFileSystem),
}


pub async fn fs_from_location_str(location: &str, opt: &CLIOption) -> Result<FS> {
    if location.starts_with("gd:") {
        let path_buf = PathBuf::from(location.trim_start_matches("gd:"));
        let fs = googledrive::GoogleDriveFileSystem::new(opt, path_buf).await?;
        Ok(FS::GoogleDrive(fs))
    } else {
        let fs = local::LocalFileSystem::new(location).await?;
        Ok(FS::Local(fs))
    }
}
