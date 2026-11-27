# Upstream needs (recorded, not filed)

| # | Where | Need | Today |
|---|---|---|---|
| 1 | craft-web-host contract | A settings message (theme, language, animations) | artcraftCloud edits `calendarcraft.ui` in `localStorage` before start; a live change needs a reload |
| 2 | craft-web-host contract | A generic secrets store per app | artcraftCloud now keeps CalendarCraft's remote passwords in its vault (`/api/calendar/remote/credentials`); a generic host-contract secret store would still help other apps |
| 3 | craft-web-host contract | A host network API (`fetch` through the host) | artcraftCloud's CSP is `connect-src 'self'`; CalendarCraft uses the host's `remoteProxy` named in `window.calendarcraftHost` |
| 4 | craft-web-host contract | "New" with no file for apps that don't edit documents | the shell opens CalendarCraft as *Untitled.ics*; CalendarCraft only reports dirty for an opened file |
| 5 | Browsers / egui | Ctrl+N and ⌘N are reserved by browsers | N or C opens the new-event dialog in the browser |
| 6 | egui (web) | System fonts aren't available to a canvas app | Inter is bundled for the web build; SF Pro is loaded natively on macOS |
| 7 | eframe (web) | Under Chrome's device-scale emulation the canvas is sized at 1× | real devices are fine; `scripts/screenshots.cjs` starts Chrome at a real 2× for phone shots |
| 8 | Stalwart | App passwords can't be scoped to CalDAV only | the MailCraft app password also opens CalDAV |
| 9 | Google / Microsoft | OAuth needed for CalDAV/Graph | read-only ICS feeds |
| 10 | craft-ical `rrule` | A lossless RRULE model (every part kept, `BYMONTHDAY`/`BYMONTH`/`BYSETPOS` lists, several positioned `BYDAY`s, `WKST`) and an expander | `Recurrence` holds a subset and `format_*` writes only that subset, so a round trip dropped parts; CalendarCraft now has its own `src/rrule.rs` (model, parse/write, summary) and `src/recur.rs` (expansion) and no longer formats rules with craft-ical |
