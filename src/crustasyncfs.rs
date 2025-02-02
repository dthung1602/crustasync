use std::path::PathBuf;
use std::sync::Arc;

use crate::cli::CLIOption;
use crate::crustasyncfs::base::FileSystem;
use crate::error::Result;
pub mod base;
pub mod googledrive;
pub mod local;

pub async fn fs_from_location_str(
    location: &str,
    opt: &CLIOption,
) -> Result<Arc<dyn FileSystem + Send + Sync>> {
    if location.starts_with("gd:") {
        let path_buf = PathBuf::from(location.trim_start_matches("gd:"));
        let fs = googledrive::GoogleDriveFileSystem::new(opt, &path_buf).await?;
        Ok(Arc::new(fs))
    } else {
        let fs = local::LocalFileSystem::new(location.as_ref()).await?;
        Ok(Arc::new(fs))
    }
}
