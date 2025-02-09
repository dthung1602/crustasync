#![recursion_limit = "256"]

use clap::Parser;
use crustasync::cli::LogLevel;
use crustasync::crustasyncfs::fs_from_location_str;
use crustasync::diff::{build_task_queue, process_tasks};
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

    let src_tree = src_fs.read_fs_from_file().await?;
    let dest_tree = dest_fs.read_fs_from_file().await?;

    let queues = build_task_queue(&src_tree, &dest_tree);

    if option.log_level <= LogLevel::INFO {
        utils::print_task_queues(&queues);
    }

    process_tasks(src_fs, dest_fs, &queues).await?;

    Ok(())
}
