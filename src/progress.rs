//! A spinner on stderr while a slow scan is running.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use crate::render::group_digits;
use crate::scan::Stats;

const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
/// Scans that finish faster than this never show the spinner, to avoid flicker.
const SHOW_AFTER: Duration = Duration::from_millis(300);
const TICK: Duration = Duration::from_millis(100);

/// Runs `work`, showing live counters from `stats` if `enabled`.
pub fn run_with_progress<T>(stats: &Stats, enabled: bool, work: impl FnOnce() -> T) -> T {
    if !enabled {
        return work();
    }

    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        let spinner = scope.spawn(|| spin(stats, &done));
        // Stops the spinner even if `work` panics, so the scope can be joined.
        let _stop = Stop {
            done: &done,
            thread: spinner.thread().clone(),
        };
        work()
    })
}

struct Stop<'a> {
    done: &'a AtomicBool,
    thread: Thread,
}

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

fn spin(stats: &Stats, done: &AtomicBool) {
    let start = Instant::now();
    let mut stderr = io::stderr();
    let mut shown = false;

    for frame in FRAMES.iter().cycle() {
        thread::park_timeout(TICK);
        if done.load(Ordering::Acquire) {
            break;
        }
        if start.elapsed() < SHOW_AFTER {
            continue;
        }
        let files = group_digits(stats.files.load(Ordering::Relaxed));
        let dirs = group_digits(stats.dirs.load(Ordering::Relaxed));
        let _ = write!(
            stderr,
            "\r\x1b[2K{frame} Scanning… {files} files, {dirs} directories"
        );
        let _ = stderr.flush();
        shown = true;
    }

    if shown {
        let _ = write!(stderr, "\r\x1b[2K");
        let _ = stderr.flush();
    }
}
