// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! A message file read without being filed (#515): `.eml` handed over by the system (a double
//! click, a later start) opens in a window of its own that is read-only and holds the message in
//! memory only. Nothing is added to the mailbox until the owner says where it goes (Move to
//! Folder, which is `file.import` of the same bytes into the folder chosen); Save As writes the
//! file's bytes. Closing the window leaves no trace.
//!
//! The message goes through the display path of every message (`crates/html`, the same checks);
//! its own pictures (`cid:`) are shown and pictures on the web are never fetched for it (it has no
//! folder or sender record to judge by: file it to have them asked about). Its attachments are
//! listed; they are opened and saved once the message is filed.

use std::sync::Arc;

use egui::Ui;
use mailcraft_model::Message;
use serde_json::{Value, json};

use crate::MailApp;

/// The largest message file shown this way, in bytes; a larger one is brought in with File ›
/// Import, which reads up to the engine's own bound. (The file stays in memory beside its parsed
/// message while the window is open.)
pub const MAX_BYTES: u64 = 64 << 20;

/// A message file read, ready for a window: its message and its bytes.
pub(crate) struct Opened {
    pub message: Arc<Message>,
    pub bytes: Arc<Vec<u8>>,
}

/// Read and parse the message file at `path`, adding nothing anywhere.
pub(crate) fn read(path: &str) -> Result<Opened, String> {
    let file = std::path::Path::new(path);
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    let bytes = read_capped(file).map_err(|e| format!("{name}: {e}"))?;
    let message = mailcraft_engine::io::parse_unfiled(&name, &bytes).map_err(|e| e.to_string())?;
    Ok(Opened { message: Arc::new(message), bytes: Arc::new(bytes) })
}

#[cfg(not(target_arch = "wasm32"))]
fn read_capped(file: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    // Opened once and checked through that handle, as every file named from outside is.
    let f = mailcraft_engine::io::regular_file(file).map_err(|e| e.to_string())?;
    let too_large = || crate::i18n::fmt(tl!("larger than {mb} MB: bring it in with File › Import"), &[("mb", &(MAX_BYTES >> 20).to_string())]);
    if f.metadata().map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    f.take(MAX_BYTES.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_BYTES) {
        return Err(too_large());
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn read_capped(_: &std::path::Path) -> Result<Vec<u8>, String> {
    Err(tl!("The browser opens no files from the system").to_string())
}

/// File the message into `folder` (`file.import` of the same bytes, as File › Import does): the
/// mailbox gains exactly one message. Answers its id.
pub(crate) fn file_into(app: &mut MailApp, bytes: &[u8], folder: u64) -> Result<u64, String> {
    let data = mailcraft_engine::cmd::base64_encode(bytes);
    let r = app.run("file.import", json!({"name": "message.eml", "data": data, "folder": folder}))?;
    r.get("ids")
        .and_then(Value::as_array)
        .and_then(|ids| ids.first())
        .and_then(Value::as_u64)
        .ok_or_else(|| tl!("The message wasn't filed").to_string())
}

/// What the bar of the window asks.
#[derive(Default)]
pub(crate) struct Asked {
    pub folder: Option<u64>,
    pub save: bool,
}

/// Keep a standalone .eml window's file actions on the reading edge in Hebrew.
/// The action order stays logical: Move to Folder, followed by Save As.
fn file_toolbar_layout(locale: &str) -> egui::Layout {
    if matches!(locale, "he" | "iw") { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) }
}

/// The bar of a window of a message file: Move to Folder, Save As.
pub(crate) fn bar(app: &mut MailApp, ui: &mut Ui) -> Asked {
    let mut asked = Asked::default();
    ui.with_layout(file_toolbar_layout(crate::i18n::current().code()), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
        ui.menu_button(tl!("Move to Folder"), |ui| {
            crate::ribbon::folder_menu(ui, app, |_, folder| asked.folder = Some(folder));
        });
        if crate::widgets::icon_text_button(ui, "save", tl!("Save As…"), true).clicked() {
            asked.save = true;
        }
    });
    asked
}

#[cfg(test)]
mod file_toolbar_tests {
    use super::file_toolbar_layout;

    fn action_centers(locale: &str) -> (f32, f32) {
        let mut centers = None;
        egui::__run_test_ui(|ui| {
            ui.set_width(360.0);
            ui.with_layout(file_toolbar_layout(locale), |ui| {
                let move_to = ui.button("Move to Folder");
                let save_as = ui.button("Save As");
                centers = Some((move_to.rect.center().x, save_as.rect.center().x));
            });
        });
        centers.expect("both file actions must be laid out")
    }

    #[test]
    fn standalone_eml_file_actions_follow_the_hebrew_reading_edge() {
        for locale in ["he", "iw"] {
            let (move_to, save_as) = action_centers(locale);
            assert!(move_to > save_as, "{locale}: Move to Folder must be to the right of Save As");
        }
        for locale in ["en", "fr", "de", "es", "it", "nl", "ro", "zh_CN", "pseudo"] {
            let (move_to, save_as) = action_centers(locale);
            assert!(move_to < save_as, "{locale}: original action order must remain left-to-right");
        }
    }
}
