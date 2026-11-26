# Changelog

## 0.3.2

- Desktop passwords are really stored in the OS credential store now. keyring was built without
  a platform backend and kept them in memory only, so they were lost on restart. A plaintext
  `calendarcraft.secrets` file is deleted only after every password reads back from the store.
- A CalDAV account with a user name but no saved password is no longer asked without one. Its
  card says "No saved password for this account", with an *Enter password* action.
- Settings › Accounts › *Edit*: change an account's name, address, user name and password
  (empty keeps the saved one) without removing it; calendars and colours stay attached.
- Phones: the search icon sits next to Day/Month and opens a focused field. Event details, new
  event, Settings, delete, file conflict and migration dialogs fit the screen and scroll.
- Start and end times stay readable (no more clipped "12:00"); on a phone they move under the date.
- Long account addresses are cut with "…" (full address on hover) instead of running under
  the buttons.

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
