# Changelog

## 0.3.4

- iCloud (and other CalDAV) events no longer show at the wrong time. Every store since 0.3.0 was
  written with a hidden `America/Los_Angeles` display zone (a leftover sample default no one
  could see or change), so in Berlin a 10:00 event showed at 01:00. The default is now the
  device's zone, and stores with the old hidden value are moved to it when they load.
- Settings › General › *Time zone*: "System (Europe/Berlin)" or any IANA zone.
- `TZID`s that are not IANA names are understood: fixed offsets (`GMT+0200`, as some Apple and
  Exchange events carry, which were read as UTC), Windows names (`W. Europe Standard Time`),
  vendor paths (`/mozilla.org/…/Europe/Berlin`), any letter case, and a `VTIMEZONE`'s
  `X-LIC-LOCATION`. A zone only the text's `VTIMEZONE` defines is converted with it.
- `DTEND`, `EXDATE` and `RECURRENCE-ID` in another zone than `DTSTART` are converted.
- A moved or changed occurrence of a repeating event (`RECURRENCE-ID`) no longer shows twice
  (once from the series, once changed); it is not written back as an `EXDATE`.
- `COUNT` counts occurrences taken out by `EXDATE`, as RFC 5545 says.
- A time skipped by the spring change (02:30 on the last Sunday of March in Berlin) is read
  with the offset before it (03:30 summer time) instead of as UTC.
- The event form shows and saves times in the display zone; saving converts them back, so an
  event keeps its `TZID` and does not move. New events (form and quick entry) get the display
  zone or the device's zone as `TZID` instead of a floating time.

## 0.3.3

- Search lists matches from every date under the search field, as in Fantastical: upcoming first,
  then past, grouped by day with time, title and calendar colour and name. Words are AND-ed and
  matched in title, location, notes, attendees and calendar name, ignoring case and accents. A
  repeating event shows its next three occurrences and its last past one; 50 results, then
  "+N more". ↑/↓ select, Enter or a click goes to the day and opens the event, Esc closes. Works
  in every toolbar width, next to Day/Month on a phone and in the web build. The calendar grid
  is no longer filtered while typing.
- Calendar sets are yours now. The sample "Work Focus" and "Home & Personal" sets are gone (old
  stores drop them unless they were changed), and "All" is computed instead of a stored list, so
  new calendars can no longer be left out. Settings › Sets creates, edits, renames and deletes
  sets (with a confirmation); a set needs a name and at least one calendar. The pills show only
  once a set exists, and a set that lost all its calendars shows nothing, with a hint, instead
  of everything. The MCP `set_calendar_set` tool takes `set_all` for All.
- Saturday and Sunday have a faint blue background and blue weekday names in Month and Week
  (and Quarter), in light and dark.
- Month: days outside the month are clearly darker grey, with lighter numbers and faded events.

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
