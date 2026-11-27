# Changelog

## 0.3.5

- **Custom repeats.** The event form's Repeat menu has *Custom…* after the presets, as in Apple
  Calendar and Fantastical: *Every N* days, weeks, months or years; for weeks, toggle chips for
  Mon–Sun shown as square one-letter chips, M T W T F S S, in every language and width (the week starts on Monday, at least one day, the event's own weekday by default); for
  months and years, on the event's day or on *the first … fifth / last* weekday ("the 2nd
  Tuesday", "the last Friday"); and an end: never, after N times, or on a date. A summary line
  ("Every 2 weeks on Mon, Wed, Fri until 31 Dec 2026") shows under it in the new-event dialog and
  replaces "repeats" in the event details. Works in the phone layout and in the web build;
  labels are translated.
- Event details, the new-event dialog and the settings, file-conflict and migration dialogs scroll
  when they are taller than the window, with a scroll bar that stays visible (in its own gutter,
  not hidden until hover) whenever there is more to see. In event details and the new-event
  dialog the actions (Delete, Cancel, Save / Add) are pinned under the scrolling fields, so a
  long repeat or long notes no longer push them out of the popover.
- Rules map to and from `RRULE` (`FREQ`, `INTERVAL`, `BYDAY` with positions, `BYMONTHDAY`,
  `BYMONTH`, `BYSETPOS`, `COUNT`, `UNTIL`, `WKST`). A rule that equals a preset shows the preset
  (`FREQ=WEEKLY;BYDAY=FR` on a Friday is *Every week*); anything else opens as Custom with its
  values filled in. A rule with parts the form can't edit (`BYHOUR`, several month days,
  "last working day", `X-` parts…) is shown with its summary, the rule itself and a note, and is
  kept exactly as it is unless you pick another repeat.
- Repeat rules are no longer rewritten when an event is saved: every part is kept (before, parts
  `craft-ical` doesn't model, such as `BYMONTHDAY` lists, `BYSETPOS` sets or `WKST`, were
  dropped on the next save to CalDAV or a file). Only `UNTIL` is put in the form RFC 5545 asks
  for: a date for all-day events, UTC beside a start with a `TZID` (the end of that day in the
  event's zone), a local time beside a floating start.
- Repeating events are expanded period by period as RFC 5545 has it: several weekdays a week
  with an interval and `WKST`, monthly and yearly positions (`2TU`, `-1FR`, `5MO`), month days
  from the end (`BYMONTHDAY=-1`), `BYMONTH`, `BYSETPOS` ("last working day"), the 31st and
  29 February skipped where they don't exist, and the start always counted as the first
  occurrence. Times stay on the wall clock across daylight-saving changes; `EXDATE` and `COUNT`
  work as before. Long-running series are worked out from near the shown dates instead of from
  their start.
- A click in one of the details popover's menus that reaches below the popover no longer closes
  it.

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
