mod cli;
mod progress;
mod render;
mod scan;
mod size;

use std::io::{self, BufWriter, IsTerminal, Write};
use std::num::NonZeroUsize;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anstream::{ColorChoice, eprintln};
use anstyle::{AnsiColor, Style};
use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::{Cli, SortKey};
use crate::render::{JsonNode, JsonReport, TextOptions, ViewOptions, group_digits};
use crate::scan::{ScanOptions, ScanResult, Stats};
use crate::size::format_size;

/// Scanning recurses once per directory level; give worker threads room for
/// very deep trees.
const SCAN_STACK_SIZE: usize = 32 * 1024 * 1024;

const ERROR: Style = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)))
    .bold();
const WARNING: Style = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)))
    .bold();
const DIM: Style = Style::new().dimmed();

fn main() -> ExitCode {
    let cli = Cli::parse();
    ColorChoice::from(cli.color).write_global();

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader went away (e.g. `sizetree | head`): not an error.
        Err(err) if is_broken_pipe(&err) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{ERROR}error:{ERROR:#} {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(cli.threads.map_or(0, NonZeroUsize::get))
        .stack_size(SCAN_STACK_SIZE)
        .build_global()
        .context("failed to start worker threads")?;

    let scan_opts = ScanOptions {
        apparent_size: cli.apparent_size,
        one_file_system: cli.one_file_system,
        max_depth: cli.depth,
        exclude: cli.exclude.clone(),
    };
    let stats = Stats::default();
    let show_progress = io::stderr().is_terminal() && io::stdout().is_terminal();

    let started = Instant::now();
    let result = progress::run_with_progress(&stats, show_progress, || {
        scan::scan(&cli.path, &scan_opts, &stats)
    })
    .with_context(|| format!("cannot read '{}'", cli.path.display()))?;
    let elapsed = started.elapsed();

    let view = ViewOptions {
        min_size: cli.min_size,
        top: cli.top,
        sort: if cli.sort_name {
            SortKey::Name
        } else {
            cli.sort
        },
        reverse: cli.reverse,
    };
    let files = stats.files.load(Ordering::Relaxed);
    let dirs = stats.dirs.load(Ordering::Relaxed);
    let size_mode = if cli.apparent_size {
        "apparent size"
    } else {
        "disk usage"
    };

    let mut out = BufWriter::new(anstream::stdout().lock());
    if cli.json {
        let report = JsonReport {
            path: &result.root.name,
            size_mode: if cli.apparent_size {
                "apparent"
            } else {
                "disk"
            },
            size: result.root.size,
            files,
            directories: dirs,
            errors: result.errors.len(),
            tree: JsonNode {
                node: &result.root,
                view: &view,
            },
        };
        serde_json::to_writer_pretty(&mut out, &report).map_err(io::Error::from)?;
        writeln!(out)?;
    } else {
        render::write_tree(
            &mut out,
            &result.root,
            &view,
            &TextOptions {
                icons: !cli.no_icons,
            },
        )?;
        writeln!(
            out,
            "\n{DIM}{} ({size_mode}) in {} and {}, scanned in {:.2}s{DIM:#}",
            format_size(result.root.size),
            plural(files, "file", "files"),
            plural(dirs, "directory", "directories"),
            elapsed.as_secs_f64(),
        )?;
    }
    out.flush()?;

    report_errors(&result, cli.verbose);
    Ok(())
}

fn report_errors(result: &ScanResult, verbose: bool) {
    let count = result.errors.len();
    if count == 0 {
        return;
    }
    if verbose {
        for e in &result.errors {
            eprintln!(
                "{WARNING}warning:{WARNING:#} {}: {}",
                e.path.display(),
                e.error
            );
        }
    } else {
        eprintln!(
            "{WARNING}warning:{WARNING:#} {} could not be read, so sizes may be incomplete \
             (use --verbose to list them)",
            plural(count as u64, "entry", "entries"),
        );
    }
}

fn plural(count: u64, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        group_digits(count),
        if count == 1 { one } else { many }
    )
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|e| e.downcast_ref::<io::Error>())
        .any(|e| e.kind() == io::ErrorKind::BrokenPipe)
}
