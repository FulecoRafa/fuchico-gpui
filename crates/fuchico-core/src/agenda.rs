//! Pure logic from `src/modules/agenda/AgendaView.tsx`: the month grid and
//! the overdue / due / upcoming / no-date / routines buckets.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};

use crate::tasks::AgendaItem;

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarCell {
    pub key: String,
    /// `YYYY-MM-DD`, or `None` for leading/trailing padding cells.
    pub iso: Option<String>,
}

/// Weeks (Sunday-first) of a month. `month` is 0-based like JS `Date`.
pub fn month_weeks(year: i32, month: u32) -> Vec<Vec<CalendarCell>> {
    let first = NaiveDate::from_ymd_opt(year, month + 1, 1).expect("valid month");
    let start_weekday = first.weekday().num_days_from_sunday();
    let next = if month == 11 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 2, 1)
    }
    .unwrap();
    let days_in_month = (next - first).num_days() as u32;

    let mut cells = Vec::new();
    for i in 0..start_weekday {
        cells.push(CalendarCell {
            key: format!("{year}-{month}-lead-{i}"),
            iso: None,
        });
    }
    for d in 1..=days_in_month {
        let iso = format!("{year}-{:02}-{d:02}", month + 1);
        cells.push(CalendarCell {
            key: iso.clone(),
            iso: Some(iso),
        });
    }
    let mut trail = 0;
    while cells.len() % 7 != 0 {
        cells.push(CalendarCell {
            key: format!("{year}-{month}-trail-{trail}"),
            iso: None,
        });
        trail += 1;
    }
    cells.chunks(7).map(|c| c.to_vec()).collect()
}

/// Items that have a date, grouped by it.
pub fn items_by_date(items: &[AgendaItem]) -> BTreeMap<String, Vec<&AgendaItem>> {
    let mut map: BTreeMap<String, Vec<&AgendaItem>> = BTreeMap::new();
    for item in items {
        if let Some(d) = &item.date {
            map.entry(d.clone()).or_default().push(item);
        }
    }
    map
}

#[derive(Debug, Default)]
pub struct AgendaBuckets<'a> {
    pub overdue: Vec<&'a AgendaItem>,
    pub due: Vec<&'a AgendaItem>,
    pub upcoming: Vec<&'a AgendaItem>,
    pub no_date: Vec<&'a AgendaItem>,
    pub routines: Vec<&'a AgendaItem>,
}

/// `today` and `selected_date` are `YYYY-MM-DD` strings (compared
/// lexicographically like the TS).
pub fn bucket_items<'a>(
    items: &'a [AgendaItem],
    today: &str,
    selected_date: Option<&str>,
) -> AgendaBuckets<'a> {
    let filtered: Vec<&AgendaItem> = match selected_date {
        Some(d) => items
            .iter()
            .filter(|i| i.date.as_deref() == Some(d))
            .collect(),
        None => items.iter().collect(),
    };
    let due_date = selected_date.unwrap_or(today);
    AgendaBuckets {
        overdue: filtered
            .iter()
            .copied()
            .filter(|i| !i.checked && i.date.as_deref().is_some_and(|d| d < today))
            .collect(),
        due: filtered
            .iter()
            .copied()
            .filter(|i| i.date.as_deref() == Some(due_date))
            .collect(),
        upcoming: filtered
            .iter()
            .copied()
            .filter(|i| i.date.as_deref().is_some_and(|d| d > today))
            .collect(),
        no_date: filtered
            .iter()
            .copied()
            .filter(|i| i.date.is_none() && i.recurrence.is_none())
            .collect(),
        routines: items.iter().filter(|i| i.recurrence.is_some()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::ItemKind;

    fn item(text: &str, date: Option<&str>, checked: bool, rec: Option<&str>) -> AgendaItem {
        AgendaItem {
            kind: ItemKind::Task,
            checked,
            text: text.into(),
            date: date.map(String::from),
            time: None,
            file: "f".into(),
            line: 1,
            recurrence: rec.map(String::from),
            recur_time: None,
        }
    }

    #[test]
    fn month_grid_is_sunday_first_and_padded() {
        // Aug 2026 starts on a Saturday and has 31 days.
        let weeks = month_weeks(2026, 7);
        assert!(weeks.iter().all(|w| w.len() == 7));
        assert_eq!(weeks[0].iter().filter(|c| c.iso.is_none()).count(), 6);
        assert_eq!(weeks[0][6].iso.as_deref(), Some("2026-08-01"));
        assert!(weeks.last().unwrap().iter().any(|c| c.iso.is_some()));
        assert_eq!(
            weeks.iter().flatten().filter(|c| c.iso.is_some()).count(),
            31
        );
        assert_eq!(weeks[0][0].key, "2026-7-lead-0");
    }

    #[test]
    fn buckets() {
        let items = vec![
            item("old", Some("2026-08-01"), false, None),
            item("done old", Some("2026-08-01"), true, None),
            item("today", Some("2026-08-27"), false, None),
            item("later", Some("2026-09-01"), false, None),
            item("nodate", None, false, None),
            item("routine", None, false, Some("daily")),
        ];
        let b = bucket_items(&items, "2026-08-27", None);
        assert_eq!(b.overdue.len(), 1);
        assert_eq!(b.due[0].text, "today");
        assert_eq!(b.upcoming.len(), 1);
        assert_eq!(b.no_date.len(), 1);
        assert_eq!(b.routines.len(), 1);
        let sel = bucket_items(&items, "2026-08-27", Some("2026-09-01"));
        assert_eq!(sel.due[0].text, "later");
        assert!(sel.no_date.is_empty());
        assert_eq!(items_by_date(&items).len(), 3);
    }
}
