//! UTC timestamp parsing and formatting shared by DASObjectStore services.
//!
//! The supported wire format is a UTC calendar timestamp with a `Z` suffix.
//! Parsing accepts optional fractional seconds and deliberately returns whole
//! seconds because the persistent telemetry and job metadata formats use that
//! precision.

const SECONDS_PER_DAY: i64 = 86_400;

/// Parses a UTC timestamp into whole seconds from the Unix epoch.
///
/// Accepts `YYYY-MM-DDTHH:MM:SSZ` and the same format with fractional seconds,
/// such as `YYYY-MM-DDTHH:MM:SS.123Z`. Fractional seconds are discarded.
pub fn parse_utc_timestamp_seconds(value: &str) -> Option<i64> {
    parse_utc_timestamp_seconds_impl(value, true)
}

/// Parses the canonical whole-second UTC timestamp format.
///
/// This accepts exactly `YYYY-MM-DDTHH:MM:SSZ`, with four-digit years.
pub fn parse_canonical_utc_timestamp_seconds(value: &str) -> Option<i64> {
    if value.len() != 20
        || value.get(4..5)? != "-"
        || value.get(7..8)? != "-"
        || value.get(10..11)? != "T"
        || value.get(13..14)? != ":"
        || value.get(16..17)? != ":"
    {
        return None;
    }
    parse_utc_timestamp_seconds_impl(value, false)
}

/// Formats Unix-epoch seconds as a canonical whole-second UTC timestamp.
pub fn format_utc_timestamp_seconds(seconds_since_epoch: i64) -> String {
    let days = seconds_since_epoch.div_euclid(SECONDS_PER_DAY);
    let seconds_of_day = seconds_since_epoch.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Adds whole seconds to a UTC timestamp and returns a canonical UTC timestamp.
pub fn add_seconds_to_utc_timestamp(timestamp: &str, seconds: u64) -> Option<String> {
    let timestamp_seconds = parse_canonical_utc_timestamp_seconds(timestamp)?;
    let seconds = i64::try_from(seconds).ok()?;
    let total = timestamp_seconds.checked_add(seconds)?;
    Some(format_utc_timestamp_seconds(total))
}

fn parse_utc_timestamp_seconds_impl(value: &str, allow_fractional_seconds: bool) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let time = match time.split_once('.') {
        Some((whole_seconds, fractional_seconds)) if allow_fractional_seconds => {
            (!fractional_seconds.is_empty()
                && fractional_seconds.bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(whole_seconds)?
        }
        Some(_) => return None,
        None => time,
    };
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if time_parts.next().is_some()
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    days.checked_mul(SECONDS_PER_DAY)?
        .checked_add(i64::from(hour).checked_mul(3_600)?)?
        .checked_add(i64::from(minute).checked_mul(60)?)?
        .checked_add(i64::from(second))
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    let year = year.checked_sub(i64::from(month <= 2))?;
    let era = year.div_euclid(400);
    let year_of_era = year.checked_sub(era.checked_mul(400)?)?;
    let month = i64::from(month);
    let day = i64::from(day);
    let month_offset = if month > 2 { -3 } else { 9 };
    let day_of_year = 153_i64
        .checked_mul(month.checked_add(month_offset)?)?
        .checked_add(2)?
        / 5
        + day
        - 1;
    let day_of_era = year_of_era
        .checked_mul(365)?
        .checked_add(year_of_era / 4)?
        .checked_sub(year_of_era / 100)?
        .checked_add(day_of_year)?;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::{
        add_seconds_to_utc_timestamp, format_utc_timestamp_seconds,
        parse_canonical_utc_timestamp_seconds, parse_utc_timestamp_seconds,
    };

    #[test]
    fn parses_canonical_and_fractional_timestamps() {
        assert_eq!(
            parse_utc_timestamp_seconds("2024-02-29T12:34:56Z"),
            Some(1_709_210_096)
        );
        assert_eq!(
            parse_utc_timestamp_seconds("2024-02-29T12:34:56.789Z"),
            Some(1_709_210_096)
        );
    }

    #[test]
    fn rejects_invalid_calendar_and_timestamp_syntax() {
        for value in [
            "2023-02-29T00:00:00Z",
            "2024-04-31T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:00:00.Z",
            "2024-01-01T00:00:00.abcZ",
            "2024-01-01T00:00:00+00:00",
        ] {
            assert_eq!(parse_utc_timestamp_seconds(value), None, "{value}");
        }
    }

    #[test]
    fn formats_and_adds_seconds_across_calendar_boundaries() {
        assert_eq!(format_utc_timestamp_seconds(-1), "1969-12-31T23:59:59Z");
        assert_eq!(
            add_seconds_to_utc_timestamp("2024-02-28T23:59:59Z", 1),
            Some("2024-02-29T00:00:00Z".to_string())
        );
    }

    #[test]
    fn canonical_parser_requires_exact_whole_second_format() {
        assert!(parse_canonical_utc_timestamp_seconds("2024-01-02T03:04:05Z").is_some());
        assert_eq!(
            parse_canonical_utc_timestamp_seconds("2024-01-02T03:04:05.1Z"),
            None
        );
        assert_eq!(
            parse_canonical_utc_timestamp_seconds("2024-1-2T3:4:5Z"),
            None
        );
    }
}
