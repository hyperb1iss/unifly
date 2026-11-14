//! Human-readable byte and duration formatting helpers.

/// Format bytes into a compact human-readable string (e.g., "245M", "1.2G").
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
pub fn fmt_bytes_short(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.1}T", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.1}G", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{}M", bytes / 1_000_000)
    } else if bytes >= 1_000 {
        format!("{}K", bytes / 1_000)
    } else {
        format!("{bytes}B")
    }
}

/// Format a TX/RX byte pair as "245M/52M".
pub fn fmt_tx_rx(tx: u64, rx: u64) -> String {
    format!("{}/{}", fmt_bytes_short(tx), fmt_bytes_short(rx))
}

/// Format seconds into a compact human duration (e.g., "47d", "4h 23m", "12m").
pub fn fmt_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;

    if days > 0 {
        format!("{days}d")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Format a rate in bytes/sec as "245 Mbps".
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
pub fn fmt_rate(bytes_per_sec: u64) -> String {
    let bits = bytes_per_sec * 8;
    if bits >= 1_000_000_000 {
        format!("{:.1} Gbps", bits as f64 / 1_000_000_000.0)
    } else if bits >= 1_000_000 {
        format!("{:.1} Mbps", bits as f64 / 1_000_000.0)
    } else if bits >= 1_000 {
        format!("{:.1} Kbps", bits as f64 / 1_000.0)
    } else {
        format!("{bits} bps")
    }
}

/// Compact rate for chart Y-axis labels: "50M", "1.2G", "500K".
/// Input is bytes/sec — converted to bits for display.
pub fn fmt_rate_axis(bytes_per_sec: f64) -> String {
    let bits = bytes_per_sec * 8.0;
    if bits >= 1_000_000_000.0 {
        format!("{:.1}G", bits / 1_000_000_000.0)
    } else if bits >= 1_000_000.0 {
        format!("{:.0}M", bits / 1_000_000.0)
    } else if bits >= 1_000.0 {
        format!("{:.0}K", bits / 1_000.0)
    } else {
        format!("{bits:.0}")
    }
}

/// Render a percentage bar split into filled and empty portions.
///
/// Returns `(filled, empty)` strings of `█` and `░` characters that together
/// span `width` character positions. Caller applies styling per segment.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::as_conversions
)]
pub fn fmt_pct_bar(pct: f64, width: u16) -> (String, String) {
    let clamped = pct.clamp(0.0, 100.0);
    let filled_count = ((clamped / 100.0) * f64::from(width)).round() as u16;
    let empty_count = width.saturating_sub(filled_count);
    (
        "█".repeat(usize::from(filled_count)),
        "░".repeat(usize::from(empty_count)),
    )
}

/// Render a proportional traffic bar using fractional block characters.
///
/// Uses ▏▎▍▌▋▊▉█ for sub-character precision across `max_chars` positions.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::as_conversions
)]
pub fn fmt_traffic_bar(value: u64, max_value: u64, max_chars: u16) -> String {
    const FRACTIONAL: &[char] = &[' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

    if max_value == 0 || max_chars == 0 {
        return " ".repeat(usize::from(max_chars));
    }
    // How many eighth-blocks to fill
    let fraction = (value as f64 / max_value as f64).min(1.0);
    let total_eighths = (fraction * f64::from(max_chars) * 8.0).round() as u32;
    let full_blocks = total_eighths / 8;
    let remainder = total_eighths % 8;

    let mut bar = "█".repeat(full_blocks as usize);
    if remainder > 0 {
        bar.push(FRACTIONAL[remainder as usize]);
    }
    // Pad to max_chars
    let bar_len = full_blocks + u32::from(remainder > 0);
    let padding = u32::from(max_chars).saturating_sub(bar_len);
    bar.push_str(&" ".repeat(padding as usize));
    bar
}
