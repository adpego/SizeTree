use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::size::parse_size;

/// Display disk usage as a tree, largest entries first.
#[derive(Parser, Debug)]
#[command(name = "sizetree", version, about, max_term_width = 100)]
pub struct Cli {
    /// Directory (or file) to analyze
    #[arg(default_value = ".", value_name = "PATH")]
    pub path: PathBuf,

    /// Maximum depth to display (0 shows only PATH itself)
    #[arg(short, long, value_name = "N")]
    pub depth: Option<usize>,

    /// Hide entries smaller than SIZE (e.g. 500K, 1.5M, 2GiB)
    #[arg(short, long, value_name = "SIZE", default_value = "0", value_parser = parse_size)]
    pub min_size: u64,

    /// Show only the N largest entries of each directory
    #[arg(short = 'n', long, value_name = "N")]
    pub top: Option<NonZeroUsize>,

    /// Sort entries by size (largest first) or by name
    #[arg(short, long, value_enum, value_name = "KEY", default_value_t = SortKey::Size)]
    pub sort: SortKey,

    /// Reverse the sort order
    #[arg(short, long)]
    pub reverse: bool,

    /// Report apparent sizes (file lengths) instead of disk usage
    #[arg(short = 'A', long)]
    pub apparent_size: bool,

    /// Don't descend into directories on other file systems
    #[arg(short = 'x', long)]
    pub one_file_system: bool,

    /// Skip entries whose name matches GLOB (can be repeated)
    #[arg(short, long, value_name = "GLOB", value_parser = glob::Pattern::new)]
    pub exclude: Vec<glob::Pattern>,

    /// Number of threads to scan with [default: number of CPUs]
    #[arg(short = 'j', long, value_name = "N")]
    pub threads: Option<NonZeroUsize>,

    /// Print the tree as JSON
    #[arg(long)]
    pub json: bool,

    /// When to use colors
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,

    /// Don't print file type icons
    #[arg(long)]
    pub no_icons: bool,

    /// List every entry that could not be read
    #[arg(short, long)]
    pub verbose: bool,

    /// Same as `--sort name` (kept for compatibility)
    #[arg(long, hide = true)]
    pub sort_name: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SortKey {
    Size,
    Name,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

impl From<ColorWhen> for anstream::ColorChoice {
    fn from(when: ColorWhen) -> Self {
        match when {
            ColorWhen::Auto => Self::Auto,
            ColorWhen::Always => Self::Always,
            ColorWhen::Never => Self::Never,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
