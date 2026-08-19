//! Width-safe text and deterministic date formatting helpers.

use crate::sanitize::sanitize_str;

use super::theme::DATE_MODE;

pub(super) fn format_commit_date(timestamp: i64, timezone: &str) -> String {
    let offset = parse_timezone_offset(timezone);
    let local = timestamp.saturating_add(offset);
    let days = local.div_euclid(86_400);
    let seconds = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} {}",
        seconds / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
        sanitize_str(timezone)
    )
}

pub(super) fn compact_date(timestamp: i64) -> String {
    let (year, month, day) = civil_from_days(timestamp.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

pub(super) fn parse_timezone_offset(timezone: &str) -> i64 {
    let bytes = timezone.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return 0;
    }
    let (hours, minutes) = match bytes {
        [_, h1, h2, b':', m1, m2] | [_, h1, h2, m1, m2] => ([*h1, *h2], [*m1, *m2]),
        _ => return 0,
    };
    let Ok(hours) = std::str::from_utf8(&hours)
        .unwrap_or_default()
        .parse::<i64>()
    else {
        return 0;
    };
    let Ok(minutes) = std::str::from_utf8(&minutes)
        .unwrap_or_default()
        .parse::<i64>()
    else {
        return 0;
    };
    let offset = hours.saturating_mul(3_600) + minutes.saturating_mul(60);
    if bytes[0] == b'-' { -offset } else { offset }
}

// Howard Hinnant's civil-date conversion, with day zero at 1970-01-01.
pub(super) fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

pub(super) fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    let mut output: String = value.chars().take(width.saturating_sub(1)).collect();
    output.push('…');
    output
}

pub(super) fn display_date(timestamp: i64, timezone: &str) -> String {
    match DATE_MODE.read().expect("date mode lock").as_str() {
        "unix" => timestamp.to_string(),
        "iso" => format_commit_date(timestamp, "+00:00"),
        "local" => format_commit_date(timestamp, timezone),
        _ => relative_age(timestamp),
    }
}

pub(super) fn relative_age(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64);
    let seconds = now.saturating_sub(timestamp).max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else if seconds < 2_592_000 {
        format!("{}d", seconds / 86_400)
    } else if seconds < 31_536_000 {
        format!("{}mo", seconds / 2_592_000)
    } else {
        format!("{}y", seconds / 31_536_000)
    }
}
