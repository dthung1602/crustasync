use std::usize;
use unicode_width::UnicodeWidthStr;
use crate::crustasyncfs::base::Node;
use crate::diff::Task;

// ------------------------------
// region Print
// ------------------------------

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

pub fn print_version() {
    let name = r#"                     _
  ___ _ __ _   _ ___| |_ __ _ ___ _   _ _ __   ___
 / __| '__| | | / __| __/ _` / __| | | | '_ \ / __|
| (__| |  | |_| \__ \ || (_| \__ \ |_| | | | | (__
 \___|_|   \__,_|___/\__\__,_|___/\__, |_| |_|\___|
                                  |___/
"#
    .to_string()
    .rgb(231, 112, 13);
    println!("{name}");
    println!("Crustasync - {DESCRIPTION}\nVersion {VERSION}");
    println!("{}", String::from(" ").default());
}

pub fn print_task_queues(queues: &[Vec<Task>]) {
    for (i, queue) in queues.iter().enumerate() {
        println!("---- Priority task queue {i} ----");
        for task in queue {
            println!(" {:?}", task)
        }
    }
}

pub fn print_tree(node: &Node) {
    print_node_with_level(node, 0);
    println!();
}

const PRINT_LINE_WIDTH: usize = 128;

pub fn print_node_with_level(node: &Node, level: usize) {
    let left_padding = ' '.to_string().repeat(level * 4);
    let mut node_name = node.name.as_str();
    if node_name.width() > PRINT_LINE_WIDTH - 12 {
        node_name = &node.name[..PRINT_LINE_WIDTH - 12];
    }
    let encoded = hex::encode(&node.content_hash[0..4]);
    let mut right_padding_len =
        PRINT_LINE_WIDTH - left_padding.width() - node_name.width() - encoded.width();
    let colored_node_name = if node.is_dir() {
        right_padding_len -= 1;
        format!("*{}", node.name.rgb(138, 173, 244))
    } else {
        node.name.default()
    };
    if right_padding_len > PRINT_LINE_WIDTH {
        // prevent overflow
        right_padding_len = 1;
    }
    let right_padding = ' '.to_string().repeat(right_padding_len);
    println!("{left_padding}{colored_node_name}{right_padding}{encoded}");

    if node.is_dir() {
        let level = level + 1;
        for child in &node.children {
            print_node_with_level(child, level)
        }
    }
}

pub trait RGBColorTextExt {
    fn rgb(&self, r: u8, g: u8, b: u8) -> String;
    fn default(&self) -> String;
}

impl RGBColorTextExt for String {
    fn rgb(&self, r: u8, g: u8, b: u8) -> String {
        format!("\x1b[38;2;{r};{g};{b}m{self}")
    }

    fn default(&self) -> String {
        format!("\x1b[39m{self}")
    }
}
// endregion

// ------------------------------
// region Macro
// ------------------------------

// Add `name` method to enum variant
#[macro_export]
macro_rules! enum_str {
    // basic version
    (
        enum $name:ident {
            $($variant:ident = $val:expr),*
            $(,)* // optional trailing comma
        }
    ) => {
        enum $name {
            $($variant = $val),*
        }

        impl $name {
            fn name(&self) -> &'static str {
                match self {
                    $($name::$variant => stringify!($variant)),*
                }
            }
        }
    };
    // enum with #[derive]
    (
        #[derive($($der:ident),* $(,)*)]
        $vis:vis enum $name:ident {
            $($variant:ident = $val:expr),*
            $(,)* // optional trailing comma
        }
    ) => {
        #[derive($($der),*)]
        $vis enum $name {
            $($variant = $val),*
        }

        impl $name {
            pub fn name(&self) -> &'static str {
                match self {
                    $($name::$variant => stringify!($variant)),*
                }
            }
        }
    };
}

// endregion
