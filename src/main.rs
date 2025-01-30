#![recursion_limit = "256"]

use clap::Parser;
use crustasync::cli::LogLevel;
use crustasync::crustasyncfs::base::FileSystem;
use crustasync::crustasyncfs::{fs_from_location_str, FS};
use crustasync::{cli, utils};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let option = cli::CLIOption::parse();

    env_logger::Builder::new()
        .filter_level(option.log_level.level_filter())
        .init();

    if option.log_level <= LogLevel::INFO {
        utils::print_version();
    }

    let src_fs = fs_from_location_str(&option.src_dir, &option).await?;
    let dest_fs = fs_from_location_str(&option.dst_dir, &option).await?;

    match src_fs {
        FS::Local(mut fs) => fs.sync_fs_to_file().await?,
        FS::GoogleDrive(mut fs) => fs.sync_fs_to_file().await?,
    }
    match dest_fs {
        FS::Local(mut fs) => fs.sync_fs_to_file().await?,
        FS::GoogleDrive(mut fs) => fs.sync_fs_to_file().await?,
    }
    Ok(())
}
