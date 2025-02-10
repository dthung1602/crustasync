use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{Parser, ValueEnum};
use log::LevelFilter;

use crate::enum_str;

enum_str! {
    #[derive(ValueEnum, Debug, Clone, PartialOrd, PartialEq)]
    pub enum LogLevel {
        ERROR = 3,
        WARN = 2,
        INFO = 1,
        DEBUG = 0,
    }
}

impl LogLevel {
    pub fn level_filter(&self) -> LevelFilter {
        LevelFilter::from_str(self.name()).unwrap()
    }
}

fn default_cfg_path() -> OsString {
    // TODO support window
    let path = PathBuf::from_iter([env::var("HOME").unwrap().as_str(), ".config/crustasync"]);
    path.into_os_string()
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CLIOption {
    #[arg(
        value_name = "SRC_DIR",
        index = 1,
        help = "Source directory.\
                \nCan be relative or absolute local path.\
                \nUse prefix `gd:` to indicate a GoogleDrive directory"
    )]
    pub src_dir: String,

    #[arg(
        value_name = "DST_DIR",
        index = 2,
        help = "Destination directory, same format as SRC_DIR"
    )]
    pub dst_dir: String,

    #[arg(long, action)]
    pub dry_run: bool,

    #[arg(long, value_enum, default_value = "info")]
    pub log_level: LogLevel,

    #[arg(long, short, default_value = default_cfg_path())]
    pub config_dir: PathBuf,
}
