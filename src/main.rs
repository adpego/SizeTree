use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use clap::{Parser, ArgAction};
use std::fmt;

// Define command line arguments using clap
#[derive(Parser, Debug)]
#[command(name = "sizetree")]
#[command(about = "Display directory sizes in a tree-like format", long_about = None)]
struct Args {
    /// Directory to analyze (defaults to current directory)
    #[arg(default_value = ".")]
    directory: PathBuf,

    /// Maximum depth to display
    #[arg(long, value_name = "N")]
    depth: Option<usize>,

    /// Minimum size to display (e.g. 1MB, 500KB)
    #[arg(long, value_name = "SIZE", default_value = "0")]
    min_size: String,

    /// Sort by name instead of size
    #[arg(long, action = ArgAction::SetTrue)]
    sort_name: bool,
}

struct FileInfo {
    path: PathBuf,
    size: u64,
    is_dir: bool,
}

#[derive(Debug)]
enum SizeError {
    ParseError(String),
    IoError(io::Error),
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SizeError::ParseError(msg) => write!(f, "Size parsing error: {}", msg),
            SizeError::IoError(err) => write!(f, "I/O error: {}", err),
        }
    }
}

impl From<io::Error> for SizeError {
    fn from(error: io::Error) -> Self {
        SizeError::IoError(error)
    }
}

impl std::error::Error for SizeError {}

fn get_size(path: &Path) -> Result<u64, SizeError> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                let mut total_size = 0;
                
                // Read the directory silently ignoring errors
                if let Ok(entries) = fs::read_dir(path) {
                    for entry_result in entries {
                        if let Ok(entry) = entry_result {
                            // Silently ignore errors
                            if let Ok(size) = get_size(&entry.path()) {
                                total_size += size;
                            }
                        }
                    }
                }
                Ok(total_size)
            } else {
                Ok(metadata.len())
            }
        },
        Err(err) => Err(SizeError::IoError(err)) // Propagate the error without printing it
    }
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

fn parse_size(size_str: &str) -> Result<u64, SizeError> {
    let size_str = size_str.trim().to_uppercase();
    
    if size_str.is_empty() {
        return Err(SizeError::ParseError("Empty size string".to_string()));
    }
    
    let (num_str, unit) = if size_str.ends_with("KB") {
        (&size_str[..size_str.len() - 2], "KB")
    } else if size_str.ends_with("MB") {
        (&size_str[..size_str.len() - 2], "MB")
    } else if size_str.ends_with("GB") {
        (&size_str[..size_str.len() - 2], "GB")
    } else if size_str.ends_with("B") {
        (&size_str[..size_str.len() - 1], "B")
    } else if size_str.ends_with("K") {
        (&size_str[..size_str.len() - 1], "KB")
    } else if size_str.ends_with("M") {
        (&size_str[..size_str.len() - 1], "MB")
    } else if size_str.ends_with("G") {
        (&size_str[..size_str.len() - 1], "GB")
    } else {
        (size_str.as_str(), "B")
    };
    
    let num = num_str.parse::<f64>()
        .map_err(|_| SizeError::ParseError(format!("Invalid number: {}", num_str)))?;
    
    let multiplier = match unit {
        "KB" => 1024,
        "MB" => 1024 * 1024,
        "GB" => 1024 * 1024 * 1024,
        "B" => 1,
        _ => return Err(SizeError::ParseError(format!("Unknown unit: {}", unit))),
    };
    
    Ok((num * multiplier as f64) as u64)
}

fn walk_dir(
    dir: &Path,
    prefix: &str,
    max_depth: Option<usize>,
    min_size: u64,
    sort_by_size: bool,
    current_depth: usize,
) -> Result<(), SizeError> {
    if let Some(max_depth) = max_depth {
        if current_depth > max_depth {
            return Ok(());
        }
    }

    // Read directory, ignoring errors
    let entries = match fs::read_dir(dir) {
        Ok(entries) => {
            let mut entry_vec = Vec::new();
            for entry_result in entries {
                if let Ok(entry) = entry_result {
                    entry_vec.push(entry);
                }
                // Silently ignore entries with errors
            }
            entry_vec
        },
        Err(err) => return Err(SizeError::IoError(err)), // Only propagate the main error
    };

    let mut files = Vec::new();

    // Collect all entries
    for entry in &entries {
        let path = entry.path();
        
        // Ignore files we can't access
        let metadata = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue, // Silently skip this file
        };
        
        let is_dir = metadata.is_dir();
        let size = if is_dir {
            match get_size(&path) {
                Ok(s) => s,
                Err(_) => 0, // Use 0 as size for directories with errors
            }
        } else {
            metadata.len()
        };

        if size < min_size {
            continue;
        }

        files.push(FileInfo {
            path,
            size,
            is_dir,
        });
    }

    // Sort files by size or name as appropriate
    if sort_by_size {
        files.sort_by(|a, b| b.size.cmp(&a.size));
    } else {
        files.sort_by(|a, b| {
            let a_name = a.path.file_name().unwrap_or_default().to_string_lossy();
            let b_name = b.path.file_name().unwrap_or_default().to_string_lossy();
            a_name.cmp(&b_name)
        });
    }

    let total_entries = files.len();
    for (i, file) in files.iter().enumerate() {
        let is_last_entry = i == total_entries - 1;
        let file_name = file.path.file_name().unwrap_or_default().to_string_lossy();
        
        // Choose an icon based on file type
        let icon = if file.is_dir { "📂" } else { "📄" };

        let connector = if is_last_entry {
            "└── "
        } else {
            "├── "
        };

        // Print the entry with an icon
        println!(
            "{}{}{} {} ({})",
            prefix,
            connector,
            icon,
            file_name,
            format_size(file.size)
        );

        // Recurse into directories
        if file.is_dir {
            let new_prefix = if is_last_entry {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };

            // Silently ignore errors in recursion
            let _ = walk_dir(
                &file.path, 
                &new_prefix, 
                max_depth, 
                min_size, 
                sort_by_size, 
                current_depth + 1
            );
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse arguments using clap
    let args = Args::parse();
    
    // Parse the minimum size
    let min_size = parse_size(&args.min_size)?;
    
    let dir = &args.directory;
    
    // Verify that the directory is valid
    if !dir.exists() {
        return Err(format!("Error: {} does not exist", dir.display()).into());
    }
    
    if !dir.is_dir() {
        return Err(format!("Error: {} is not a directory", dir.display()).into());
    }
    
    // Get and display the root directory size
    match get_size(dir) {
        Ok(root_size) => {
            println!("{} ({})", dir.display(), format_size(root_size));
            
            // Skip displaying the tree if the root directory is smaller than min_size
            if root_size < min_size {
                println!("No entries meet the minimum size criteria.");
                return Ok(());
            }
            
            // Display the tree and silently ignore errors
            let _ = walk_dir(dir, "", args.depth, min_size, !args.sort_name, 0);
        },
        Err(err) => {
            return Err(Box::new(err));
        }
    }
    
    Ok(())
}