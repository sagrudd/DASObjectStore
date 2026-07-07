const MIB: u128 = 1024 * 1024;
const GIB: u128 = MIB * 1024;
const TIB: u128 = GIB * 1024;

/// Formats byte counts with binary units for TUI planning displays.
pub fn format_size_label(bytes: u64) -> String {
    let bytes = u128::from(bytes);
    let (unit_bytes, unit) = if bytes >= TIB {
        (TIB, "TiB")
    } else if bytes >= GIB {
        (GIB, "GiB")
    } else {
        (MIB, "MiB")
    };

    let tenths = ((bytes * 10) + (unit_bytes / 2)) / unit_bytes;
    format!("{}.{:01} {}", tenths / 10, tenths % 10, unit)
}
