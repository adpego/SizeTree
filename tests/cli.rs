use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn sizetree(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sizetree"))
        .args(args)
        .output()
        .expect("failed to run sizetree")
}

/// Runs `sizetree --json --apparent-size DIR EXTRA...` and parses the report.
fn report(dir: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["--json", "--apparent-size", dir.to_str().unwrap()];
    args.extend(extra);
    let out = sizetree(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("invalid JSON")
}

fn child<'a>(node: &'a Value, name: &str) -> &'a Value {
    node["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no child named {name:?} in {node}"))
}

fn names(node: &Value) -> Vec<&str> {
    node["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect()
}

fn write(path: impl AsRef<Path>, len: usize) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; len]).unwrap();
}

/// ```text
/// big.bin            10000
/// sub/medium.bin      3000
/// sub/deeper/small.txt 100
/// tiny.txt              10
/// ```
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path().join("big.bin"), 10_000);
    write(dir.path().join("sub/medium.bin"), 3_000);
    write(dir.path().join("sub/deeper/small.txt"), 100);
    write(dir.path().join("tiny.txt"), 10);
    dir
}

#[test]
fn sums_sizes_and_sorts_largest_first() {
    let dir = fixture();
    let report = report(dir.path(), &[]);

    assert_eq!(report["size"], 13_110);
    assert_eq!(report["files"], 4);
    assert_eq!(report["directories"], 2);
    let tree = &report["tree"];
    assert_eq!(names(tree), ["big.bin", "sub", "tiny.txt"]);
    assert_eq!(child(tree, "sub")["size"], 3_100);
    assert_eq!(child(child(tree, "sub"), "deeper")["size"], 100);
}

#[test]
fn depth_limits_displayed_levels_but_not_sizes() {
    let dir = fixture();

    let tree = &report(dir.path(), &["--depth", "1"])["tree"];
    assert_eq!(names(tree), ["big.bin", "sub", "tiny.txt"]);
    assert_eq!(child(tree, "sub")["size"], 3_100);
    assert!(names(child(tree, "sub")).is_empty());

    let tree = &report(dir.path(), &["--depth", "0"])["tree"];
    assert_eq!(tree["size"], 13_110);
    assert!(names(tree).is_empty());
}

#[test]
fn filters_and_orders_entries() {
    let dir = fixture();
    let tree = |extra: &[&str]| report(dir.path(), extra)["tree"].clone();

    assert_eq!(names(&tree(&["--min-size", "1K"])), ["big.bin", "sub"]);
    assert_eq!(names(&tree(&["--top", "1"])), ["big.bin"]);
    assert_eq!(
        names(&tree(&["--sort", "name"])),
        ["big.bin", "sub", "tiny.txt"]
    );
    assert_eq!(
        names(&tree(&["--sort-name", "-r"])),
        ["tiny.txt", "sub", "big.bin"]
    );
    assert_eq!(names(&tree(&["--reverse"])), ["tiny.txt", "sub", "big.bin"]);
}

#[test]
fn excluded_entries_are_not_counted() {
    let dir = fixture();
    let report = report(dir.path(), &["-e", "sub", "--exclude", "*.txt"]);
    assert_eq!(names(&report["tree"]), ["big.bin"]);
    assert_eq!(report["size"], 10_000);
}

#[test]
fn prints_a_tree_with_a_summary() {
    let dir = fixture();
    let path = dir.path().to_str().unwrap();
    let out = sizetree(&[path, "-A", "--no-icons", "--color", "never"]);
    assert!(out.status.success());

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains('\x1b'),
        "colors must be disabled: {stdout}"
    );
    let names: Vec<String> = stdout
        .lines()
        .take_while(|line| !line.is_empty())
        .map(|line| line.chars().skip(37).collect())
        .collect();
    assert_eq!(
        names,
        [
            format!("{path}/"),
            "├── big.bin".into(),
            "├── sub/".into(),
            "│   ├── medium.bin".into(),
            "│   └── deeper/".into(),
            "│       └── small.txt".into(),
            "└── tiny.txt".into(),
        ]
    );
    assert!(stdout.contains("12.8 KiB (apparent size) in 4 files and 2 directories"));
}

#[test]
fn top_summarizes_hidden_entries() {
    let dir = fixture();
    let out = sizetree(&[
        dir.path().to_str().unwrap(),
        "-A",
        "-n",
        "1",
        "--color",
        "never",
    ]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("3.04 KiB  23.7%"), "{stdout}");
    assert!(stdout.contains("└── … 2 more entries"), "{stdout}");
}

#[test]
fn missing_path_is_a_clean_error() {
    let out = sizetree(&["/definitely/not/here"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.starts_with("error: cannot read '/definitely/not/here':"),
        "{stderr}"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn invalid_size_is_rejected_by_the_parser() {
    let out = sizetree(&["--min-size", "lots"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("invalid value 'lots' for '--min-size <SIZE>'"),
        "{stderr}"
    );
}

#[test]
fn closed_stdout_is_not_a_crash() {
    let dir = fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sizetree"))
        .arg(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());

    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn symlinks_are_listed_but_never_followed() {
    use std::os::unix::fs::symlink;

    let dir = fixture();
    symlink("big.bin", dir.path().join("link.bin")).unwrap();
    // A loop that made the old version count everything many times over.
    symlink("..", dir.path().join("sub/loop")).unwrap();

    let report = report(dir.path(), &[]);
    let link_sizes = ("big.bin".len() + "..".len()) as u64;
    assert_eq!(report["size"], 13_110 + link_sizes);

    let link = child(&report["tree"], "link.bin");
    assert_eq!(link["type"], "symlink");
    assert_eq!(link["target"], "big.bin");
    assert!(link.get("children").is_none());
}

#[cfg(unix)]
#[test]
fn hard_links_are_counted_once() {
    let dir = fixture();
    fs::hard_link(dir.path().join("big.bin"), dir.path().join("sub/same.bin")).unwrap();

    let report = report(dir.path(), &[]);
    assert_eq!(report["size"], 13_110);
    assert_eq!(report["files"], 4);
}

#[cfg(unix)]
#[test]
fn disk_usage_ignores_holes_in_sparse_files() {
    let dir = tempfile::tempdir().unwrap();
    let sparse = fs::File::create(dir.path().join("sparse.img")).unwrap();
    sparse.set_len(1 << 30).unwrap();

    let path = dir.path().to_str().unwrap();
    let apparent = &report(dir.path(), &[]);
    assert_eq!(apparent["size"], 1 << 30);
    assert_eq!(apparent["size_mode"], "apparent");

    let out = sizetree(&["--json", path]);
    let disk: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(disk["size_mode"], "disk");
    assert!(disk["size"].as_u64().unwrap() < 1 << 20, "{disk}");
}

#[cfg(unix)]
#[test]
fn unreadable_directories_produce_a_warning() {
    use std::os::unix::fs::PermissionsExt;

    let dir = fixture();
    let locked = dir.path().join("sub/deeper");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Running as root: permissions are not enforced, nothing to test.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    let path = dir.path().to_str().unwrap();
    let quiet = sizetree(&["--color", "never", path]);
    let verbose = sizetree(&["--color", "never", "--verbose", path]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(quiet.status.success());
    let stderr = String::from_utf8(quiet.stderr).unwrap();
    assert!(
        stderr.contains("warning: 1 entry could not be read"),
        "{stderr}"
    );

    let stderr = String::from_utf8(verbose.stderr).unwrap();
    assert!(stderr.contains("deeper: Permission denied"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn control_characters_in_names_are_escaped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path().join("evil\nname\x1b[31m"), 1);

    let out = sizetree(&[dir.path().to_str().unwrap(), "--color", "always"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("evil?name?[31m"), "{stdout:?}");
}
