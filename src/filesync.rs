//! File-backed calendars in a host (artcraftCloud): paths, ETag polling, conflicts and migration.
//!
//! Standalone web and desktop still keep local calendars in `calendarcraft.store`. In host mode
//! those calendars live as `.ics` files in the person's drive (default folder `Calendars/`), and
//! the saved store only keeps a reference by file id (the drive path).

use crate::model::{AccountConfig, AccountKind, CalendarSource, CalendarStore};

/// Default folder in the person's drive for calendars created in host mode.
pub const DEFAULT_FOLDER: &str = "Calendars";
/// How often to check whether a file changed, in seconds, when the setting is unset or zero.
pub const DEFAULT_SYNC_SECS: u32 = 120;
/// Do not poll more often than this, even if the setting is smaller.
pub const MIN_SYNC_SECS: u32 = 15;
/// Wait this long after a file-backed calendar becomes dirty before writing it.
pub const AUTOSAVE_DELAY_SECS: f64 = 1.5;

/// The account that groups every file-backed calendar in host mode.
pub const ACC_FILES: &str = "acc_files";

/// What to do after comparing the file's ETag to the last one we saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollAction {
    /// The file is unchanged, or we have nothing to compare yet.
    None,
    /// The file changed and we have no unsaved edits: reload it.
    Reload,
    /// Both sides changed: do not overwrite silently.
    Conflict,
}

/// Arm, hold, or fire a debounce window for file autosave.
///
/// `armed_at` is `0.0` when no dirty period is in progress. The window starts on
/// the first dirty tick, so an edit late in a session does not write immediately.
pub fn arm_autosave(armed_at: f64, now: f64, dirty: bool, delay: f64) -> (f64, bool) {
    if !dirty {
        return (0.0, false);
    }
    if armed_at <= 0.0 {
        return (now, false);
    }
    if now - armed_at < delay {
        return (armed_at, false);
    }
    (now, true)
}

pub fn poll_action(local_dirty: bool, remote_etag_changed: bool) -> PollAction {
    match (local_dirty, remote_etag_changed) {
        (_, false) => PollAction::None,
        (false, true) => PollAction::Reload,
        (true, true) => PollAction::Conflict,
    }
}

/// The person's choice in the conflict dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Discard local edits and load the file as it is now.
    ReloadTheirs,
    /// Write our version over the file, using the current remote ETag as If-Match.
    KeepMine,
    /// Write our version to a new file and reload the original.
    SaveACopy,
}

/// How to carry out a conflict choice (or a normal save).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritePlan {
    Reload,
    /// Overwrite `path` with If-Match set to the ETag we believe is current.
    Overwrite { path: String, if_match: Option<String> },
    /// Create a new file next to `path`; no If-Match.
    Copy { path: String },
}

pub fn plan_conflict(
    choice: ConflictChoice,
    path: &str,
    current_remote_etag: Option<&str>,
    existing: &[String],
) -> WritePlan {
    match choice {
        ConflictChoice::ReloadTheirs => WritePlan::Reload,
        ConflictChoice::KeepMine => WritePlan::Overwrite {
            path: path.to_string(),
            if_match: current_remote_etag.map(str::to_string),
        },
        ConflictChoice::SaveACopy => WritePlan::Copy {
            path: copy_path(path, existing),
        },
    }
}

/// Interpret an HTTP write: 2xx is saved, 412 (and legacy 409) is an ETag conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    Saved { etag: Option<String> },
    Conflict { current_etag: Option<String> },
    Failed(String),
}

pub fn interpret_write(status: u16, etag: Option<String>, error: Option<String>) -> WriteOutcome {
    match status {
        200..=299 => WriteOutcome::Saved { etag },
        412 | 409 => WriteOutcome::Conflict { current_etag: etag },
        _ => WriteOutcome::Failed(error.unwrap_or_else(|| format!("write failed ({status})"))),
    }
}

pub fn sync_interval_secs(configured: u32) -> u32 {
    if configured == 0 {
        DEFAULT_SYNC_SECS
    } else {
        configured.max(MIN_SYNC_SECS)
    }
}

/// A file name for a calendar, with a `.ics` suffix and nothing that would climb a path.
pub fn safe_file_name(calendar_name: &str) -> String {
    let mut stem = String::new();
    for ch in calendar_name.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            continue;
        }
        stem.push(ch);
    }
    let stem = stem.trim().trim_matches('.');
    let stem = if stem.is_empty() { "Calendar" } else { stem };
    if stem.to_ascii_lowercase().ends_with(".ics") {
        stem.to_string()
    } else {
        format!("{stem}.ics")
    }
}

/// `Calendars/Work.ics`, or `Calendars/Work 2.ics` if that name is taken.
pub fn unique_path(folder: &str, file_name: &str, existing: &[String]) -> String {
    let folder = folder.trim().trim_matches('/');
    let file_name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let (stem, ext) = split_ics(file_name);
    let candidate = |n: u32| {
        let name = if n <= 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} {n}.{ext}")
        };
        if folder.is_empty() {
            name
        } else {
            format!("{folder}/{name}")
        }
    };
    let mut n = 1u32;
    loop {
        let path = candidate(n);
        if !existing.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
            return path;
        }
        n = n.saturating_add(1);
        if n > 200 {
            return candidate(n);
        }
    }
}

/// `Work.ics` → `Work (copy).ics`, then `Work (copy 2).ics`, matching artcraftCloud's copies.
pub fn copy_path(path: &str, existing: &[String]) -> String {
    let (folder, name) = match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    };
    let (stem, ext) = split_ics(name);
    let candidate = |n: u32| {
        let label = if n <= 1 {
            "copy".to_string()
        } else {
            format!("copy {n}")
        };
        let name = format!("{stem} ({label}).{ext}");
        if folder.is_empty() {
            name
        } else {
            format!("{folder}/{name}")
        }
    };
    let mut n = 1u32;
    loop {
        let path = candidate(n);
        if !existing.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
            return path;
        }
        n = n.saturating_add(1);
        if n > 200 {
            return candidate(n);
        }
    }
}

fn split_ics(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (name, "ics"),
    }
}

/// Stable calendar id derived from the file id (the drive path), not from the contents.
pub fn calendar_id_for(file_id: &str) -> String {
    format!("cal_file_{}", crate::ics::short_hash(file_id))
}

pub fn is_file_backed(cal: &CalendarSource) -> bool {
    cal.file_id.as_deref().is_some_and(|id| !id.is_empty())
}

/// Local calendars in the store that should be written out as `.ics` files once, in host mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalExport {
    pub calendar_id: String,
    pub name: String,
    pub suggested_path: String,
}

pub fn pending_exports(store: &CalendarStore, existing_paths: &[String]) -> Vec<LocalExport> {
    let mut existing = existing_paths.to_vec();
    let mut out = Vec::new();
    for cal in &store.calendars {
        if is_file_backed(cal) {
            continue;
        }
        let Some(acc) = store.accounts.iter().find(|a| a.id == cal.account_id) else {
            continue;
        };
        if acc.kind != AccountKind::Local {
            continue;
        }
        let file_name = safe_file_name(&cal.name);
        let path = unique_path(DEFAULT_FOLDER, &file_name, &existing);
        existing.push(path.clone());
        out.push(LocalExport {
            calendar_id: cal.id.clone(),
            name: cal.name.clone(),
            suggested_path: path,
        });
    }
    out
}

/// After a successful export, drop that local calendar (and `acc_local` if it has none left).
/// Events of other calendars are left alone. The file-backed calendar is expected to have been
/// inserted already (or will be) by the caller; this only removes the local source.
pub fn remove_exported_local(store: &mut CalendarStore, calendar_id: &str) {
    store.events.retain(|e| e.calendar_id != calendar_id);
    store.calendars.retain(|c| c.id != calendar_id);
    for set in &mut store.calendar_sets {
        set.calendar_ids.retain(|id| id != calendar_id);
    }
    let local_left = store.calendars.iter().any(|c| {
        store
            .accounts
            .iter()
            .any(|a| a.id == c.account_id && a.kind == AccountKind::Local)
    });
    if !local_left {
        store
            .accounts
            .retain(|a| !(a.kind == AccountKind::Local && a.id == "acc_local"));
    }
}

/// The account that holds file-backed calendars, created if needed.
pub fn ensure_files_account(store: &mut CalendarStore, name: &str) {
    if store.accounts.iter().any(|a| a.id == ACC_FILES) {
        if let Some(acc) = store.accounts.iter_mut().find(|a| a.id == ACC_FILES) {
            acc.kind = AccountKind::File;
            acc.enabled = true;
        }
        return;
    }
    store.accounts.push(AccountConfig {
        id: ACC_FILES.into(),
        name: name.into(),
        provider: "File".into(),
        enabled: true,
        kind: AccountKind::File,
        ..Default::default()
    });
}

/// Whether two ETags name the same revision (quoted, weak, or raw).
pub fn etag_equal(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> &str {
        s.trim()
            .strip_prefix("W/")
            .unwrap_or(s.trim())
            .trim()
            .trim_matches('"')
    }
    !a.is_empty() && !b.is_empty() && norm(a) == norm(b)
}

pub fn etag_changed(known: Option<&str>, remote: Option<&str>) -> bool {
    match (known, remote) {
        (Some(k), Some(r)) if !k.is_empty() && !r.is_empty() => !etag_equal(k, r),
        // No remote tag: we cannot tell, so do not treat it as a change.
        (_, None) => false,
        // We had no tag and the host now sends one: first observation, not a change.
        (None, Some(_)) => false,
        (Some(_), Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CalendarEvent, CalendarSource};
    use chrono::NaiveDate;

    fn local_cal(id: &str, name: &str) -> (AccountConfig, CalendarSource) {
        let acc = AccountConfig {
            id: "acc_local".into(),
            name: "On this device".into(),
            provider: "Local".into(),
            enabled: true,
            kind: AccountKind::Local,
            ..Default::default()
        };
        let cal = CalendarSource {
            id: id.into(),
            account_id: "acc_local".into(),
            name: name.into(),
            color_hex: "#B899EB".into(),
            visible: true,
            ..Default::default()
        };
        (acc, cal)
    }

    #[test]
    fn poll_reloads_when_only_the_file_changed() {
        assert_eq!(poll_action(false, false), PollAction::None);
        assert_eq!(poll_action(true, false), PollAction::None);
        assert_eq!(poll_action(false, true), PollAction::Reload);
        assert_eq!(poll_action(true, true), PollAction::Conflict);
    }

    #[test]
    fn etag_compare_ignores_quotes_and_weak_flags() {
        assert!(etag_equal("1-ab", "\"1-ab\""));
        assert!(etag_equal("W/\"1-ab\"", "1-ab"));
        assert!(!etag_equal("1-ab", "2-cd"));
        assert!(!etag_changed(Some("1-ab"), Some("\"1-ab\"")));
        assert!(etag_changed(Some("1-ab"), Some("2-cd")));
        assert!(!etag_changed(None, Some("1-ab")));
        assert!(!etag_changed(Some("1-ab"), None));
    }

    #[test]
    fn unique_and_copy_paths_avoid_collisions() {
        assert_eq!(
            unique_path("Calendars", "Work.ics", &[]),
            "Calendars/Work.ics"
        );
        assert_eq!(
            unique_path(
                "Calendars",
                "Work.ics",
                &["Calendars/Work.ics".into()]
            ),
            "Calendars/Work 2.ics"
        );
        assert_eq!(
            copy_path("Calendars/Work.ics", &[]),
            "Calendars/Work (copy).ics"
        );
        assert_eq!(
            copy_path(
                "Calendars/Work.ics",
                &["Calendars/Work (copy).ics".into()]
            ),
            "Calendars/Work (copy 2).ics"
        );
    }

    #[test]
    fn file_names_drop_path_characters() {
        assert_eq!(safe_file_name("Work"), "Work.ics");
        assert_eq!(safe_file_name("  "), "Calendar.ics");
        assert_eq!(safe_file_name("a/b:c.ics"), "abc.ics");
        assert_eq!(calendar_id_for("Calendars/Work.ics"), calendar_id_for("Calendars/Work.ics"));
        assert_ne!(
            calendar_id_for("Calendars/Work.ics"),
            calendar_id_for("Calendars/Home.ics")
        );
    }

    #[test]
    fn conflict_plans_match_the_three_choices() {
        let path = "Calendars/Team.ics";
        assert_eq!(
            plan_conflict(ConflictChoice::ReloadTheirs, path, Some("3-aa"), &[]),
            WritePlan::Reload
        );
        assert_eq!(
            plan_conflict(ConflictChoice::KeepMine, path, Some("3-aa"), &[]),
            WritePlan::Overwrite {
                path: path.into(),
                if_match: Some("3-aa".into()),
            }
        );
        assert_eq!(
            plan_conflict(ConflictChoice::SaveACopy, path, Some("3-aa"), &[]),
            WritePlan::Copy {
                path: "Calendars/Team (copy).ics".into(),
            }
        );
    }

    #[test]
    fn write_outcomes_use_412_or_legacy_409() {
        assert_eq!(
            interpret_write(200, Some("2-bb".into()), None),
            WriteOutcome::Saved {
                etag: Some("2-bb".into())
            }
        );
        assert_eq!(
            interpret_write(412, Some("2-bb".into()), None),
            WriteOutcome::Conflict {
                current_etag: Some("2-bb".into())
            }
        );
        assert_eq!(
            interpret_write(409, Some("2-bb".into()), None),
            WriteOutcome::Conflict {
                current_etag: Some("2-bb".into())
            }
        );
        assert!(matches!(interpret_write(500, None, Some("no".into())), WriteOutcome::Failed(_)));
    }

    #[test]
    fn autosave_starts_the_clock_on_first_dirty_tick() {
        // An edit long after startup must not write on the same frame.
        assert_eq!(arm_autosave(0.0, 30.0, true, AUTOSAVE_DELAY_SECS), (30.0, false));
        assert_eq!(arm_autosave(30.0, 31.0, true, AUTOSAVE_DELAY_SECS), (30.0, false));
        assert_eq!(arm_autosave(30.0, 31.5, true, AUTOSAVE_DELAY_SECS), (31.5, true));
        // After a write (or once everything is clean) the next edit starts a new window.
        assert_eq!(arm_autosave(31.5, 32.0, false, AUTOSAVE_DELAY_SECS), (0.0, false));
        assert_eq!(arm_autosave(0.0, 32.0, true, AUTOSAVE_DELAY_SECS), (32.0, false));
        assert_eq!(arm_autosave(0.0, 10.0, false, AUTOSAVE_DELAY_SECS), (0.0, false));
    }

    #[test]
    fn interval_falls_back_and_has_a_floor() {
        assert_eq!(sync_interval_secs(0), 120);
        assert_eq!(sync_interval_secs(120), 120);
        assert_eq!(sync_interval_secs(5), MIN_SYNC_SECS);
        assert_eq!(sync_interval_secs(600), 600);
    }

    #[test]
    fn migration_lists_local_calendars_and_removes_them_only_when_asked() {
        let mut store = CalendarStore::empty();
        let (acc, cal) = local_cal("cal_local_1", "Personal");
        store.accounts.push(acc);
        store.calendars.push(cal);
        store.events.push(CalendarEvent {
            id: "e1".into(),
            calendar_id: "cal_local_1".into(),
            title: "Lunch".into(),
            date: NaiveDate::from_ymd_opt(2026, 11, 26).unwrap(),
            ..Default::default()
        });
        let pending = pending_exports(&store, &[]);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].suggested_path, "Calendars/Personal.ics");
        // Not removed until the write succeeds.
        assert_eq!(store.calendars.len(), 1);
        remove_exported_local(&mut store, "cal_local_1");
        assert!(store.calendars.is_empty());
        assert!(store.events.is_empty());
        assert!(!store.accounts.iter().any(|a| a.id == "acc_local"));
    }

    #[test]
    fn migration_skips_calendars_that_already_have_a_file_id() {
        let mut store = CalendarStore::empty();
        let (acc, mut cal) = local_cal("cal_local_1", "Personal");
        cal.file_id = Some("Calendars/Personal.ics".into());
        store.accounts.push(acc);
        store.calendars.push(cal);
        assert!(pending_exports(&store, &[]).is_empty());
    }
}
