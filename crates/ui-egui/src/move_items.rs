// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Move Items and Copy Items (#1176): where the selected messages go when Move to Folder or Copy to
//! Folder runs without a folder (Ctrl+Shift+V and Ctrl+Shift+Y, the menu bar's items, Search
//! Commands). The folders listed are those Home › Move lists: the account in view's, as a tree in
//! the same order. Typing narrows the list to the folders whose place matches and marks the first;
//! the arrow keys, Enter, a click and a double-click choose, and OK runs `mail.move` or `mail.copy`
//! with the folder chosen, for the messages selected then, as the ribbon's Move menu runs them
//! (`MailApp::run`: what the window says after a move is said here too). The folders are listed
//! as they were when the dialog opened; a search looks through all of them, and as many rows as
//! the list shows at most are drawn.
//!
//! The dialog moves the selection and nothing else: it takes no ids, so that whoever opens it (an
//! agent may, through `ui.dialog`) can't have the owner move messages they didn't select.

use egui::{Align2, Key, Ui, vec2};
use std::collections::HashMap;

use mailcraft_model::{Folder, FolderKind, Id, Mailbox};
use serde_json::{Value, json};

use crate::MailApp;
use crate::theme::{Tokens, regular};

/// Most characters of the search box.
const QUERY_MAX: usize = 120;
/// Most rows drawn, whatever the mailbox holds.
const ROWS_MAX: usize = 5_000;
/// Most folders read for the list and its search.
const FOLDERS_MAX: usize = 100_000;
/// How deep the tree is walked.
const DEPTH_MAX: usize = 32;

/// Keep typed folder searches on the reading edge, without changing their query semantics.
fn move_search_alignment(language: crate::i18n::Lang) -> Align2 {
    if language == crate::i18n::Lang::Hebrew { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER }
}

/// Text anchor and available width for a folder row. Tree depth belongs to the right
/// edge in Hebrew, just as it belongs to the left edge in English.
fn folder_row_bounds(rect: egui::Rect, depth: usize, searching: bool, language: crate::i18n::Lang) -> (f32, f32) {
    let indent = if searching { 0.0 } else { 16.0 * depth.min(12) as f32 };
    let inset = 10.0 + indent;
    if language == crate::i18n::Lang::Hebrew {
        let right = (rect.max.x - inset).max(rect.min.x + 10.0);
        (right, (right - rect.min.x - 10.0).max(0.0))
    } else {
        let left = (rect.min.x + inset).min(rect.max.x - 10.0);
        (left, (rect.max.x - 10.0 - left).max(0.0))
    }
}

/// The right-anchored counterpart of theme::paint_clipped, with the same one-line
/// ellipsis behavior. Moving the galley rather than the click rectangle preserves
/// the folder row's existing pointer/keyboard targets.
fn paint_right_clipped(painter: &egui::Painter, right: f32, center_y: f32, width: f32, text: &str, font: egui::FontId, color: egui::Color32) {
    if width <= 4.0 || text.is_empty() {
        return;
    }
    let mut job =
        egui::text::LayoutJob::single_section(text.to_owned(), egui::text::TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    let size = galley.size();
    painter.galley(egui::pos2(right - size.x, center_y - size.y / 2.0), galley, color);
}

/// The dialog's state.
pub struct MoveItems {
    /// Copy Items, not Move Items.
    pub copy: bool,
    query: String,
    /// The account's folders, as a tree, read once when the dialog opened.
    rows: Vec<Row>,
    /// The folder the keys or a click marked.
    marked: Option<Id>,
    error: String,
}

/// One folder of the list.
struct Row {
    folder: Id,
    name: String,
    /// Its place in its account, shown for a search's matches.
    path: String,
    /// The place folded as a search folds it, worked out once.
    fold: String,
    depth: usize,
}

/// What the dialog did in a frame.
pub enum Outcome {
    Open,
    Close,
}

impl MoveItems {
    /// Move Items (`copy` false) or Copy Items for the messages selected; `None` when none is, or
    /// there is no account to list.
    pub fn open(app: &MailApp, copy: bool) -> Option<MoveItems> {
        if app.session.sel.messages.is_empty() {
            return None;
        }
        // The account Home › Move lists: the folder in view's, else the default one.
        let mb = &app.session.mailbox;
        let account = mb.folder(app.session.view.folder).map(|f| f.account).or(mb.default_account().map(|a| a.id))?;
        let rows = rows(mb, account);
        Some(MoveItems { copy, query: String::new(), rows, marked: None, error: String::new() })
    }

    /// The dialog's name, as `ui.dialog` takes it.
    pub fn name(&self) -> &'static str {
        if self.copy { "copyItems" } else { "moveItems" }
    }

    /// The window's title, in the interface language.
    pub fn title(&self) -> &'static str {
        if self.copy { tl!("Copy Items") } else { tl!("Move Items") }
    }

    /// The folder marked now, for tests.
    #[cfg(test)]
    pub fn marked(&self) -> Option<Id> {
        self.marked
    }

    /// The folders listed, in order: each with its depth and its place, for tests.
    #[cfg(test)]
    pub fn listed(&self) -> Vec<(Id, usize, String)> {
        self.rows.iter().map(|r| (r.folder, r.depth, r.path.clone())).collect()
    }

    /// Marks `folder`, as a click on its row does.
    #[cfg(test)]
    pub fn mark(&mut self, folder: Id) {
        self.marked = Some(folder);
    }

    /// The command OK runs with the folder marked; `None` while none is.
    pub fn command(&self) -> Option<(&'static str, Value)> {
        let folder = self.marked?;
        Some((if self.copy { "mail.copy" } else { "mail.move" }, json!({"folder": folder})))
    }

    /// Runs the command with the folder marked: the dialog closes when it is done, and says why when
    /// it isn't.
    pub fn go(&mut self, app: &mut MailApp) -> Outcome {
        let Some((id, params)) = self.command() else { return Outcome::Open };
        match app.run(id, params) {
            Ok(_) => Outcome::Close,
            Err(e) => {
                self.error = e;
                Outcome::Open
            }
        }
    }

    /// The dialog's body: the search box, the list and OK and Cancel. `enter` is the frame's Enter.
    pub fn ui(&mut self, ui: &mut Ui, app: &mut MailApp, t: &Tokens, enter: bool) -> Outcome {
        // (A move into the folder in view moves nothing: it isn't offered, as in Home › Move.)
        let here = (!self.copy).then_some(app.session.view.folder);
        ui.label(if self.copy { tl!("Copy the selected items to the folder:") } else { tl!("Move the selected items to the folder:") });
        let language = crate::i18n::current();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text(tl!("Search folders"))
                .char_limit(QUERY_MAX)
                .align(move_search_alignment(language))
                .desired_width(f32::INFINITY),
        );
        let resp = crate::text_menu::field(resp);
        crate::text_menu::hold(&resp);
        let wanted = mailcraft_model::words::fold(&self.query);
        // (Every folder is searched; the rows drawn stop at the most a list shows.)
        let shown: Vec<usize> = (0..self.rows.len()).filter(|i| wanted.is_empty() || self.rows[*i].fold.contains(&wanted)).take(ROWS_MAX).collect();
        let choosable: Vec<Id> = shown.iter().map(|i| self.rows[*i].folder).filter(|f| Some(*f) != here).collect();
        // A search marks its first match; a folder that is no longer listed isn't marked.
        if resp.changed() {
            self.marked = if wanted.is_empty() { None } else { choosable.first().copied() };
        }
        if self.marked.is_some_and(|m| !choosable.contains(&m)) {
            self.marked = None;
        }
        let (down, up) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp)));
        if (down || up) && !choosable.is_empty() {
            let last = choosable.len() - 1;
            let at = self.marked.and_then(|m| choosable.iter().position(|f| *f == m));
            let next = match (at, down) {
                (None, true) => 0,
                (None, false) => last,
                (Some(i), true) => {
                    if i >= last {
                        0
                    } else {
                        i + 1
                    }
                }
                (Some(i), false) => i.checked_sub(1).unwrap_or(last),
            };
            self.marked = choosable.get(next).copied();
        }
        let mut chosen = false;
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
            for &i in &shown {
                let row = &self.rows[i];
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), egui::Sense::click());
                let can = Some(row.folder) != here;
                // (Painted: its name, whether it is marked and whether it can be, for a screen reader.)
                let marked = self.marked == Some(row.folder);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListBoxOption, can, &row.path));
                // (A folder is chosen, AccessKit's `selected`, not switched on: as in `gmail_tabs`.)
                ui.ctx().accesskit_node_builder(resp.id, |node| node.set_selected(marked));
                if marked {
                    ui.painter().rect_filled(r, 4.0, t.selection);
                    if down || up {
                        ui.scroll_to_rect(r, None);
                    }
                } else if can && resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, t.hover);
                }
                let searching = !wanted.is_empty();
                let text = if searching { &row.path } else { &row.name };
                let ink = if can { t.text } else { t.text_disabled };
                let (anchor, width) = folder_row_bounds(r, row.depth, searching, language);
                if language == crate::i18n::Lang::Hebrew {
                    paint_right_clipped(ui.painter(), anchor, r.center().y, width, text, regular(13.0), ink);
                } else {
                    crate::theme::paint_clipped(ui.painter(), egui::pos2(anchor, r.center().y), width, text, regular(13.0), ink);
                }
                if can && resp.clicked() {
                    self.marked = Some(row.folder);
                }
                if can && resp.double_clicked() {
                    self.marked = Some(row.folder);
                    chosen = true;
                }
            }
            if shown.is_empty() {
                ui.label(egui::RichText::new(tl!("No folders match")).color(t.text_dim));
            }
        });
        if !self.error.is_empty() {
            ui.label(egui::RichText::new(self.error.as_str()).color(t.red));
        }
        let (ok, cancel) = crate::dialogs::buttons(ui, tl!("OK"), self.marked.is_some());
        if cancel {
            return Outcome::Close;
        }
        if ok || chosen || (enter && self.marked.is_some()) { self.go(app) } else { Outcome::Open }
    }
}

/// The folders of `account`, as a tree in the order Home › Move lists them ([`Mailbox::children`]'s
/// order), each with its place in the account. Built in one pass over the folders, so that an
/// account with very many of them opens the dialog without a wait.
fn rows(mb: &Mailbox, account: Id) -> Vec<Row> {
    let rank = |f: &Folder| FolderKind::STANDARD.iter().position(|k| *k == f.kind).unwrap_or(usize::MAX);
    let mut under: HashMap<Option<Id>, Vec<&Folder>> = HashMap::new();
    for f in mb.folders.iter().filter(|f| f.account == account) {
        under.entry(f.parent).or_default().push(f);
    }
    for v in under.values_mut() {
        v.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    }
    fn walk(under: &HashMap<Option<Id>, Vec<&Folder>>, parent: Option<Id>, above: &str, depth: usize, out: &mut Vec<Row>) {
        if depth > DEPTH_MAX {
            return;
        }
        for f in under.get(&parent).into_iter().flatten() {
            if out.len() >= FOLDERS_MAX {
                return;
            }
            let name = crate::nav::folder_name(f).to_string();
            let path = if above.is_empty() { name.clone() } else { format!("{above}/{name}") };
            let fold = mailcraft_model::words::fold(&path);
            out.push(Row { folder: f.id, name, path: path.clone(), fold, depth });
            walk(under, Some(f.id), &path, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(&under, None, "", 0, &mut out);
    out
}


#[cfg(test)]
mod hebrew_move_dialog_layout_tests {
    use super::*;

    #[test]
    fn move_and_copy_folder_search_read_from_the_correct_edge() {
        assert_eq!(move_search_alignment(crate::i18n::Lang::Hebrew), Align2::RIGHT_CENTER);
        for language in [crate::i18n::Lang::English, crate::i18n::Lang::Pseudo] {
            assert_eq!(move_search_alignment(language), Align2::LEFT_CENTER);
        }
    }

    #[test]
    fn move_and_copy_folder_rows_mirror_depth_without_changing_hit_areas() {
        for width in [84.0, 180.0, 520.0] {
            let rect = egui::Rect::from_min_size(egui::pos2(14.0, 24.0), vec2(width, 26.0));
            for depth in [0, 1, 4, 12, 24] {
                for searching in [false, true] {
                    let (left, left_width) = folder_row_bounds(rect, depth, searching, crate::i18n::Lang::English);
                    let (right, right_width) = folder_row_bounds(rect, depth, searching, crate::i18n::Lang::Hebrew);
                    assert!(left >= rect.min.x + 10.0 && left <= rect.max.x - 10.0);
                    assert!(right >= rect.min.x + 10.0 && right <= rect.max.x - 10.0);
                    assert!((left + right - rect.min.x - rect.max.x).abs() < 0.01);
                    assert!((left_width - right_width).abs() < 0.01);
                    if searching {
                        assert!((left - rect.min.x - 10.0).abs() < 0.01);
                        assert!((rect.max.x - right - 10.0).abs() < 0.01);
                    }
                }
            }
        }
    }
}
