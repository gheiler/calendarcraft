# CalDAV in CalendarCraft, and how artcraftCloud connects it to Stalwart

## The client (`src/caldav.rs`, `src/sync.rs`, `src/net.rs`)

1. **Discovery.** `PROPFIND Depth: 1` on the account address. If that lists calendars (a calendar
   home), they are used. Otherwise `current-user-principal` (on the address, then on
   `/.well-known/caldav`), then `calendar-home-set`, then `PROPFIND Depth: 1` on the home.
   Redirects are followed by the client itself (up to 5), so a credential never goes to a host
   it wasn't given for unless the server sends it there.
2. **Reading.** `REPORT calendar-query` with a `time-range` around the shown date (about six
   months back, thirteen ahead), widened as you move. Each `calendar-data` is parsed
   (`src/ics.rs`, using [craft-ical](https://github.com/storytold/craft-libs) for lines, values,
   `RRULE` text and `VTIMEZONE`); repeating events are expanded locally (`src/recur.rs`:
   DAILY/WEEKLY/MONTHLY/YEARLY, INTERVAL, COUNT, UNTIL, BYDAY, EXDATE) in the event's own
   zone, then shown in the user's zone. Read-only calendars
   (`current-user-privilege-set` without write) and multi-event resources are locked.
3. **Writing.** A new event is `PUT <calendar>/<uid>.ics` with `If-None-Match: *`; a change is a
   `PUT` with `If-Match: <etag>`; a delete is `DELETE` with `If-Match`. On `412` the calendar is
   read again (the rejected local event is replaced by the server's version, with a message).
   Mail-backed calendars in artcraftCloud are this CalDAV path (Stalwart), not JMAP. Two tabs
   editing the same event already conflict at the event resource; CalendarCraft does not show the
   three-way *Reload theirs / Keep mine / Save a copy* dialog used for file-backed calendars,
   because each event is its own `.ics` on the server and a reload is cheap. The later save is
   refused and the calendar is re-read.
4. **Feeds.** An `.ics` URL (or `webcal://`) is fetched with `GET` and shown read-only.

On the desktop requests go straight to the server (`ureq`). In the browser they go through
`fetch`: to the host's own path for host accounts, through the host's `remoteProxy` for other
servers (a page in artcraftCloud may only reach its own origin, and most CalDAV servers send no
CORS headers), and directly when the app runs on its own and the server allows CORS.

## artcraftCloud

Stalwart keeps calendars next to mail: each account has a CalDAV home at
`/dav/cal/<address>/` (`/.well-known/caldav` redirects there). artcraftCloud already makes a
Stalwart account for every person with a mailbox, and keeps an **app password** for it sealed
on the server (the one MailCraft's JMAP proxy uses). CalendarCraft reuses it:

```
browser (CalendarCraft, same origin)            artcraftCloud                        Stalwart
  PROPFIND /caldav/ada%40x.org/  ──cookie──▶  /caldav/*: session → user → mailbox
  x-requested-with: artcraft                  app password from the vault
                                              ──Basic ada@x.org:<app pw>──▶  /dav/cal/ada%40x.org/
                                              ◀── 207, hrefs /dav/cal/… rewritten to /caldav/…
```

- `GET /api/calendar/calendarcraft.js` (put in front of CalendarCraft's `index.html` by the
  server, like MailCraft's hook) sets `window.calendarcraftHost` with one account,
  `url: "/caldav/<address>/"`, when the person has a mailbox, and `remoteProxy`.
- `/caldav/*` takes only the app's requests (`X-Requested-With: artcraft`), only `GET HEAD
  OPTIONS PROPFIND REPORT PUT DELETE`, never a path that climbs out (`..`, `%2e`, `%2f`), passes
  `Depth`, `If-Match`, `If-None-Match`, `Content-Type`, and returns `ETag` and `Location`.
  A disabled person's session is refused, so their calendar closes with their account.
- `/api/calendar/remote` passes one request to another server for the app (iCloud, Fastmail, a
  public feed): `x-calendar-url` is the target, `x-calendar-auth` the `Authorization` to send.
  Only http(s), only public addresses (`ARTCRAFT_CALENDAR_ALLOW_PRIVATE=1` for a lab), the
  checked address is the one connected to, redirects are returned (not followed), and
  `WWW-Authenticate` is dropped. The credential for such a server is kept by CalendarCraft in the
  server, sealed in artcraftCloud's vault; the proxy attaches it and the password does not sit in the browser.
- An `.ics` file in the drive opens in CalendarCraft (`.ics`, `.ical`, `.ifb`) and is saved
  back with the shell's Save/Autosave.
- artcraftCloud's Settings write `theme`, `language` and `animations` into `calendarcraft.ui`.

Other calendar apps (Apple Calendar, Thunderbird, DAVx⁵) can use the same calendar directly:
server `https://<mail host>/dav/cal/<address>/` (or just the host, via `.well-known`), user name
the email address, password the artcraftCloud password.
