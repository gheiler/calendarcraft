# Your own calendar accounts

*Settings › Accounts › Add* in CalendarCraft. Passwords stay on this device. Inside artcraftCloud
the requests go through artcraftCloud's calendar proxy; standalone in a browser the server must
allow CORS (the desktop app has no such limit).

| Provider | Kind | Address | Password |
|---|---|---|---|
| iCloud | CalDAV | `https://caldav.icloud.com/` | an app-specific password (appleid.apple.com) |
| Fastmail | CalDAV | `https://caldav.fastmail.com/dav/calendars/user/<you@fastmail.com>/` | an app password (Settings › Privacy & Security) |
| Nextcloud / ownCloud | CalDAV | `https://<host>/remote.php/dav/` | an app password |
| Stalwart, Radicale, Baïkal, SOGo, mailbox.org, Posteo | CalDAV | the server, or its `/.well-known/caldav` | account or app password |
| artcraftCloud (from another app) | CalDAV | `https://<mail host>/dav/cal/<address>/` | the artcraftCloud password |
| Google Calendar | ICS feed (read only) | Calendar settings › *Secret address in iCal format* | none |
| Outlook.com / Microsoft 365 | ICS feed (read only) | Outlook › Settings › Calendar › Shared calendars › *Publish* (ICS link) | none |
| Proton Calendar | ICS feed (read only) | *Share via link* | none |
| Holidays, sports, school… | ICS feed | any `https://` or `webcal://` `.ics` link | none |

Google (CalDAV needs OAuth 2.0 with a registered client) and Microsoft (Graph API, no CalDAV)
can't be written to without an OAuth sign-in, which CalendarCraft doesn't have yet.

## Mail: does MailCraft work with personal accounts?

- **Desktop MailCraft:** yes: IMAP/SMTP (Gmail with an app password, iCloud, Fastmail…), Microsoft
  accounts through OAuth, Exchange, JMAP.
- **MailCraft in the browser (and so in artcraftCloud):** JMAP only. Browsers can't open IMAP or
  SMTP connections, so a personal Gmail/IMAP mailbox can't be read there. In artcraftCloud the
  page may also only talk to its own origin, so even another JMAP server (Fastmail) is out of
  reach without a proxy. What works today: the artcraftCloud mailbox in MailCraft, and personal
  accounts in desktop MailCraft or any mail app (the artcraftCloud mailbox also works in them over
  IMAP/SMTP).
- Ways to add personal mail in artcraftCloud, for a decision: (a) a JMAP proxy for named outside
  servers (Fastmail) like `/api/calendar/remote`; (b) an IMAP↔JMAP bridge on the server (a
  sizeable component, credentials held server-side); (c) let Stalwart fetch the external mailbox
  into the user's mailbox (Stalwart has no built-in IMAP fetcher; a fetchmail/imapsync sidecar).

## Where passwords are kept

CalendarCraft no longer writes account passwords as a plaintext JSON map.

| How you run it | Where the password lives |
|---|---|
| Desktop app | The OS credential store: macOS Keychain, Windows Credential Manager, or Linux Secret Service. The service name is `CalendarCraft`; the account is the calendar account id (`acc_…`). |
| Standalone web | Encrypted with WebCrypto AES-GCM. The key is a non-extractable `CryptoKey` in IndexedDB (`calendarcraft-keys`). Only the ciphertext and nonce are in `localStorage["calendarcraft.secrets"]`. This protects a copied storage dump or a casual look at the disk; it does not protect against script running on this same origin. |
| Inside artcraftCloud | The server keeps them sealed with its vault (ChaCha20-Poly1305, per person and account). The remote calendar proxy attaches the password on the server; it is not stored in the browser and is not sent back. |

If an older `calendarcraft.secrets` plaintext map is still present, it is moved into the new store the first time the app starts, and then deleted. When the app is hosted, each leftover password is stored with the username already on that account (or the account's email if the username was never saved).
