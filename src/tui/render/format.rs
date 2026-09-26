//! Width-safe text and deterministic date formatting helpers.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme::DateMode;

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

pub(super) fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

pub(super) fn truncate_with(value: &str, width: usize, ellipsis: &str) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let suffix = take_width(ellipsis, width);
    let suffix_width = display_width(&suffix);
    let mut output = take_width(value, width.saturating_sub(suffix_width));
    output.push_str(&suffix);
    output
}

pub(super) fn pad_left(value: &str, width: usize) -> String {
    let value = truncate_with(value, width, "");
    format!(
        "{}{value}",
        " ".repeat(width.saturating_sub(display_width(&value)))
    )
}

pub(super) fn pad_right(value: &str, width: usize) -> String {
    let value = truncate_with(value, width, "");
    format!(
        "{value}{}",
        " ".repeat(width.saturating_sub(display_width(&value)))
    )
}

/// Greedy word wrap by terminal cells. Words wider than a row are split.
pub(super) fn wrap_words(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in value.split(' ') {
        let mut word = word.to_owned();
        let gap = usize::from(!row.is_empty());
        if display_width(&row) + gap + display_width(&word) <= width {
            if gap == 1 {
                row.push(' ');
            }
            row.push_str(&word);
            continue;
        }
        if !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        while display_width(&word) > width {
            let head = take_width(&word, width);
            if head.is_empty() {
                break;
            }
            word = word[head.len()..].to_owned();
            rows.push(head);
        }
        row = word;
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

fn take_width(value: &str, width: usize) -> String {
    let mut used: usize = 0;
    value
        .chars()
        .take_while(|character| {
            let character_width = UnicodeWidthChar::width(*character).unwrap_or(0);
            if used.saturating_add(character_width) > width {
                false
            } else {
                used += character_width;
                true
            }
        })
        .collect()
}

/// Compact date for list columns: minutes, not seconds, and no offset suffix,
/// so a whole page of rows shares one narrow aligned column.
pub(super) fn list_date(timestamp: i64, timezone: &str, mode: DateMode) -> String {
    let minutes = |offset: i64| {
        let local = timestamp.saturating_add(offset);
        let seconds = local.rem_euclid(86_400);
        format!(
            "{} {:02}:{:02}",
            compact_date(local),
            seconds / 3_600,
            (seconds % 3_600) / 60
        )
    };
    match mode {
        DateMode::Unix => timestamp.to_string(),
        DateMode::Iso => minutes(0),
        DateMode::Local => minutes(parse_timezone_offset(timezone)),
        DateMode::Relative => relative_age(timestamp),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_and_padding_use_terminal_cell_width() {
        assert_eq!(display_width("界e\u{301}"), 3);
        assert_eq!(truncate_with("界界abc", 5, "…"), "界界…");
        assert_eq!(display_width(&pad_right("界", 4)), 4);
        assert_eq!(pad_left("7d", 4), "  7d");
    }

    #[test]
    fn word_wrap_respects_cells_and_splits_long_words() {
        assert_eq!(wrap_words("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap_words("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap_words("界界界", 4), ["界界", "界"]);
        assert_eq!(wrap_words("", 4), [""]);
    }

    #[test]
    fn list_dates_stay_compact_in_every_mode() {
        // 2023-11-14 22:13:20 UTC
        let timestamp = 1_700_000_000;
        assert_eq!(
            list_date(timestamp, "-04:00", DateMode::Local),
            "2023-11-14 18:13"
        );
        assert_eq!(
            list_date(timestamp, "-04:00", DateMode::Iso),
            "2023-11-14 22:13"
        );
        assert_eq!(list_date(timestamp, "-04:00", DateMode::Unix), "1700000000");
    }
}
