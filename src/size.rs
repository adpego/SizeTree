//! Parsing and formatting of human-readable sizes (binary units: 1K = 1024 bytes).

const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

/// Formats a byte count with at most three significant digits, e.g. `999 B`,
/// `1.50 KiB`, `34.5 MiB` or `812 GiB`. The result is never wider than 8 chars.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    // Switch units before reaching 4 integer digits so columns stay narrow.
    while value >= 999.5 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    let unit = UNITS[unit];
    if value < 9.995 {
        format!("{value:.2} {unit}")
    } else if value < 99.95 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

/// Parses sizes such as `0`, `512`, `500K`, `1.5 MB`, `2GiB` or `1t`.
///
/// Units are case-insensitive and always binary, so `K`, `KB` and `KiB`
/// all mean 1024 bytes.
pub fn parse_size(input: &str) -> Result<u64, String> {
    let input = input.trim();
    let unit_start = input
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(input.len());
    let (number, unit) = input.split_at(unit_start);
    let number = number.trim();

    let shift = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 0,
        "k" | "kb" | "kib" => 10,
        "m" | "mb" | "mib" => 20,
        "g" | "gb" | "gib" => 30,
        "t" | "tb" | "tib" => 40,
        "p" | "pb" | "pib" => 50,
        _ => {
            return Err(format!(
                "unknown unit '{unit}' (expected B, K, M, G, T or P)"
            ));
        }
    };
    let multiplier = 1u64 << shift;

    if number.is_empty() {
        return Err("expected a number, optionally followed by a unit (e.g. 500K, 1.5M)".into());
    }

    // Integers are handled exactly; f64 would lose precision above 2^53.
    if let Ok(whole) = number.parse::<u64>() {
        return whole
            .checked_mul(multiplier)
            .ok_or_else(|| "size is too large".to_string());
    }

    let value: f64 = number
        .parse()
        .map_err(|_| format!("invalid number '{number}'"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!("invalid number '{number}'"));
    }
    let bytes = value * multiplier as f64;
    if bytes >= u64::MAX as f64 {
        return Err("size is too large".into());
    }
    Ok(bytes.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1000), "0.98 KiB");
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
        assert_eq!(format_size(10 * 1024), "10.0 KiB");
        assert_eq!(format_size(512 * 1024), "512 KiB");
        assert_eq!(format_size(1023 * 1024), "1.00 MiB");
        assert_eq!(format_size(5 << 30), "5.00 GiB");
        assert_eq!(format_size(u64::MAX), "16.0 EiB");
    }

    #[test]
    fn formatted_sizes_fit_in_eight_columns() {
        for shift in 0..64 {
            for bytes in [
                (1u64 << shift) - 1,
                1 << shift,
                (1 << shift) + (1 << shift) / 3,
            ] {
                let text = format_size(bytes);
                assert!(text.len() <= 8, "{bytes} -> {text:?}");
            }
        }
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("0"), Ok(0));
        assert_eq!(parse_size("512"), Ok(512));
        assert_eq!(parse_size("512B"), Ok(512));
        assert_eq!(parse_size("500K"), Ok(500 * 1024));
        assert_eq!(parse_size("500kb"), Ok(500 * 1024));
        assert_eq!(parse_size("1.5M"), Ok(1536 * 1024));
        assert_eq!(parse_size(" 1 MB "), Ok(1 << 20));
        assert_eq!(parse_size("2GiB"), Ok(2 << 30));
        assert_eq!(parse_size("1t"), Ok(1 << 40));
        assert_eq!(parse_size("16383P"), Ok(16383 << 50));
    }

    #[test]
    fn rejects_invalid_sizes() {
        for input in [
            "", "abc", "MB", "-5", "-1K", "1.2.3", "1e3", "nan", "inf", "5X", "1 K B",
        ] {
            assert!(parse_size(input).is_err(), "{input:?} should be rejected");
        }
        assert!(parse_size("16384P").is_err(), "overflow must be rejected");
        assert!(parse_size("99999999999999999999").is_err());
    }
}
