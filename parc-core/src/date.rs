use chrono::{Datelike, Local, NaiveDate};

use crate::error::ParcError;

/// Relative date shorthands used in both search DSL and --due flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelativeDate {
    Today,
    Yesterday,
    Tomorrow,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    NextWeek,
    Overdue,
    DaysAgo(u32),
    InDays(u32),
}

/// Try to parse a string as a relative date shorthand.
pub fn parse_relative_date(value: &str) -> Option<RelativeDate> {
    match value {
        "today" => Some(RelativeDate::Today),
        "yesterday" => Some(RelativeDate::Yesterday),
        "tomorrow" => Some(RelativeDate::Tomorrow),
        "this-week" => Some(RelativeDate::ThisWeek),
        "last-week" => Some(RelativeDate::LastWeek),
        "next-week" => Some(RelativeDate::NextWeek),
        "this-month" => Some(RelativeDate::ThisMonth),
        "last-month" => Some(RelativeDate::LastMonth),
        "overdue" => Some(RelativeDate::Overdue),
        _ => {
            // N-days-ago pattern
            if let Some(rest) = value.strip_suffix("-days-ago") {
                return rest.parse::<u32>().ok().map(RelativeDate::DaysAgo);
            }
            // in-N-days pattern
            if let Some(rest) = value.strip_prefix("in-") {
                if let Some(n_str) = rest.strip_suffix("-days") {
                    return n_str.parse::<u32>().ok().map(RelativeDate::InDays);
                }
            }
            None
        }
    }
}

/// Resolve a relative date to a (start_date, end_date) range as ISO date strings.
/// For point dates, start == end.
pub fn resolve_relative_date_to_range(rel: &RelativeDate) -> (String, String) {
    let today = Local::now().date_naive();
    match rel {
        RelativeDate::Today => {
            let d = today.format("%Y-%m-%d").to_string();
            (d.clone(), d)
        }
        RelativeDate::Yesterday => {
            let d = (today - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            (d.clone(), d)
        }
        RelativeDate::Tomorrow => {
            let d = (today + chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            (d.clone(), d)
        }
        RelativeDate::ThisWeek => {
            let weekday = today.weekday().num_days_from_monday();
            let monday = today - chrono::Duration::days(weekday as i64);
            let sunday = monday + chrono::Duration::days(6);
            (
                monday.format("%Y-%m-%d").to_string(),
                sunday.format("%Y-%m-%d").to_string(),
            )
        }
        RelativeDate::LastWeek => {
            let weekday = today.weekday().num_days_from_monday();
            let this_monday = today - chrono::Duration::days(weekday as i64);
            let last_monday = this_monday - chrono::Duration::days(7);
            let last_sunday = last_monday + chrono::Duration::days(6);
            (
                last_monday.format("%Y-%m-%d").to_string(),
                last_sunday.format("%Y-%m-%d").to_string(),
            )
        }
        RelativeDate::NextWeek => {
            let weekday = today.weekday().num_days_from_monday();
            let this_monday = today - chrono::Duration::days(weekday as i64);
            let next_monday = this_monday + chrono::Duration::days(7);
            let next_sunday = next_monday + chrono::Duration::days(6);
            (
                next_monday.format("%Y-%m-%d").to_string(),
                next_sunday.format("%Y-%m-%d").to_string(),
            )
        }
        RelativeDate::ThisMonth => {
            let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
            let last = if today.month() == 12 {
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap()
                    - chrono::Duration::days(1)
            } else {
                NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1).unwrap()
                    - chrono::Duration::days(1)
            };
            (
                first.format("%Y-%m-%d").to_string(),
                last.format("%Y-%m-%d").to_string(),
            )
        }
        RelativeDate::LastMonth => {
            let (y, m) = if today.month() == 1 {
                (today.year() - 1, 12)
            } else {
                (today.year(), today.month() - 1)
            };
            let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
            let last = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap()
                - chrono::Duration::days(1);
            (
                first.format("%Y-%m-%d").to_string(),
                last.format("%Y-%m-%d").to_string(),
            )
        }
        RelativeDate::Overdue => {
            let yesterday = (today - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            ("0000-01-01".to_string(), yesterday)
        }
        RelativeDate::DaysAgo(n) => {
            let d = (today - chrono::Duration::days(*n as i64))
                .format("%Y-%m-%d")
                .to_string();
            (d.clone(), d)
        }
        RelativeDate::InDays(n) => {
            let d = (today + chrono::Duration::days(*n as i64))
                .format("%Y-%m-%d")
                .to_string();
            (d.clone(), d)
        }
    }
}

/// Resolve a --due value to a concrete YYYY-MM-DD date string.
/// If the input is a relative shorthand, resolve it to the end of the range
/// (e.g. this-week → Sunday, next-week → next Sunday).
/// If not a shorthand, pass through as-is (assumed YYYY-MM-DD).
pub fn resolve_due_date(input: &str) -> Result<String, ParcError> {
    if let Some(rel) = parse_relative_date(input) {
        let (_start, end) = resolve_relative_date_to_range(&rel);
        Ok(end)
    } else {
        // Validate it's a valid date or pass through
        if NaiveDate::parse_from_str(input, "%Y-%m-%d").is_ok() {
            Ok(input.to_string())
        } else {
            Err(ParcError::ParseError(format!(
                "invalid due date '{}': expected YYYY-MM-DD or shorthand (today, tomorrow, in-N-days, etc.)",
                input
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_date_shorthands() {
        assert_eq!(parse_relative_date("today"), Some(RelativeDate::Today));
        assert_eq!(parse_relative_date("yesterday"), Some(RelativeDate::Yesterday));
        assert_eq!(parse_relative_date("tomorrow"), Some(RelativeDate::Tomorrow));
        assert_eq!(parse_relative_date("this-week"), Some(RelativeDate::ThisWeek));
        assert_eq!(parse_relative_date("last-week"), Some(RelativeDate::LastWeek));
        assert_eq!(parse_relative_date("next-week"), Some(RelativeDate::NextWeek));
        assert_eq!(parse_relative_date("this-month"), Some(RelativeDate::ThisMonth));
        assert_eq!(parse_relative_date("last-month"), Some(RelativeDate::LastMonth));
        assert_eq!(parse_relative_date("overdue"), Some(RelativeDate::Overdue));
        assert_eq!(parse_relative_date("3-days-ago"), Some(RelativeDate::DaysAgo(3)));
        assert_eq!(parse_relative_date("in-3-days"), Some(RelativeDate::InDays(3)));
        assert_eq!(parse_relative_date("not-a-date"), None);
    }

    #[test]
    fn test_resolve_due_date_today() {
        let result = resolve_due_date("today").unwrap();
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(result, today);
    }

    #[test]
    fn test_resolve_due_date_tomorrow() {
        let result = resolve_due_date("tomorrow").unwrap();
        let tomorrow = (Local::now().date_naive() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, tomorrow);
    }

    #[test]
    fn test_resolve_due_date_in_n_days() {
        let result = resolve_due_date("in-3-days").unwrap();
        let expected = (Local::now().date_naive() + chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_due_date_passthrough() {
        let result = resolve_due_date("2026-03-15").unwrap();
        assert_eq!(result, "2026-03-15");
    }

    #[test]
    fn test_resolve_due_date_invalid() {
        assert!(resolve_due_date("not-valid").is_err());
    }

    #[test]
    fn test_resolve_range_point_dates() {
        let (start, end) = resolve_relative_date_to_range(&RelativeDate::Today);
        assert_eq!(start, end);

        let (start, end) = resolve_relative_date_to_range(&RelativeDate::Yesterday);
        assert_eq!(start, end);
    }

    #[test]
    fn test_resolve_range_this_week() {
        let (start, end) = resolve_relative_date_to_range(&RelativeDate::ThisWeek);
        let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d").unwrap();
        let end_date = NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap();
        assert_eq!(start_date.weekday(), chrono::Weekday::Mon);
        assert_eq!(end_date.weekday(), chrono::Weekday::Sun);
        assert_eq!((end_date - start_date).num_days(), 6);
    }
}
