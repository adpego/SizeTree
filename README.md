# SizeTree

**`sizetree`** shows where your disk space went: a directory tree with the
largest entries first, each with its size, its share of the total and a bar.

```console
$ sizetree -d 2 -n 3
 797 MiB 100.0% ████████████████████ 📂 .
 797 MiB 100.0% ████████████████████ ├── 📂 target
 696 MiB  87.4% █████████████████▌   │   ├── 📂 debug
77.4 MiB   9.7% ██                   │   ├── 📂 release
22.9 MiB   2.9% ▋                    │   ├── 📂 x86_64-pc-windows-gnu
12.0 KiB   0.0%                      │   └── … 3 more entries
52.0 KiB   0.0%                      ├── 📂 src
16.0 KiB   0.0%                      │   ├── 📄 render.rs
8.00 KiB   0.0%                      │   ├── 📄 main.rs
8.00 KiB   0.0%                      │   ├── 📄 scan.rs
16.0 KiB   0.0%                      │   └── … 3 more entries
36.0 KiB   0.0%                      ├── 📄 LICENSE
52.0 KiB   0.0%                      └── … 5 more entries

797 MiB (disk usage) in 3,138 files and 599 directories, scanned in 0.00s
```

In a terminal the output is colored: sizes go from green to red as they
grow, and directories, symlinks and tree guides are styled differently.

## Features

- **Fast**: the tree is walked once, in parallel on all CPU cores.
- **Accurate**: the totals are the same as `du`'s. Symlinks are listed but
  never followed, so link loops can't inflate sizes. Files with several hard
  links count once.
- **Disk usage or apparent size**: by default you see the space files really
  take on disk (sparse files and block rounding included). With
  `--apparent-size` you see file lengths instead.
- **Focused output**: limit the depth, keep only the N largest entries per
  directory, hide small entries, or exclude names with globs.
- **Scriptable**: `--json` prints the same tree as JSON. Colors turn off
  automatically when output is piped, and `NO_COLOR` is respected.
- **Honest about errors**: entries it can't read (permission denied, …)
  produce a warning instead of being dropped silently, and `--verbose` lists
  them.
- A live progress indicator shows on long scans.

## Installation

You need [Rust](https://rustup.rs) **1.88 or newer**. Check your version with
`rustc --version` and update with `rustup update` if needed.

```sh
# Latest version from GitHub
cargo install --git https://github.com/adpego/SizeTree

# Or from a local checkout
git clone https://github.com/adpego/SizeTree
cd SizeTree
cargo install --path .
```

This installs the `sizetree` binary in `~/.cargo/bin`.

## Usage

```sh
sizetree                      # current directory, the whole tree
sizetree ~/Downloads -d 1     # just the direct children
sizetree / -x -d 2 -n 10      # the 10 largest entries per directory, two
                              # levels deep, without leaving the root file system
sizetree -m 100M              # hide anything smaller than 100 MiB
sizetree -e node_modules -e '*.log'   # skip names matching these globs
sizetree -s name -d 1         # sort alphabetically
sizetree -A                   # apparent sizes (file lengths) instead of disk usage
sizetree --json -d 1 | jq '.tree.children[] | {name, size}'
```

### Options

| Option | Description |
|---|---|
| `[PATH]` | Directory (or file) to analyze. Defaults to `.` |
| `-d, --depth <N>` | Maximum depth to display. `0` shows only `PATH`; `1` adds its direct children. Sizes always include everything below |
| `-m, --min-size <SIZE>` | Hide entries smaller than `SIZE` |
| `-n, --top <N>` | Show only the `N` largest entries of each directory. The rest are grouped into a `… N more entries` line |
| `-s, --sort <size\|name>` | Sort by size (largest first, the default) or by name |
| `-r, --reverse` | Reverse the sort order |
| `-A, --apparent-size` | Report file lengths instead of disk usage |
| `-x, --one-file-system` | Don't descend into directories on other file systems (handy for `/`, which skips `/proc`, `/sys` and other mounts) |
| `-e, --exclude <GLOB>` | Skip entries whose name matches `GLOB`. Can be repeated |
| `-j, --threads <N>` | Number of scanning threads. Defaults to the number of CPUs |
| `--json` | Print the tree as JSON |
| `--color <auto\|always\|never>` | When to use colors. `auto` colors only terminals and honors `NO_COLOR` |
| `--no-icons` | Don't print the 📂 📄 🔗 icons. Directories get a trailing `/` instead |
| `-v, --verbose` | List every entry that could not be read |

Sizes take an optional, case-insensitive unit: `B`, `K`, `M`, `G`, `T` or `P`,
also written as `KB` or `KiB`. Units are always binary (1K = 1024 bytes), and
decimals and spaces are fine: `500K`, `1.5M`, `2 GiB`.

## How sizes are counted

- **Disk usage** (the default) is the space allocated on disk, as with
  `du`. It can be larger than the file length (block rounding) or much
  smaller (sparse files, such as VM images). A directory's own blocks
  count too.
- **Apparent size** (`-A`) adds up file lengths, as with
  `du --apparent-size`. A directory counts only for its contents.
- **Symlinks** are shown as `🔗 name → target` and count with their own
  size (a few bytes). Their targets are never followed. The only exception
  is `PATH` itself, so `sizetree some-link/` works as expected.
- **Hard links**: a file with several names counts once and appears under
  the first name found.
- **Errors**: entries that can't be read leave out their size, and you get a
  warning on stderr. The exit code is still 0. It is 1 if `PATH` itself
  can't be read, and 2 for invalid arguments.

On Windows, disk usage falls back to file lengths, and hard link detection
and `--one-file-system` are not available.

## JSON output

`--json` prints the tree with the same `--depth`, `--min-size`, `--top` and
sort options as the text view:

```console
$ sizetree src -A --json -d 1 -n 2
{
  "path": "src",
  "size_mode": "apparent",
  "size": 33579,
  "files": 6,
  "directories": 0,
  "errors": 0,
  "tree": {
    "name": "src",
    "type": "directory",
    "size": 33579,
    "children": [
      {
        "name": "render.rs",
        "type": "file",
        "size": 12546
      },
      {
        "name": "scan.rs",
        "type": "file",
        "size": 6825
      }
    ]
  }
}
```

- Sizes are in bytes. `size_mode` is `"disk"` or `"apparent"`.
- `files` and `directories` count everything inside `PATH`, not only what
  is displayed.
- `type` is one of `directory`, `file`, `symlink` (with a `target` field) or
  `other` (sockets, devices, FIFOs…). Only directories have `children`.

## Development

```sh
cargo test                                   # unit and integration tests
cargo clippy --all-targets -- -D warnings    # lints
cargo fmt                                    # formatting
cargo build --release                        # optimized binary in target/release/
```

CI runs these checks on Linux, macOS and Windows, and builds with the
minimum supported Rust version (1.88).

The code is split into small modules:

| File | Responsibility |
|---|---|
| `src/cli.rs` | Command line options (clap) |
| `src/scan.rs` | Parallel directory walk that builds the size tree |
| `src/render.rs` | Filtering and sorting, the text tree and JSON output |
| `src/size.rs` | Parsing and formatting human-readable sizes |
| `src/progress.rs` | Progress indicator shown during long scans |
| `tests/cli.rs` | End-to-end tests that run the real binary |

## License

[GNU Affero General Public License v3.0](LICENSE)
