const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
const UNIT_STEP: f64 = 1024.0;

/// Formats a byte count for people, e.g. `5.3 MB` or `64 MB`.
pub(crate) fn format_byte_size(bytes: usize) -> String {
    if (bytes as f64) < UNIT_STEP {
        return format!("{bytes} B");
    }

    let (value, unit) = scaled_value(bytes as f64);
    format!("{} {unit}", trim_fraction(value))
}

fn scaled_value(bytes: f64) -> (f64, &'static str) {
    let mut value = bytes / UNIT_STEP;
    let mut unit_index = 0;

    while value >= UNIT_STEP && unit_index + 1 < UNITS.len() {
        value /= UNIT_STEP;
        unit_index += 1;
    }

    (value, UNITS[unit_index])
}

fn trim_fraction(value: f64) -> String {
    let rounded = format!("{value:.1}");
    rounded
        .strip_suffix(".0")
        .map(str::to_owned)
        .unwrap_or(rounded)
}

#[cfg(test)]
mod tests {
    use super::format_byte_size;

    #[test]
    fn formats_small_sizes_in_bytes() {
        assert_eq!(format_byte_size(0), "0 B");
        assert_eq!(format_byte_size(1023), "1023 B");
    }

    #[test]
    fn formats_whole_units_without_fraction() {
        assert_eq!(format_byte_size(1024), "1 KB");
        assert_eq!(format_byte_size(1024 * 1024), "1 MB");
        assert_eq!(format_byte_size(64 * 1024 * 1024), "64 MB");
    }

    #[test]
    fn formats_fractional_units_with_one_decimal() {
        assert_eq!(format_byte_size(5_557_453), "5.3 MB");
        assert_eq!(format_byte_size(1536), "1.5 KB");
    }
}
