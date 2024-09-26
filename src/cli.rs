use crate::enum_str;
use clap::{Parser, ValueEnum};
use log::LevelFilter;
use std::str::FromStr;

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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CLIOption {
    #[arg(value_name = "SRC_DIR", index = 1)]
    pub src_dir: String,

    #[arg(value_name = "DST_DIR", index = 2)]
    pub dst_dir: String,

    #[arg(long, action)]
    pub dry_run: bool,

    #[arg(long, value_enum, default_value = "info")]
    pub log_level: LogLevel,
}
