//! Search across every date, as Fantastical does: each word must appear (in any order) in an
//! event's title, location, notes, attendees or calendar name, ignoring case and accents.
//! Upcoming matches come first, soonest first, then past ones, most recent first. A repeating
//! event shows its next few occurrences (and its last past one), not every one.

use crate::model::{CalendarEvent, CalendarStore};
use crate::tz::DisplayTz;
use chrono::{Duration, NaiveDate};

/// How far ahead a repeating event is expanded.
pub const LOOKAHEAD_DAYS: i64 = 730;
/// Upcoming occurrences listed per repeating event.
pub const SERIES_UPCOMING: usize = 3;
/// Results listed before "N more".
pub const DEFAULT_LIMIT: usize = 50;

/// One match: an occurrence of `event_id`, already in the display zone.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub event_id: String,
    /// The occurrence (its `date`, times and `end_date` are in the display zone).
    pub occurrence: CalendarEvent,
    /// On or after today (or still running today).
    pub upcoming: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// Matches not listed because of the limit.
    pub more: usize,
}

/// Lower case without accents: "Café Zürich" and "cafe zurich" compare equal.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'æ' => out.push_str("ae"),
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
            'ď' | 'đ' | 'ð' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
            'ĥ' | 'ħ' => out.push('h'),
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
            'ĳ' => out.push_str("ij"),
            'ĵ' => out.push('j'),
            'ķ' => out.push('k'),
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => out.push('l'),
            'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
            'œ' => out.push_str("oe"),
            'ŕ' | 'ŗ' | 'ř' => out.push('r'),
            'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => out.push('s'),
            'ß' => out.push_str("ss"),
            'ţ' | 'ť' | 'ŧ' | 'ț' => out.push('t'),
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
            'ŵ' => out.push('w'),
            'ý' | 'ÿ' | 'ŷ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            'þ' => out.push_str("th"),
            // Combining marks (a decomposed "é"): drop them.
            '\u{300}'..='\u{36f}' => {}
            c => out.push(c),
        }
    }
    out
}

/// The query's words, folded. Empty when the query is blank.
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(fold).filter(|t| !t.is_empty()).collect()
}

/// Whether every term appears in the event's title, location, notes, attendees or calendar name.
pub fn matches(store: &CalendarStore, ev: &CalendarEvent, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let mut hay = String::new();
    hay.push_str(&ev.title);
    hay.push('\n');
    hay.push_str(ev.location.as_deref().unwrap_or(""));
    hay.push('\n');
    hay.push_str(&ev.notes);
    for a in &ev.attendees {
        hay.push('\n');
        hay.push_str(a);
    }
    if let Some(cal) = store.calendar_by_id(&ev.calendar_id) {
        hay.push('\n');
        hay.push_str(&cal.name);
    }
    let hay = fold(&hay);
    terms.iter().all(|t| hay.contains(t.as_str()))
}

fn last_day(o: &CalendarEvent) -> NaiveDate {
    o.end_date.unwrap_or(o.date).max(o.date)
}

/// Search the calendars that are showing (the chosen set, visible calendars, enabled accounts).
pub fn search(store: &CalendarStore, query: &str, today: NaiveDate, limit: usize) -> SearchResults {
    let terms = terms(query);
    if terms.is_empty() {
        return SearchResults::default();
    }
    let display = DisplayTz::parse(&store.settings.timezone);
    let until = today + Duration::days(LOOKAHEAD_DAYS);
    let mut upcoming: Vec<SearchHit> = Vec::new();
    let mut past: Vec<SearchHit> = Vec::new();
    for ev in &store.events {
        if !store.is_calendar_visible(&ev.calendar_id) || !matches(store, ev, &terms) {
            continue;
        }
        let occurrences = ev.occurrences_until(display, until);
        let hit = |o: CalendarEvent, up: bool| SearchHit { event_id: ev.id.clone(), occurrence: o, upcoming: up };
        let (mut next, mut before): (Vec<_>, Vec<_>) = occurrences.into_iter().partition(|o| last_day(o) >= today);
        if ev.rrule.is_some() {
            next.truncate(SERIES_UPCOMING);
            // The latest past one, so a finished series can still be found.
            before = before.pop().into_iter().collect();
        }
        upcoming.extend(next.into_iter().map(|o| hit(o, true)));
        past.extend(before.into_iter().map(|o| hit(o, false)));
    }
    let key = |h: &SearchHit| (h.occurrence.date, h.occurrence.sort_key(), fold(&h.occurrence.title));
    upcoming.sort_by_key(key);
    past.sort_by_key(key);
    past.reverse();
    let total = upcoming.len() + past.len();
    let hits: Vec<SearchHit> = upcoming.into_iter().chain(past).take(limit).collect();
    SearchResults { more: total - hits.len(), hits }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CalendarSource;
    use chrono::NaiveTime;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn store() -> CalendarStore {
        let mut s = CalendarStore::empty();
        s.settings.timezone = "Local".into();
        s.calendars.push(CalendarSource { id: "c1".into(), account_id: "a".into(), name: "Zahnärzte".into(), color_hex: "#ff0000".into(), visible: true, ..Default::default() });
        s.calendars.push(CalendarSource { id: "c2".into(), account_id: "a".into(), name: "Work".into(), color_hex: "#00ff00".into(), visible: true, ..Default::default() });
        let ev = |id: &str, cal: &str, title: &str, date: NaiveDate| CalendarEvent {
            id: id.into(),
            calendar_id: cal.into(),
            title: title.into(),
            date,
            start_time: NaiveTime::from_hms_opt(9, 0, 0),
            end_time: NaiveTime::from_hms_opt(10, 0, 0),
            ..Default::default()
        };
        s.events.push(ev("past", "c1", "Dentist check-up", d(2026, 3, 2)));
        s.events.push(ev("soon", "c1", "Dentist cleaning", d(2026, 12, 1)));
        s.events.push(ev("later", "c1", "Dentist follow-up", d(2027, 2, 1)));
        s.events.push(ev("older", "c1", "Dentist first visit", d(2025, 6, 1)));
        let mut cafe = ev("cafe", "c2", "Coffee", d(2026, 11, 30));
        cafe.location = Some("Café Zürich".into());
        cafe.attendees = vec!["maya@example.org".into()];
        cafe.notes = "Bring the Q4 plan".into();
        s.events.push(cafe);
        let mut weekly = ev("weekly", "c2", "Standup", d(2026, 1, 5));
        weekly.rrule = Some("FREQ=WEEKLY".into());
        s.events.push(weekly);
        s
    }

    fn ids(r: &SearchResults) -> Vec<(&str, NaiveDate)> {
        r.hits.iter().map(|h| (h.event_id.as_str(), h.occurrence.date)).collect()
    }

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Café ZÜRICH Straße"), "cafe zurich strasse");
        assert_eq!(fold("Cafe\u{301}"), "cafe");
        assert_eq!(terms("  Zürich  café "), vec!["zurich", "cafe"]);
    }

    #[test]
    fn upcoming_first_then_past_most_recent_first() {
        let r = search(&store(), "dentist", d(2026, 11, 26), 50);
        assert_eq!(
            ids(&r),
            vec![("soon", d(2026, 12, 1)), ("later", d(2027, 2, 1)), ("past", d(2026, 3, 2)), ("older", d(2025, 6, 1))]
        );
        assert!(r.hits[0].upcoming && !r.hits[2].upcoming);
        assert_eq!(r.more, 0);
    }

    #[test]
    fn matches_location_notes_attendees_and_calendar_name_with_all_words() {
        let s = store();
        let today = d(2026, 11, 26);
        assert_eq!(ids(&search(&s, "cafe zurich", today, 50)), vec![("cafe", d(2026, 11, 30))]);
        assert_eq!(ids(&search(&s, "maya", today, 50)).len(), 1);
        assert_eq!(ids(&search(&s, "q4 PLAN", today, 50)).len(), 1);
        assert_eq!(ids(&search(&s, "zahnarzte cleaning", today, 50)), vec![("soon", d(2026, 12, 1))]);
        assert!(search(&s, "coffee dentist", today, 50).hits.is_empty(), "words are AND-ed");
        assert!(search(&s, "   ", today, 50).hits.is_empty());
    }

    #[test]
    fn repeating_events_show_the_next_few_and_the_last_past_one() {
        let r = search(&store(), "standup", d(2026, 11, 26), 50);
        assert_eq!(
            ids(&r),
            vec![("weekly", d(2026, 11, 30)), ("weekly", d(2026, 12, 7)), ("weekly", d(2026, 12, 14)), ("weekly", d(2026, 11, 23))]
        );
    }

    #[test]
    fn limit_counts_the_rest() {
        let r = search(&store(), "dentist", d(2026, 11, 26), 2);
        assert_eq!(r.hits.len(), 2);
        assert_eq!(r.more, 2);
    }

    #[test]
    fn hidden_calendars_and_other_sets_are_not_searched() {
        let mut s = store();
        s.calendars[1].visible = false;
        assert!(search(&s, "coffee", d(2026, 11, 26), 50).hits.is_empty());
        s.calendars[1].visible = true;
        s.calendar_sets.push(crate::model::CalendarSet { id: "s1".into(), name: "Teeth".into(), calendar_ids: vec!["c1".into()] });
        s.settings.active_calendar_set = Some("s1".into());
        assert!(search(&s, "coffee", d(2026, 11, 26), 50).hits.is_empty());
        assert_eq!(search(&s, "dentist", d(2026, 11, 26), 50).hits.len(), 4);
    }
}
