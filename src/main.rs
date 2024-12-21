mod cli;
mod crustasyncfs;
mod diff;
mod oauth;
mod utils;

use std::cmp::{Ordering, PartialOrd};

use clap::Parser;
use cli::LogLevel;
use crustasyncfs::base::FileSystem;
use crustasyncfs::googledrive::GoogleDriveFileSystem;
use crustasyncfs::local::LocalFileSystem;
use hex;
use log::{debug, error, info, warn};
use tokio::io;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let option = cli::CLIOption::parse();

    env_logger::Builder::new()
        .filter_level(option.log_level.level_filter())
        .init();

    if option.log_level <= LogLevel::INFO {
        utils::print_version();
    }

    let mut drivefs = GoogleDriveFileSystem::new(&option, "/bar").await?;

    let tree = drivefs.build_tree().await?;

    utils::print_tree(&tree);

    println!("PATH_TO_ID: {:?}", drivefs.path_to_id);

    return Ok(());

    info!("Building src directory tree");
    let src_fs = LocalFileSystem::new(option.src_dir).await?;
    let src_tree = src_fs.build_tree().await?;

    if option.log_level <= LogLevel::DEBUG {
        debug!("Src directory:");
        utils::print_tree(&src_tree);
    }

    info!("Building dst directory tree");
    let dst_fs = LocalFileSystem::new(option.dst_dir).await?;
    let dst_tree = dst_fs.build_tree().await?;

    if option.log_level <= LogLevel::DEBUG {
        debug!("Dst directory:");
        utils::print_tree(&dst_tree);
    }

    let task_queues = diff::build_task_queue(&src_tree, &dst_tree);
    if option.log_level < LogLevel::DEBUG || option.dry_run {
        debug!("Tasks:");
        utils::print_task_queues(&task_queues);
    }

    if !option.dry_run {
        diff::process_tasks(src_fs, dst_fs, &task_queues).await?;
    }

    Ok(())
}
