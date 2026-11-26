//! File-backed calendars in a host: create, load, save, poll, migrate, and the conflict dialog.

use crate::filesync::{self, ConflictChoice, PollAction, WriteOutcome, ACC_FILES, DEFAULT_FOLDER};
use crate::host::{self, FileReply as HostFileReply, HostMsg};
use crate::i18n::t;
use crate::ics;
use crate::model::{AccountKind, CalendarSource};

use super::{today, CalendarCraftApp, FileDoc};

/// A calendar stored as an `.ics` file in the host's drive, referenced by path (file id).
#[derive(Debug, Clone)]
pub struct FileCal {
    pub path: String,
    pub etag: Option<String>,
    pub dirty: bool,
    pub name: String,
    pub calendar_id: String,
}

/// A file that changed both on disk and in this session.
#[derive(Debug, Clone)]
pub struct FileConflict {
    pub calendar_id: String,
    pub path: String,
    pub name: String,
    pub remote_etag: Option<String>,
}

/// Feedback while local store calendars are exported to files, or after that finishes.
#[derive(Debug, Clone, Default)]
pub struct MigrationNotice {
    pub pending: usize,
    pub saved: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum PendingFile {
    Create { name: String, color: String },
    Load { calendar_id: String },
    Stat { calendar_id: String },
    Save { calendar_id: String },
    Migrate { calendar_id: String, name: String },
}

impl CalendarCraftApp {
    pub(super) fn drop_ephemeral_file_accounts(store: &mut crate::model::CalendarStore) {
        let keep: std::collections::HashSet<String> = store
            .calendars
            .iter()
            .filter(|c| filesync::is_file_backed(c))
            .map(|c| c.account_id.clone())
            .collect();
        store
            .accounts
            .retain(|a| a.kind != AccountKind::File || keep.contains(&a.id));
        store
            .calendars
            .retain(|c| c.account_id != "acc_file" || filesync::is_file_backed(c));
        let kept_cals: Vec<String> = store.calendars.iter().map(|c| c.id.clone()).collect();
        store.events.retain(|e| e.calendar_id != "cal_file" || kept_cals.contains(&e.calendar_id));
    }

    pub(super) fn restore_file_cals_from_store(&mut self) {
        for cal in &self.store.calendars {
            let Some(path) = cal.file_id.clone() else {
                continue;
            };
            if path.is_empty() {
                continue;
            }
            self.file_cals.insert(
                cal.id.clone(),
                FileCal {
                    path: path.clone(),
                    etag: cal.file_etag.clone(),
                    dirty: false,
                    name: file_name_of(&path),
                    calendar_id: cal.id.clone(),
                },
            );
        }
    }

    /// In a host, a "local" calendar is an `.ics` file in `Calendars/`. Elsewhere it stays in the store.
    pub fn add_local_calendar(&mut self, name: &str) {
        if host::hosted() {
            self.create_file_calendar(name);
            return;
        }
        self.add_store_local_calendar(name);
    }

    pub(super) fn add_store_local_calendar(&mut self, name: &str) {
        if !self.store.accounts.iter().any(|a| a.id == "acc_local") {
            self.store.accounts.push(crate::model::AccountConfig {
                id: "acc_local".into(),
                name: t("On this device").into(),
                provider: "Local".into(),
                enabled: true,
                kind: AccountKind::Local,
                ..Default::default()
            });
        }
        let id = format!(
            "cal_local_{}",
            ics::short_hash(&format!("{name}{}", self.store.calendars.len()))
        );
        let color = self.next_color();
        self.store.calendars.push(CalendarSource {
            id: id.clone(),
            account_id: "acc_local".into(),
            name: name.to_string(),
            color_hex: color,
            visible: true,
            ..Default::default()
        });
        self.add_to_all_set(&id);
        self.dirty_store = true;
    }

    pub(super) fn create_file_calendar(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let color = self.next_color();
        let text = ics::write_calendar(Some(name), std::iter::empty());
        let file_name = filesync::safe_file_name(name);
        let id = self.file_request(PendingFile::Create {
            name: name.to_string(),
            color,
        });
        host::post_file(
            "create",
            &id,
            None,
            Some(&file_name),
            Some(DEFAULT_FOLDER),
            Some(text.as_bytes()),
            None,
        );
        self.busy = self.busy.saturating_add(1);
    }

    pub(super) fn start_host_file_setup(&mut self) {
        if !host::hosted() || self.launch.demo {
            return;
        }
        self.restore_file_cals_from_store();
        let exports = filesync::pending_exports(&self.store, &self.known_file_paths());
        if !exports.is_empty() {
            self.migration = Some(MigrationNotice {
                pending: exports.len(),
                saved: Vec::new(),
                failed: Vec::new(),
            });
            self.flash(t("Saving your calendars as files…"), false);
            for item in exports {
                self.export_local_calendar(&item.calendar_id, &item.name, &item.suggested_path);
            }
        }
        let ids: Vec<String> = self.file_cals.keys().cloned().collect();
        for id in ids {
            self.request_file_load(&id);
        }
    }

    fn export_local_calendar(&mut self, calendar_id: &str, name: &str, path: &str) {
        let events: Vec<&crate::model::CalendarEvent> = self
            .store
            .events
            .iter()
            .filter(|e| e.calendar_id == calendar_id)
            .collect();
        let text = ics::write_calendar(Some(name), events);
        let file_name = file_name_of(path);
        let id = self.file_request(PendingFile::Migrate {
            calendar_id: calendar_id.to_string(),
            name: name.to_string(),
        });
        host::post_file(
            "create",
            &id,
            Some(path),
            Some(&file_name),
            Some(DEFAULT_FOLDER),
            Some(text.as_bytes()),
            None,
        );
        self.busy = self.busy.saturating_add(1);
    }

    pub(super) fn request_file_load(&mut self, calendar_id: &str) {
        let Some(path) = self.file_cals.get(calendar_id).map(|f| f.path.clone()) else {
            return;
        };
        let id = self.file_request(PendingFile::Load {
            calendar_id: calendar_id.to_string(),
        });
        host::post_file("read", &id, Some(&path), None, None, None, None);
        self.busy = self.busy.saturating_add(1);
    }

    pub(super) fn request_file_sync(&mut self, force: bool) {
        if !host::hosted() || self.file_cals.is_empty() {
            return;
        }
        if self.file_conflict.is_some() {
            return;
        }
        let interval = filesync::sync_interval_secs(self.store.settings.file_sync_interval_secs) as f64;
        if !force && self.last_file_sync > 0.0 && self.now - self.last_file_sync < interval {
            return;
        }
        // Avoid a burst if we just synced (e.g. focus + visibility together).
        if !force && self.last_file_sync > 0.0 && self.now - self.last_file_sync < 2.0 {
            return;
        }
        if self
            .pending_files
            .values()
            .any(|p| matches!(p, PendingFile::Stat { .. } | PendingFile::Load { .. }))
        {
            return;
        }
        self.last_file_sync = self.now;
        let ids: Vec<(String, String)> = self
            .file_cals
            .iter()
            .map(|(id, f)| (id.clone(), f.path.clone()))
            .collect();
        for (calendar_id, path) in ids {
            let req = self.file_request(PendingFile::Stat { calendar_id });
            host::post_file("stat", &req, Some(&path), None, None, None, None);
        }
    }

    pub(super) fn maybe_autosave_files(&mut self) {
        if !host::hosted() || self.file_conflict.is_some() {
            return;
        }
        // Give a moment after an edit so an external change can still be detected as a conflict,
        // and so we do not write on every frame.
        let dirty: Vec<String> = self
            .file_cals
            .iter()
            .filter(|(_, f)| f.dirty)
            .filter(|(id, _)| {
                !self
                    .pending_files
                    .values()
                    .any(|p| matches!(p, PendingFile::Save { calendar_id } if calendar_id == *id))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let (armed, due) = filesync::arm_autosave(
            self.last_file_autosave,
            self.now,
            !dirty.is_empty(),
            filesync::AUTOSAVE_DELAY_SECS,
        );
        self.last_file_autosave = armed;
        if !due {
            return;
        }
        for id in dirty {
            self.save_file_calendar(&id);
        }
    }

    pub fn save_file_calendar(&mut self, calendar_id: &str) {
        let Some(file) = self.file_cals.get(calendar_id).cloned() else {
            return;
        };
        let cal_name = self
            .store
            .calendar_by_id(calendar_id)
            .map(|c| c.name.clone());
        let events: Vec<&crate::model::CalendarEvent> = self
            .store
            .events
            .iter()
            .filter(|e| e.calendar_id == calendar_id)
            .collect();
        let text = ics::write_calendar(cal_name.as_deref(), events);
        let if_match = file.etag.as_deref();
        let id = self.file_request(PendingFile::Save {
            calendar_id: calendar_id.to_string(),
        });
        host::post_file(
            "write",
            &id,
            Some(&file.path),
            Some(&file.name),
            None,
            Some(text.as_bytes()),
            if_match,
        );
        self.busy = self.busy.saturating_add(1);
    }

    /// `file.save`: the open document, and any other dirty file-backed calendars.
    pub fn save_document(&mut self) {
        if let Some(doc) = self.file_doc.clone() {
            if let Some(path) = doc.path.as_deref().filter(|p| !p.is_empty()) {
                if self.file_cals.contains_key(&doc.calendar_id) {
                    self.save_file_calendar(&doc.calendar_id);
                } else {
                    // Should not happen; fall back to the original save message.
                    self.post_open_save(&doc, path);
                }
            } else {
                self.post_open_save(&doc, "");
            }
        }
        let others: Vec<String> = self
            .file_cals
            .iter()
            .filter(|(_, f)| f.dirty)
            .filter(|(id, _)| self.file_doc.as_ref().map(|d| &d.calendar_id) != Some(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in others {
            self.save_file_calendar(&id);
        }
    }

    fn post_open_save(&mut self, doc: &FileDoc, _path: &str) {
        let cal_name = self
            .store
            .calendar_by_id(&doc.calendar_id)
            .map(|c| c.name.clone());
        let events: Vec<&crate::model::CalendarEvent> = self
            .store
            .events
            .iter()
            .filter(|e| e.calendar_id == doc.calendar_id)
            .collect();
        let text = ics::write_calendar(cal_name.as_deref(), events);
        host::post_save(&doc.name, text.as_bytes());
        if let Some(d) = self.file_doc.as_mut() {
            d.dirty = false;
        }
        host::post_dirty(false);
    }

    pub(super) fn open_hosted_document(
        &mut self,
        name: String,
        bytes: Vec<u8>,
        path: Option<String>,
        etag: Option<String>,
    ) {
        let path = path.filter(|p| !p.is_empty());
        let (acc_id, cal_id) = match &path {
            Some(p) => (ACC_FILES.to_string(), filesync::calendar_id_for(p)),
            None => ("acc_file".to_string(), "cal_file".to_string()),
        };
        if path.is_some() {
            filesync::ensure_files_account(&mut self.store, t("In my files"));
        }
        self.apply_ics_bytes(&cal_id, &acc_id, &name, &bytes, path.clone(), etag.clone());
        self.file_doc = Some(FileDoc {
            name: name.clone(),
            account_id: acc_id,
            calendar_id: cal_id.clone(),
            dirty: false,
            path: path.clone(),
            etag: etag.clone(),
        });
        if let Some(p) = path {
            self.file_cals.insert(
                cal_id.clone(),
                FileCal {
                    path: p,
                    etag,
                    dirty: false,
                    name,
                    calendar_id: cal_id,
                },
            );
            self.dirty_store = true;
        }
        host::post_dirty(false);
    }

    fn apply_ics_bytes(
        &mut self,
        cal_id: &str,
        acc_id: &str,
        file_name: &str,
        bytes: &[u8],
        path: Option<String>,
        etag: Option<String>,
    ) {
        let text = String::from_utf8_lossy(bytes).to_string();
        let parsed = if text.trim().is_empty() {
            Ok(ics::IcsCalendar::default())
        } else {
            ics::parse(&text, cal_id)
        };
        match parsed {
            Ok(cal) => {
                self.store.events.retain(|e| e.calendar_id != cal_id);
                let color = cal
                    .color
                    .as_deref()
                    .and_then(crate::caldav::normalize_color)
                    .unwrap_or_else(|| {
                        self.store
                            .calendars
                            .iter()
                            .find(|c| c.id == cal_id)
                            .map(|c| c.color_hex.clone())
                            .unwrap_or_else(|| self.next_color())
                    });
                let display = cal
                    .name
                    .clone()
                    .unwrap_or_else(|| file_name.trim_end_matches(".ics").to_string());
                if let Some(existing) = self.store.calendars.iter_mut().find(|c| c.id == cal_id) {
                    existing.name = display;
                    existing.color_hex = color;
                    existing.file_id = path.clone();
                    existing.file_etag = etag.clone();
                    existing.account_id = acc_id.to_string();
                } else {
                    if !self.store.accounts.iter().any(|a| a.id == acc_id) {
                        self.store.accounts.insert(
                            0,
                            crate::model::AccountConfig {
                                id: acc_id.to_string(),
                                name: file_name.to_string(),
                                provider: t("File").into(),
                                enabled: true,
                                kind: AccountKind::File,
                                ..Default::default()
                            },
                        );
                    }
                    self.store.calendars.insert(
                        0,
                        CalendarSource {
                            id: cal_id.to_string(),
                            account_id: acc_id.to_string(),
                            name: display,
                            color_hex: color,
                            visible: true,
                            file_id: path.clone(),
                            file_etag: etag.clone(),
                            ..Default::default()
                        },
                    );
                    self.add_to_all_set(cal_id);
                }
                let first = cal
                    .events
                    .iter()
                    .map(|e| e.date)
                    .filter(|d| *d >= today())
                    .min()
                    .or_else(|| cal.events.iter().map(|e| e.date).max());
                let count = cal.events.len();
                self.store.events.extend(cal.events);
                if let Some(day) = first {
                    self.select_date(day);
                }
                self.flash(
                    format!("{file_name}: {count} {}", t(if count == 1 { "event" } else { "events" })),
                    false,
                );
            }
            Err(err) => {
                self.flash(format!("{file_name}: {err}"), true);
            }
        }
    }

    pub(super) fn handle_file_reply(&mut self, reply: HostFileReply) {
        self.busy = self.busy.saturating_sub(1);
        let Some(pending) = self.pending_files.remove(&reply.request_id) else {
            return;
        };
        match pending {
            PendingFile::Create { name, color } => self.on_created(&reply, name, color, None),
            PendingFile::Migrate { calendar_id, name } => {
                self.on_migrated(&reply, &calendar_id, &name)
            }
            PendingFile::Load { calendar_id } => self.on_loaded(&reply, &calendar_id),
            PendingFile::Stat { calendar_id } => self.on_stat(&reply, &calendar_id),
            PendingFile::Save { calendar_id } => self.on_saved(&reply, &calendar_id),
        }
    }

    fn on_created(
        &mut self,
        reply: &HostFileReply,
        name: String,
        color: String,
        migrate_from: Option<String>,
    ) {
        if !reply.ok || reply.conflict {
            self.flash(t("Could not save the calendar as a file."), true);
            return;
        }
        let Some(path) = reply.path.clone() else {
            self.flash(t("Could not save the calendar as a file."), true);
            return;
        };
        filesync::ensure_files_account(&mut self.store, t("In my files"));
        let cal_id = filesync::calendar_id_for(&path);
        if !self.store.calendars.iter().any(|c| c.id == cal_id) {
            self.store.calendars.push(CalendarSource {
                id: cal_id.clone(),
                account_id: ACC_FILES.into(),
                name: name.clone(),
                color_hex: color,
                visible: true,
                file_id: Some(path.clone()),
                file_etag: reply.etag.clone(),
                ..Default::default()
            });
            self.add_to_all_set(&cal_id);
        } else if let Some(c) = self.store.calendars.iter_mut().find(|c| c.id == cal_id) {
            c.file_id = Some(path.clone());
            c.file_etag = reply.etag.clone();
            c.name = name.clone();
        }
        self.file_cals.insert(
            cal_id.clone(),
            FileCal {
                path,
                etag: reply.etag.clone(),
                dirty: false,
                name: reply.name.clone().unwrap_or(name),
                calendar_id: cal_id,
            },
        );
        let _ = migrate_from;
        self.dirty_store = true;
    }

    fn on_migrated(&mut self, reply: &HostFileReply, calendar_id: &str, name: &str) {
        if !reply.ok || reply.path.is_none() {
            if let Some(m) = self.migration.as_mut() {
                m.pending = m.pending.saturating_sub(1);
                m.failed.push(name.to_string());
            }
            self.flash(
                format!("{}: {}", name, t("Could not save the calendar as a file.")),
                true,
            );
            self.finish_migration_if_done();
            return;
        }
        let path = reply.path.clone().unwrap();
        let color = self
            .store
            .calendar_by_id(calendar_id)
            .map(|c| c.color_hex.clone())
            .unwrap_or_else(|| self.next_color());
        // Re-home events to the new file-backed calendar, then drop the local source.
        let new_id = filesync::calendar_id_for(&path);
        if new_id != calendar_id {
            for ev in self.store.events.iter_mut() {
                if ev.calendar_id == calendar_id {
                    ev.calendar_id = new_id.clone();
                }
            }
        }
        filesync::ensure_files_account(&mut self.store, t("In my files"));
        if !self.store.calendars.iter().any(|c| c.id == new_id) {
            self.store.calendars.push(CalendarSource {
                id: new_id.clone(),
                account_id: ACC_FILES.into(),
                name: name.to_string(),
                color_hex: color,
                visible: true,
                file_id: Some(path.clone()),
                file_etag: reply.etag.clone(),
                ..Default::default()
            });
            self.add_to_all_set(&new_id);
        }
        self.file_cals.insert(
            new_id.clone(),
            FileCal {
                path,
                etag: reply.etag.clone(),
                dirty: false,
                name: reply.name.clone().unwrap_or_else(|| format!("{name}.ics")),
                calendar_id: new_id,
            },
        );
        filesync::remove_exported_local(&mut self.store, calendar_id);
        self.dirty_store = true;
        if let Some(m) = self.migration.as_mut() {
            m.pending = m.pending.saturating_sub(1);
            m.saved.push(name.to_string());
        }
        self.finish_migration_if_done();
    }

    fn finish_migration_if_done(&mut self) {
        let Some(m) = self.migration.as_ref() else {
            return;
        };
        if m.pending > 0 {
            return;
        }
        if m.failed.is_empty() && !m.saved.is_empty() {
            self.flash(t("Your calendars were saved as files in Calendars."), false);
        } else if !m.failed.is_empty() {
            self.flash(
                format!(
                    "{} {}",
                    t("Could not save the calendar as a file."),
                    m.failed.join(", ")
                ),
                true,
            );
        }
    }

    fn on_loaded(&mut self, reply: &HostFileReply, calendar_id: &str) {
        if !reply.ok {
            self.flash(
                reply
                    .error
                    .clone()
                    .unwrap_or_else(|| t("Could not save the calendar as a file.").to_string()),
                true,
            );
            return;
        }
        let Some(bytes) = reply.bytes.as_ref() else {
            return;
        };
        let acc_id = ACC_FILES.to_string();
        let path = reply
            .path
            .clone()
            .or_else(|| self.file_cals.get(calendar_id).map(|f| f.path.clone()));
        let name = reply
            .name
            .clone()
            .or_else(|| self.file_cals.get(calendar_id).map(|f| f.name.clone()))
            .unwrap_or_else(|| "Calendar.ics".into());
        filesync::ensure_files_account(&mut self.store, t("In my files"));
        self.apply_ics_bytes(calendar_id, &acc_id, &name, bytes, path.clone(), reply.etag.clone());
        if let Some(f) = self.file_cals.get_mut(calendar_id) {
            f.etag = reply.etag.clone();
            f.dirty = false;
            if let Some(p) = path {
                f.path = p;
            }
        }
        if let Some(c) = self.store.calendars.iter_mut().find(|c| c.id == calendar_id) {
            c.file_etag = reply.etag.clone();
        }
        if let Some(doc) = self.file_doc.as_mut() {
            if doc.calendar_id == calendar_id {
                doc.dirty = false;
                doc.etag = reply.etag.clone();
                host::post_dirty(false);
            }
        }
        self.dirty_store = true;
    }

    fn on_stat(&mut self, reply: &HostFileReply, calendar_id: &str) {
        let remote = reply.etag.as_deref();
        let Some(file) = self.file_cals.get(calendar_id).cloned() else {
            return;
        };
        let changed = filesync::etag_changed(file.etag.as_deref(), remote);
        match filesync::poll_action(file.dirty, changed) {
            PollAction::None => {}
            PollAction::Reload => {
                self.request_file_load(calendar_id);
                self.flash(t("Reloaded from your files"), false);
            }
            PollAction::Conflict => {
                if self.file_conflict.is_none() {
                    self.file_conflict = Some(FileConflict {
                        calendar_id: calendar_id.to_string(),
                        path: file.path,
                        name: self
                            .store
                            .calendar_by_id(calendar_id)
                            .map(|c| c.name.clone())
                            .unwrap_or(file.name),
                        remote_etag: reply.etag.clone(),
                    });
                }
            }
        }
    }

    fn on_saved(&mut self, reply: &HostFileReply, calendar_id: &str) {
        let status = if reply.ok {
            200
        } else if reply.conflict {
            412
        } else {
            500
        };
        match filesync::interpret_write(status, reply.etag.clone(), reply.error.clone()) {
            WriteOutcome::Saved { etag } => {
                if let Some(f) = self.file_cals.get_mut(calendar_id) {
                    f.etag = etag.clone();
                    f.dirty = false;
                    if let Some(p) = reply.path.clone() {
                        f.path = p;
                    }
                }
                if let Some(c) = self.store.calendars.iter_mut().find(|c| c.id == calendar_id) {
                    c.file_etag = etag.clone();
                    if let Some(p) = reply.path.clone() {
                        c.file_id = Some(p);
                    }
                }
                if let Some(doc) = self.file_doc.as_mut() {
                    if doc.calendar_id == calendar_id {
                        doc.dirty = false;
                        doc.etag = etag;
                        host::post_dirty(false);
                    }
                }
                self.dirty_store = true;
            }
            WriteOutcome::Conflict { current_etag } => {
                let file = self.file_cals.get(calendar_id).cloned();
                self.file_conflict = Some(FileConflict {
                    calendar_id: calendar_id.to_string(),
                    path: file.as_ref().map(|f| f.path.clone()).unwrap_or_default(),
                    name: self
                        .store
                        .calendar_by_id(calendar_id)
                        .map(|c| c.name.clone())
                        .or_else(|| file.map(|f| f.name))
                        .unwrap_or_default(),
                    remote_etag: current_etag.or(reply.etag.clone()),
                });
            }
            WriteOutcome::Failed(err) => {
                self.flash(err, true);
            }
        }
    }

    pub fn resolve_file_conflict(&mut self, choice: ConflictChoice) {
        let Some(conflict) = self.file_conflict.take() else {
            return;
        };
        let existing = self.known_file_paths();
        let plan = filesync::plan_conflict(
            choice,
            &conflict.path,
            conflict.remote_etag.as_deref(),
            &existing,
        );
        match plan {
            filesync::WritePlan::Reload => {
                if let Some(f) = self.file_cals.get_mut(&conflict.calendar_id) {
                    f.dirty = false;
                    if let Some(tag) = conflict.remote_etag.clone() {
                        f.etag = Some(tag);
                    }
                }
                self.request_file_load(&conflict.calendar_id);
            }
            filesync::WritePlan::Overwrite { path, if_match } => {
                if let Some(f) = self.file_cals.get_mut(&conflict.calendar_id) {
                    f.etag = if_match.clone();
                    f.path = path;
                    f.dirty = true;
                }
                self.save_file_calendar(&conflict.calendar_id);
            }
            filesync::WritePlan::Copy { path } => {
                // Reload the original, and write our events to the copy as a new calendar.
                let cal_id = conflict.calendar_id.clone();
                let cal_name = self
                    .store
                    .calendar_by_id(&cal_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| conflict.name.clone());
                let color = self
                    .store
                    .calendar_by_id(&cal_id)
                    .map(|c| c.color_hex.clone())
                    .unwrap_or_else(|| self.next_color());
                let events: Vec<crate::model::CalendarEvent> = self
                    .store
                    .events
                    .iter()
                    .filter(|e| e.calendar_id == cal_id)
                    .cloned()
                    .collect();
                let text = ics::write_calendar(Some(&cal_name), events.iter());
                let file_name = file_name_of(&path);
                let id = self.file_request(PendingFile::Create {
                    name: copy_calendar_name(&cal_name),
                    color,
                });
                host::post_file(
                    "create",
                    &id,
                    Some(&path),
                    Some(&file_name),
                    None,
                    Some(text.as_bytes()),
                    None,
                );
                self.busy = self.busy.saturating_add(1);
                if let Some(f) = self.file_cals.get_mut(&cal_id) {
                    f.dirty = false;
                }
                self.request_file_load(&cal_id);
                self.flash(t("A copy was saved"), false);
            }
        }
    }

    pub(super) fn mark_file_dirty(&mut self, cal_id: &str) {
        if let Some(f) = self.file_cals.get_mut(cal_id) {
            f.dirty = true;
        }
        if let Some(doc) = self.file_doc.as_mut() {
            if doc.calendar_id == cal_id && !doc.dirty {
                doc.dirty = true;
                host::post_dirty(true);
            }
        }
    }

    pub(super) fn handle_host_msg(&mut self, msg: HostMsg) {
        match msg {
            HostMsg::Open {
                name,
                bytes,
                path,
                etag,
            } => self.open_hosted_document(name, bytes, path, etag),
            HostMsg::File(reply) => self.handle_file_reply(reply),
            HostMsg::SyncHint => {
                self.file_sync_requested = true;
                self.request_file_sync(true);
            }
            HostMsg::Command(cmd, args) => self.handle_host_command(cmd, args),
        }
    }

    fn handle_host_command(&mut self, cmd: String, args: serde_json::Value) {
        match cmd.as_str() {
            "file.save" => self.save_document(),
            "view.today" => self.select_date(today()),
            "sync.refresh" => {
                self.refresh_all();
                self.request_file_sync(true);
            },
            "event.new" => self.open_new_event(self.selected_date, None),
            "calendar.subscribe" => {
                let field = |k: &str| {
                    args.get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                self.account_form = super::AccountForm {
                    kind_ics: true,
                    url: field("url"),
                    name: field("name"),
                    ..Default::default()
                };
                self.add_account_from_form();
                if let Some(err) = self.account_form.error.take() {
                    self.flash(err, true);
                }
            }
            "event.add" => {
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if title.is_empty() {
                    return;
                }
                let date = args
                    .get("date")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .unwrap_or(self.selected_date);
                let calendar_id = args
                    .get("calendar")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| self.default_calendar())
                    .unwrap_or_default();
                if calendar_id.is_empty() {
                    return;
                }
                let ev = crate::model::CalendarEvent {
                    id: format!("e_{}", ics::short_hash(&format!("{title}{date}{}", self.store.events.len()))),
                    calendar_id,
                    title: title.to_string(),
                    date,
                    ..Default::default()
                };
                self.commit_event(ev);
            }
            "calendar.create" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if !name.is_empty() {
                    self.add_local_calendar(name);
                }
            }
            "calendar.syncFiles" => self.request_file_sync(true),
            "calendar.resolveConflict" => {
                let choice = match args.get("choice").and_then(|v| v.as_str()) {
                    Some("theirs") | Some("reload") => ConflictChoice::ReloadTheirs,
                    Some("mine") | Some("keep") => ConflictChoice::KeepMine,
                    Some("copy") => ConflictChoice::SaveACopy,
                    _ => return,
                };
                self.resolve_file_conflict(choice);
            }
            _ => {}
        }
    }

    fn file_request(&mut self, pending: PendingFile) -> String {
        self.next_file_req = self.next_file_req.saturating_add(1);
        let id = self.next_file_req.to_string();
        self.pending_files.insert(id.clone(), pending);
        id
    }

    fn known_file_paths(&self) -> Vec<String> {
        self.file_cals.values().map(|f| f.path.clone()).collect()
    }

    pub(super) fn file_state_json(&self) -> serde_json::Value {
        let calendars: Vec<serde_json::Value> = self
            .file_cals
            .values()
            .map(|f| {
                serde_json::json!({
                    "id": f.calendar_id,
                    "path": f.path,
                    "etag": f.etag,
                    "dirty": f.dirty,
                    "name": f.name,
                })
            })
            .collect();
        let conflict = self.file_conflict.as_ref().map(|c| {
            serde_json::json!({
                "calendar": c.calendar_id,
                "path": c.path,
                "name": c.name,
            })
        });
        serde_json::json!({
            "fileCalendars": calendars,
            "conflict": conflict,
            "migration": self.migration.as_ref().map(|m| serde_json::json!({
                "pending": m.pending,
                "saved": m.saved,
                "failed": m.failed,
            })),
        })
    }
}

fn file_name_of(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

fn copy_calendar_name(name: &str) -> String {
    if name.contains("(copy)") || name.contains("(Copy)") {
        format!("{name} 2")
    } else {
        format!("{name} (copy)")
    }
}
