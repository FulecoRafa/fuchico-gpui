//! Pure helpers for the agenda view (no GPUI): the local "today", month
//! cursor navigation, the localized month title and row labels.

use chrono::{Datelike, Local, NaiveDate};

use crate::i18n::Locale;

/// `YYYY-MM-DD` of `date`.
pub fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Today in the **local** timezone. The original used
/// `new Date().toISOString()` (UTC), which is off by a day near midnight
/// outside UTC (PARITY QUIRK, fixed per DECISIONS.md).
pub fn local_today() -> NaiveDate {
    Local::now().date_naive()
}

/// The calendar's month cursor; `month` is 0-based like JS `Date`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonthCursor {
    pub year: i32,
    pub month: u32,
}

impl MonthCursor {
    pub fn of(date: NaiveDate) -> Self {
        MonthCursor { year: date.year(), month: date.month0() }
    }

    /// ChevronLeft: previous month, wrapping the year.
    pub fn prev(self) -> Self {
        if self.month == 0 { MonthCursor { year: self.year - 1, month: 11 } } else { MonthCursor { month: self.month - 1, ..self } }
    }

    /// ChevronRight: next month, wrapping the year.
    pub fn next(self) -> Self {
        if self.month == 11 { MonthCursor { year: self.year + 1, month: 0 } } else { MonthCursor { month: self.month + 1, ..self } }
    }
}

const MONTHS_EN: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS_PT: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];

/// `Intl.DateTimeFormat(locale, {month: "long", year: "numeric"})`:
/// "July 2026" / "julho de 2026".
pub fn month_title(locale: Locale, cursor: MonthCursor) -> String {
    let m = cursor.month.min(11) as usize;
    match locale {
        Locale::En => format!("{} {}", MONTHS_EN[m], cursor.year),
        Locale::PtBr => format!("{} de {}", MONTHS_PT[m], cursor.year),
    }
}

/// `🔁 {recurrence}[ {recurTime}]`.
pub fn recurrence_label(recurrence: &str, recur_time: Option<&str>) -> String {
    match recur_time {
        Some(t) => format!("🔁 {recurrence} {t}"),
        None => format!("🔁 {recurrence}"),
    }
}

/// Last path segment (either separator), like the TS `basename`.
pub fn basename(path: &str) -> &str {
    path.split(['/', '\\']).rfind(|s| !s.is_empty()).unwrap_or(path)
}

/// Day number of a `YYYY-MM-DD` cell (`Number(iso.slice(-2))`).
pub fn day_of(iso: &str) -> u32 {
    iso.get(iso.len().saturating_sub(2)..).and_then(|d| d.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_wraps_years() {
        let jan = MonthCursor { year: 2026, month: 0 };
        assert_eq!(jan.prev(), MonthCursor { year: 2025, month: 11 });
        assert_eq!(jan.prev().next(), jan);
        let dec = MonthCursor { year: 2026, month: 11 };
        assert_eq!(dec.next(), MonthCursor { year: 2027, month: 0 });
        assert_eq!(MonthCursor { year: 2026, month: 6 }.next(), MonthCursor { year: 2026, month: 7 });
    }

    #[test]
    fn cursor_of_date_is_zero_based() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 20).unwrap();
        assert_eq!(MonthCursor::of(d), MonthCursor { year: 2026, month: 6 });
        assert_eq!(iso(d), "2026-07-20");
    }

    #[test]
    fn month_titles_follow_intl() {
        let c = MonthCursor { year: 2026, month: 9 };
        assert_eq!(month_title(Locale::En, c), "October 2026");
        assert_eq!(month_title(Locale::PtBr, c), "outubro de 2026");
    }

    #[test]
    fn labels() {
        assert_eq!(recurrence_label("daily", Some("08:00")), "🔁 daily 08:00");
        assert_eq!(recurrence_label("mon,thu", None), "🔁 mon,thu");
        assert_eq!(basename("/v/sub/personal.md"), "personal.md");
        assert_eq!(basename("C:\\v\\a.md"), "a.md");
        assert_eq!(basename("a.md"), "a.md");
        assert_eq!(day_of("2026-07-05"), 5);
        assert_eq!(day_of("2026-07-31"), 31);
    }

    #[test]
    fn local_today_matches_chrono_local() {
        assert_eq!(iso(local_today()), Local::now().format("%Y-%m-%d").to_string());
    }
}
