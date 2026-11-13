// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Find and Replace in the message being written (Format Text › Editing): what to look for, what
//! to put in its place and whether letter case counts. The dialog only collects them: Find Next
//! runs `edit.find`, Replace and Replace All `edit.replace`, and the text shows what they select.
//! Replace All is one step of the message's undo.

use egui::{Align, RichText, Ui, vec2};
use mailcraft_model::rich::MAX_FIND;
use serde_json::{Value, json};

use crate::MailApp;
use crate::dialogs::button_row;
use crate::theme::Tokens;
use crate::widgets::primary_button;

pub const WIDTH: f32 = 420.0;

/// The interface's reading edge, independent of the message text being searched.
fn reading_align(hebrew: bool) -> Align {
    if hebrew { Align::RIGHT } else { Align::LEFT }
}

/// Keep button hit targets and command semantics unchanged while placing the primary
/// Find Next action at the Hebrew dialog's right reading edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Close,
    ReplaceAll,
    ReplaceOne,
    FindNext,
}

fn actions(hebrew: bool, replace: bool) -> Vec<Action> {
    let mut order = vec![Action::Close];
    if replace {
        order.extend([Action::ReplaceAll, Action::ReplaceOne]);
    }
    order.push(Action::FindNext);
    if hebrew {
        order.reverse();
    }
    order
}

fn field_label(ui: &mut Ui, text: &str, hebrew: bool) {
    if hebrew {
        ui.add_sized(vec2(ui.available_width(), 20.0), egui::Label::new(text).halign(Align::RIGHT));
    } else {
        ui.label(text);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindReplace {
    pub text: String,
    pub with: String,
    pub match_case: bool,
    /// Opened for Replace: the field and the buttons for it are shown.
    pub replace: bool,
    /// What the last search or replacement came to.
    pub note: String,
}

impl FindReplace {
    /// Opened from the message being written (or the signature open), the text looked for to
    /// begin with being what is selected, when that is one short line.
    pub fn open(app: &MailApp, replace: bool) -> Option<FindReplace> {
        let (body, _) = app.session.doc()?;
        let sel = body.selection();
        let selected: String =
            if sel.is_empty() { String::new() } else { body.plain().chars().skip(sel.start()).take(sel.end() - sel.start()).collect() };
        let text = if selected.contains('\n') || selected.chars().count() > MAX_FIND { String::new() } else { selected };
        Some(FindReplace { text, replace, ..FindReplace::default() })
    }

    /// The dialog's contents; whether it is done.
    pub fn show(&mut self, app: &mut MailApp, ui: &mut Ui, enter: bool) -> bool {
        let t = Tokens::get(ui.ctx());
        // Hebrew becomes selectable with the Hebrew catalogue integration. This is
        // deliberately a layout-only change until that separate source lands.
        let hebrew = matches!(app.language().code(), "he" | "iw");
        let mut done = false;
        field_label(ui, tl!("Find what"), hebrew);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.text).char_limit(MAX_FIND).horizontal_align(reading_align(hebrew)).desired_width(f32::INFINITY),
        );
        let resp = crate::text_menu::field(resp);
        app.hit.add("findText", 0, resp.rect);
        if !resp.has_focus() && ui.memory(|m| m.focused().is_none()) {
            resp.request_focus();
        }
        if self.replace {
            field_label(ui, tl!("Replace with"), hebrew);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.with).char_limit(MAX_FIND).horizontal_align(reading_align(hebrew)).desired_width(f32::INFINITY),
            );
            app.hit.add("replaceText", 0, crate::text_menu::field(resp).rect);
        }
        let case = if hebrew {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| ui.checkbox(&mut self.match_case, tl!("Match case"))).inner
        } else {
            ui.checkbox(&mut self.match_case, tl!("Match case"))
        };
        app.hit.add("findMatchCase", 0, case.rect);
        if !self.note.is_empty() {
            if hebrew {
                ui.add_sized(
                    vec2(ui.available_width(), 20.0),
                    egui::Label::new(RichText::new(self.note.as_str()).color(t.text_dim)).halign(Align::RIGHT),
                );
            } else {
                ui.label(RichText::new(self.note.as_str()).color(t.text_dim));
            }
        }
        let can = !self.text.is_empty() && !self.text.contains(['\n', '\r']);
        let (mut next, mut one, mut all) = (enter && can, false, false);
        ui.add_space(4.0);
        button_row(ui, |ui| {
            for action in actions(hebrew, self.replace) {
                match action {
                    Action::Close => {
                        let b = ui.add(egui::Button::new(tl!("Close")).min_size(vec2(72.0, 26.0)));
                        app.hit.add("findClose", 0, b.rect);
                        done |= b.clicked();
                    }
                    Action::ReplaceAll => {
                        let b = ui.add_enabled(can, egui::Button::new(tl!("Replace All")).min_size(vec2(72.0, 26.0)));
                        app.hit.add("replaceAll", 0, b.rect);
                        all = b.clicked();
                    }
                    Action::ReplaceOne => {
                        let b = ui.add_enabled(can, egui::Button::new(tl!("Replace")).min_size(vec2(72.0, 26.0)));
                        app.hit.add("replaceOne", 0, b.rect);
                        one = b.clicked();
                    }
                    Action::FindNext => {
                        let b = primary_button(ui, tl!("Find Next"), can);
                        app.hit.add("findNext", 0, b.rect);
                        next |= b.clicked();
                    }
                }
            }
        });
        let base = json!({"text": self.text, "matchCase": self.match_case});
        let with = |mut v: Value, extra: Value| {
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            v
        };
        let asked = if all {
            Some(("edit.replace", with(base, json!({"with": self.with, "all": true}))))
        } else if one {
            Some(("edit.replace", with(base, json!({"with": self.with}))))
        } else if next {
            Some(("edit.find", base))
        } else {
            None
        };
        if let Some((command, params)) = asked {
            self.note = match app.execute(command, params) {
                Ok(v) => said(&v, all),
                Err(e) => tl!(&e).to_string(),
            };
        }
        done
    }
}

/// What a search or a replacement came to, in a line.
fn said(v: &Value, all: bool) -> String {
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    if all {
        return tln!(n("replaced"), "{n} replacement was made.", "{n} replacements were made.");
    }
    if v.get("found").and_then(Value::as_bool) != Some(true) {
        return tl!("The text wasn't found.").to_string();
    }
    String::new()
}

#[cfg(test)]
#[path = "find_replace_tests.rs"]
mod find_replace_tests;
