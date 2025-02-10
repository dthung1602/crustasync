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

    let src_tree = src_fs.get_tree(true).await?;
    let dest_tree = dest_fs.get_tree(true).await?;

    let queues = build_task_queue(&src_tree, &dest_tree);

    if option.log_level <= LogLevel::INFO || option.dry_run {
        println!("\n\nSOURCE TREE:\n");
        utils::print_tree(&src_tree);
        println!("\n\nDEST TREE:\n");
        utils::print_tree(&dest_tree);
        println!("\n\nTASK QUEUES:\n");
        utils::print_task_queues(&queues);
        println!("\n\n");
    }

    if !option.dry_run {
        process_tasks(src_fs, dest_fs.clone(), &queues).await?;
        dest_fs.write_tree_to_file(&src_tree).await?;
    }

    Ok(())
}
