//! Turns a scanned [`Node`] tree into a colored text tree or JSON.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::io::{self, Write};
use std::num::NonZeroUsize;

use anstyle::{AnsiColor, Style};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use crate::cli::SortKey;
use crate::scan::{Kind, Node};
use crate::size::format_size;

const BAR_WIDTH: usize = 20;
const BAR_PARTIALS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// Which children are shown, and in what order.
pub struct ViewOptions {
    pub min_size: u64,
    pub top: Option<NonZeroUsize>,
    pub sort: SortKey,
    pub reverse: bool,
}

pub struct TextOptions {
    pub icons: bool,
}

struct Listing<'a> {
    shown: Vec<&'a Node>,
    /// Entries left out by `--top`: how many, and their combined size.
    omitted: Option<(usize, u64)>,
}

fn listing<'a>(node: &'a Node, opts: &ViewOptions) -> Listing<'a> {
    let mut shown: Vec<&Node> = node
        .children
        .iter()
        .filter(|child| child.size >= opts.min_size)
        .collect();

    let mut omitted = None;
    if let Some(top) = opts.top
        && shown.len() > top.get()
    {
        shown.sort_by(|a, b| by_size(a, b));
        let rest = shown.split_off(top.get());
        omitted = Some((rest.len(), rest.iter().map(|n| n.size).sum()));
    }

    match opts.sort {
        SortKey::Size => shown.sort_by(|a, b| by_size(a, b)),
        SortKey::Name => shown.sort_by(|a, b| by_name(a, b)),
    }
    if opts.reverse {
        shown.reverse();
    }
    Listing { shown, omitted }
}

fn by_size(a: &Node, b: &Node) -> Ordering {
    b.size.cmp(&a.size).then_with(|| by_name(a, b))
}

fn by_name(a: &Node, b: &Node) -> Ordering {
    a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.name.cmp(&b.name))
}

struct Theme {
    guide: Style,
    dim: Style,
    dir: Style,
    symlink: Style,
    other: Style,
}

const THEME: Theme = Theme {
    guide: Style::new().dimmed(),
    dim: Style::new().dimmed(),
    dir: Style::new()
        .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Blue)))
        .bold(),
    symlink: Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan))),
    other: Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow))),
};

/// Bigger units get warmer colors, so large entries stand out at a glance.
fn size_style(bytes: u64) -> Style {
    let color = match bytes {
        0..0x400 => return THEME.dim,
        0x400..0x10_0000 => AnsiColor::Green,
        0x10_0000..0x4000_0000 => AnsiColor::Yellow,
        0x4000_0000.. => AnsiColor::Red,
    };
    Style::new().fg_color(Some(color.into()))
}

pub fn write_tree(
    out: &mut impl Write,
    root: &Node,
    view: &ViewOptions,
    text: &TextOptions,
) -> io::Result<()> {
    let mut tree = TreeWriter {
        out,
        view,
        text,
        total: root.size,
        prefix: String::new(),
    };
    tree.columns(root.size)?;
    tree.name(root)?;
    tree.children(root)
}

struct TreeWriter<'a, W> {
    out: &'a mut W,
    view: &'a ViewOptions,
    text: &'a TextOptions,
    total: u64,
    prefix: String,
}

impl<W: Write> TreeWriter<'_, W> {
    fn children(&mut self, node: &Node) -> io::Result<()> {
        let Listing { shown, omitted } = listing(node, self.view);

        for (i, child) in shown.iter().enumerate() {
            let last = i + 1 == shown.len() && omitted.is_none();
            let (branch, indent) = if last {
                ("└── ", "    ")
            } else {
                ("├── ", "│   ")
            };

            self.columns(child.size)?;
            write!(
                self.out,
                "{}{}{branch}{:#}",
                THEME.guide, self.prefix, THEME.guide
            )?;
            self.name(child)?;

            if !child.children.is_empty() {
                let len = self.prefix.len();
                self.prefix.push_str(indent);
                self.children(child)?;
                self.prefix.truncate(len);
            }
        }

        if let Some((count, size)) = omitted {
            let entries = if count == 1 { "entry" } else { "entries" };
            self.columns(size)?;
            writeln!(
                self.out,
                "{}{}└── {:#}{}… {count} more {entries}{:#}",
                THEME.guide, self.prefix, THEME.guide, THEME.dim, THEME.dim
            )?;
        }
        Ok(())
    }

    /// Size, percentage of the total and bar: a fixed-width left margin, so
    /// the tree stays aligned no matter how long the names are.
    fn columns(&mut self, size: u64) -> io::Result<()> {
        let fraction = if self.total == 0 {
            0.0
        } else {
            size as f64 / self.total as f64
        };
        let style = size_style(size);
        write!(
            self.out,
            "{style}{:>8}{style:#} {}{:>5.1}%{:#} {style}{}{style:#} ",
            format_size(size),
            THEME.dim,
            fraction * 100.0,
            THEME.dim,
            bar(fraction),
        )
    }

    fn name(&mut self, node: &Node) -> io::Result<()> {
        let (icon, style) = match node.kind {
            Kind::Dir => ("📂 ", THEME.dir),
            Kind::File => ("📄 ", Style::new()),
            Kind::Symlink { .. } => ("🔗 ", THEME.symlink),
            Kind::Other => ("⚙️ ", THEME.other),
        };
        let icon = if self.text.icons { icon } else { "" };
        // Without icons, a trailing slash is what tells directories apart.
        let slash = if matches!(node.kind, Kind::Dir) && !self.text.icons {
            "/"
        } else {
            ""
        };

        write!(
            self.out,
            "{icon}{style}{}{slash}{style:#}",
            printable(&node.name)
        )?;
        if let Kind::Symlink {
            target: Some(target),
        } = &node.kind
        {
            write!(
                self.out,
                " {}→ {}{:#}",
                THEME.dim,
                printable(target),
                THEME.dim
            )?;
        }
        writeln!(self.out)
    }
}

/// Replaces control characters (newlines, escape sequences…) with `?`, like
/// `ls` does, so odd file names can't break the layout or the terminal.
fn printable(name: &str) -> Cow<'_, str> {
    if name.contains(char::is_control) {
        name.chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect()
    } else {
        Cow::Borrowed(name)
    }
}

/// Renders `fraction` (0.0..=1.0) as a bar with 1/8 character precision.
fn bar(fraction: f64) -> String {
    let eighths = (fraction.clamp(0.0, 1.0) * (BAR_WIDTH * 8) as f64).round() as usize;
    let (full, partial) = (eighths / 8, eighths % 8);

    let mut bar = "█".repeat(full);
    if full < BAR_WIDTH {
        bar.push(BAR_PARTIALS[partial]);
        bar.push_str(&" ".repeat(BAR_WIDTH - full - 1));
    }
    bar
}

/// Formats an integer with thousands separators: `1234567` -> `1,234,567`.
pub fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Serialize)]
pub struct JsonReport<'a> {
    pub path: &'a str,
    pub size_mode: &'static str,
    pub size: u64,
    pub files: u64,
    pub directories: u64,
    pub errors: usize,
    pub tree: JsonNode<'a>,
}

/// Serializes a node with the same filtering and ordering as the text tree.
pub struct JsonNode<'a> {
    pub node: &'a Node,
    pub view: &'a ViewOptions,
}

impl Serialize for JsonNode<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let node = self.node;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", &node.name)?;
        map.serialize_entry("type", node.kind.as_str())?;
        map.serialize_entry("size", &node.size)?;
        if let Kind::Symlink {
            target: Some(target),
        } = &node.kind
        {
            map.serialize_entry("target", target)?;
        }
        if matches!(node.kind, Kind::Dir) {
            let children: Vec<_> = listing(node, self.view)
                .shown
                .into_iter()
                .map(|node| JsonNode {
                    node,
                    view: self.view,
                })
                .collect();
            map.serialize_entry("children", &children)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> Node {
        Node {
            name: name.into(),
            size,
            kind: Kind::File,
            children: Vec::new(),
        }
    }

    fn dir(name: &str, children: Vec<Node>) -> Node {
        let size = children.iter().map(|c| c.size).sum();
        Node {
            name: name.into(),
            size,
            kind: Kind::Dir,
            children,
        }
    }

    fn view(sort: SortKey) -> ViewOptions {
        ViewOptions {
            min_size: 0,
            top: None,
            sort,
            reverse: false,
        }
    }

    fn names<'a>(listing: &Listing<'a>) -> Vec<&'a str> {
        listing.shown.iter().map(|n| n.name.as_str()).collect()
    }

    #[test]
    fn sorts_by_size_then_name() {
        let root = dir(
            "r",
            vec![file("b", 1), file("a", 1), file("big", 9), file("C", 1)],
        );
        assert_eq!(
            names(&listing(&root, &view(SortKey::Size))),
            ["big", "a", "b", "C"]
        );
        assert_eq!(
            names(&listing(&root, &view(SortKey::Name))),
            ["a", "b", "big", "C"]
        );

        let reversed = ViewOptions {
            reverse: true,
            ..view(SortKey::Size)
        };
        assert_eq!(names(&listing(&root, &reversed)), ["C", "b", "a", "big"]);
    }

    #[test]
    fn top_keeps_largest_and_summarizes_the_rest() {
        let root = dir(
            "r",
            vec![file("a", 5), file("b", 50), file("c", 20), file("d", 1)],
        );
        let opts = ViewOptions {
            top: NonZeroUsize::new(2),
            ..view(SortKey::Name)
        };
        let result = listing(&root, &opts);
        assert_eq!(names(&result), ["b", "c"]);
        assert_eq!(result.omitted, Some((2, 6)));
    }

    #[test]
    fn min_size_hides_small_entries() {
        let root = dir("r", vec![file("a", 5), file("b", 50)]);
        let opts = ViewOptions {
            min_size: 6,
            ..view(SortKey::Size)
        };
        let result = listing(&root, &opts);
        assert_eq!(names(&result), ["b"]);
        assert_eq!(result.omitted, None);
    }

    #[test]
    fn bars_have_fixed_width() {
        assert_eq!(bar(0.0), " ".repeat(BAR_WIDTH));
        assert_eq!(bar(1.0), "█".repeat(BAR_WIDTH));
        assert_eq!(bar(0.5), format!("{}{}", "█".repeat(10), " ".repeat(10)));
        assert_eq!(bar(1.0 / 160.0), format!("▏{}", " ".repeat(19)));
        for i in 0..=1000 {
            assert_eq!(bar(i as f64 / 1000.0).chars().count(), BAR_WIDTH);
        }
    }

    #[test]
    fn escapes_control_characters() {
        assert_eq!(printable("plain name"), "plain name");
        assert_eq!(printable("evil\nname\x1b[31m"), "evil?name?[31m");
    }

    #[test]
    fn groups_digits() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(1234567), "1,234,567");
    }

    #[test]
    fn writes_plain_tree() {
        let root = dir(
            "root",
            vec![dir("sub", vec![file("x", 300)]), file("y", 100)],
        );
        let mut out = Vec::new();
        write_tree(
            &mut out,
            &root,
            &view(SortKey::Size),
            &TextOptions { icons: false },
        )
        .unwrap();
        let text = anstream::adapter::strip_str(std::str::from_utf8(&out).unwrap()).to_string();
        let lines: Vec<String> = text.lines().map(|l| l.chars().skip(37).collect()).collect();
        assert_eq!(lines, ["root/", "├── sub/", "│   └── x", "└── y"]);
        assert!(text.starts_with("   400 B 100.0% ████████████████████ root/"));
    }
}
