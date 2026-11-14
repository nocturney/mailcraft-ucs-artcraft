// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Native, synthetic Hebrew mailbox acceptance: create -> save -> close -> new app -> reopen.
//! This exercises real file commands and persistence; it does not pretend to be a human GUI click-through.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use mailcraft_engine::{Session, store};
use mailcraft_model::Mailbox;
use mailcraft_ui_egui::{MailApp, Services};
use serde_json::json;

const NOW: i64 = 1_791_993_600;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("mailcraft-he-roundtrip-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn empty_app() -> MailApp {
    let mailbox = Mailbox::with_local_account("", "");
    let mut app = MailApp::new(Session::at(mailbox, NOW, 0), Services::default());
    app.auto_sync = false;
    app
}

#[test]
fn creates_saves_closes_and_reopens_synthetic_hebrew_mailbox() {
    let scratch = Scratch::new();
    let path = scratch.0.join("hebrew-smoke.mailcraft");
    assert!(!path.exists());

    // Start a new native app, switch the UI to Hebrew and create a synthetic sample mailbox.
    let mut writer = empty_app();
    let language = writer.execute("ui.language", json!({"language": "he"})).unwrap();
    assert_eq!(language["inForce"], json!("he"));
    let new = writer.execute("file.new", json!({"sample": true})).unwrap();
    assert_eq!(new["sample"], json!(true));
    let count = writer.session.mailbox.messages.len();
    assert!(count > 0, "sample mailbox should contain synthetic messages");

    // Change the new mailbox through a real application command before saving.
    let id = writer
        .session
        .mailbox
        .messages
        .values()
        .find(|m| !m.read)
        .expect("a synthetic unread message")
        .id;
    writer.execute("mail.markRead", json!({"id": id})).unwrap();
    assert!(writer.session.mailbox.messages.get(&id).unwrap().read);

    // Use the real Save As command for a new mailbox, then finish normal on-exit saving.
    let saved = writer
        .execute("file.saveAs", json!({"path": path.to_string_lossy().to_string()}))
        .unwrap();
    assert_eq!(saved["copy"], json!(false));
    assert_eq!(writer.session.path.as_deref(), Some(path.as_path()));
    assert!(path.is_file(), "Save As must create a real .mailcraft file");
    writer.save_on_exit().unwrap();
    drop(writer);

    // Do not read the previous app's memory: load the actual file and start a separate app.
    let on_disk = store::open(&path).expect("saved mailbox can be reopened");
    assert_eq!(on_disk.mailbox.messages.len(), count);
    assert!(on_disk.mailbox.messages.get(&id).unwrap().read);

    let mut reopened = empty_app();
    let opened = reopened
        .execute("file.open", json!({"path": path.to_string_lossy().to_string()}))
        .unwrap();
    assert_eq!(opened["messages"], json!(count));
    assert_eq!(reopened.session.path.as_deref(), Some(path.as_path()));
    assert!(reopened.session.mailbox.messages.get(&id).unwrap().read);
    reopened.save_on_exit().unwrap();
}
