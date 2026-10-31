use chrono::{Datelike, Duration, NaiveDate, Weekday};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeekRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

pub fn week_after_sunday(date: NaiveDate) -> Result<WeekRange, String> {
    if date.weekday() != Weekday::Sun {
        return Err("주간 선택 기준은 일요일이어야 합니다.".into());
    }
    let from = date
        .checked_add_signed(Duration::days(1))
        .ok_or("날짜 범위를 벗어났습니다.")?;
    let to = date
        .checked_add_signed(Duration::days(7))
        .ok_or("날짜 범위를 벗어났습니다.")?;
    Ok(WeekRange { from, to })
}
pub fn week_containing(date: NaiveDate) -> WeekRange {
    let offset = i64::from(date.weekday().num_days_from_monday());
    WeekRange {
        from: date - Duration::days(offset),
        to: date + Duration::days(6 - offset),
    }
}
pub fn calendar_days(month: NaiveDate) -> Vec<NaiveDate> {
    let first = month.with_day(1).unwrap();
    let start = first - Duration::days(i64::from(first.weekday().num_days_from_sunday()));
    (0..42).map(|i| start + Duration::days(i)).collect()
}

/// Sunday-first visible dates. Rendering and fetching must share this range.
/// The anchor is bounded; neighboring cells may extend into 1899 or 9999.
pub fn calendar_range(anchor: NaiveDate, rolling: Option<(u8, u8)>) -> Result<WeekRange, String> {
    if !(1900..=9998).contains(&anchor.year()) {
        return Err("날짜는 1900년부터 9998년까지 선택할 수 있습니다.".into());
    }
    let (first, count) = if let Some((weeks, past)) = rolling {
        if !(2..=8).contains(&weeks) || past > 2 || past >= weeks {
            return Err(
                "표시 주 수는 2~8주, 지난주는 0~2주이며 전체 주 수보다 작아야 합니다.".into(),
            );
        }
        (anchor - Duration::weeks(i64::from(past)), Some(weeks))
    } else {
        (anchor.with_day(1).unwrap(), None)
    };
    let from = first - Duration::days(i64::from(first.weekday().num_days_from_sunday()));
    let to = if let Some(weeks) = count {
        from + Duration::days(i64::from(weeks) * 7 - 1)
    } else {
        let last = shift_month(first, 1) - Duration::days(1);
        last + Duration::days(6 - i64::from(last.weekday().num_days_from_sunday()))
    };
    Ok(WeekRange { from, to })
}
pub fn shift_month(date: NaiveDate, delta: i32) -> NaiveDate {
    let index = date.year() * 12 + date.month0() as i32 + delta;
    NaiveDate::from_ymd_opt(index.div_euclid(12), index.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(date)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configurable_ranges_cover_months_and_rolling_year_boundaries() {
        let feb = calendar_range(date("2026-02-12"), None).unwrap();
        assert_eq!((feb.from, feb.to), (date("2026-02-01"), date("2026-02-28")));
        let leap = calendar_range(date("2028-02-29"), None).unwrap();
        assert!(leap.from <= date("2028-02-01") && leap.to >= date("2028-02-29"));
        assert_eq!((leap.to - leap.from).num_days() + 1, 35);
        let six = calendar_range(date("2026-05-01"), None).unwrap();
        assert_eq!((six.to - six.from).num_days() + 1, 42);
        let rolling = calendar_range(date("2027-01-03"), Some((4, 1))).unwrap();
        assert_eq!(
            (rolling.from, rolling.to),
            (date("2026-12-27"), date("2027-01-23"))
        );
        for anchor in ["1900-01-01", "9998-12-31"] {
            assert!(calendar_range(date(anchor), Some((8, 2))).is_ok());
        }
        assert!(calendar_range(date("1899-12-31"), None).is_err());
        for invalid in [(0, 0), (9, 0), (2, 2), (8, 3)] {
            assert!(calendar_range(date("2026-10-31"), Some(invalid)).is_err());
        }
    }
    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    #[test]
    fn sunday_selector_crosses_year() {
        assert_eq!(
            week_after_sunday(date("2026-12-27")).unwrap(),
            WeekRange {
                from: date("2026-12-28"),
                to: date("2027-01-03")
            }
        );
        assert!(week_after_sunday(date("2026-12-28")).is_err());
    }
    #[test]
    fn containing_sunday_is_previous_monday() {
        assert_eq!(week_containing(date("2027-01-03")).from, date("2026-12-28"));
    }
    #[test]
    fn leap_calendar_and_month_boundary() {
        let days = calendar_days(date("2028-02-01"));
        assert_eq!(days.len(), 42);
        assert_eq!(days[0].weekday(), Weekday::Sun);
        assert!(days.contains(&date("2028-02-29")));
        assert_eq!(shift_month(date("2027-01-31"), -1), date("2026-12-01"));
    }
}
