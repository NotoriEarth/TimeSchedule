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
pub fn shift_month(date: NaiveDate, delta: i32) -> NaiveDate {
    let index = date.year() * 12 + date.month0() as i32 + delta;
    NaiveDate::from_ymd_opt(index.div_euclid(12), index.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(date)
}
#[cfg(test)]
mod tests {
    use super::*;
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
