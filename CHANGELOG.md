# Changelog

## 0.3.0

- Read and write iCalendar text through [craft-ical](https://github.com/storytold/craft-libs)
  (pinned to git revision `dc2cb42`), instead of CalendarCraft's own parser and writer.
- Honor `TZID` / `VTIMEZONE` (with an IANA fallback via `chrono-tz`). UTC (`…Z`), floating
  times and all-day dates keep their meaning. Events are shown in the zone chosen in Settings.
- Recurring events are still expanded in CalendarCraft, now in the event's own zone so a weekly
  time stays put across a daylight-saving change, then converted for display.
- Export writes the original kind of time back, with a `VTIMEZONE` for named zones, so a
  round trip stays valid.
- The toolchain is pinned to Rust 1.100.0 (`rust-toolchain.toml`); craft-ical needs 1.95+.
