// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The list under the search box that offers what the word being typed may be (#308): a keyword
//! from its start (`cat` → `category:`), the mailbox's categories after `category:`, what `is:`
//! and `has:` take, and people after `from:`, `to:` and `cc:`, each by name with the address
//! beside it, from the contacts and the mail on this computer
//! ([`mailcraft_engine::cmd::view::search_suggestions`] says what). The arrow keys
//! choose; Enter takes the one chosen, Tab that one or the first, a click the one under it. Until
//! an arrow key is pressed nothing is chosen, so Enter does what it did. The list is egui's popup:
//! Escape closes it and leaves the search (and a message being written open); the next Escape
//! clears it, as before. It stays closed until the search or the cursor changes, or the box is
//! left. With Shift, Ctrl or Alt held, the keys are not the list's; Shift+Tab leaves the box, as
//! without it.

use egui::{Context, Rect, Sense, Stroke, Ui, pos2, vec2};
use mailcraft_model::search::Suggestion;

use crate::theme::{Tokens, c32, paint_clipped, regular};

/// How tall a row of the list is, and the side of a category's mark.
const ROW_H: f32 = 26.0;
const MARK: f32 = 10.0;

/// The list, from one frame to the next.
#[derive(Clone, Default)]
pub(crate) struct Suggesting {
    pub items: Vec<Suggestion>,
    /// The one the arrow keys chose.
    pub selected: Option<usize>,
    /// Whether the list was to show in the frame before, and whether the search or the cursor
    /// changed since: it is opened only then (when it comes to be wanted, and for a search or
    /// a cursor that changed), not on every frame, so that what egui closed (Escape) stays closed.
    wanted: bool,
    moved: bool,
    /// What `items` are for: the search, the cursor in it, the mailbox's categories (a renamed
    /// one is offered by its new name), and the mailbox as it was ([`Session::rev`] and
    /// [`Session::openings`]: the people that mail arriving, or another mailbox, brings are
    /// offered without a key being pressed). A take is only of items for the search as it is now.
    ///
    /// [`Session::rev`]: mailcraft_engine::Session::rev
    /// [`Session::openings`]: mailcraft_engine::Session::openings
    pub for_text: String,
    for_cursor: usize,
    for_categories: Vec<mailcraft_model::Category>,
    for_mailbox: (u64, u64),
}

/// Regions for a right-to-left suggestion row. The label reads from the right;
/// a category mark is beyond its right edge, and an LTR address keeps the left
/// side. The row's click target and the order of suggestions do not change.
fn hebrew_suggestion_regions(r: Rect, colored: bool, name_width: f32, addressed: bool) -> (Option<Rect>, Rect, Option<Rect>) {
    let left = r.min.x + 8.0;
    let mut right = r.max.x - 8.0;
    let mark = if colored {
        let rect = Rect::from_center_size(pos2(right - MARK / 2.0, r.center().y), vec2(MARK, MARK));
        right -= MARK + 8.0;
        Some(rect)
    } else {
        None
    };
    let room = (right - left).max(0.0);
    let name_room = if addressed { room * 0.6 } else { room };
    let name_used = name_width.max(0.0).min(name_room);
    let name = Rect::from_min_max(pos2(right - name_used, r.min.y), pos2(right, r.max.y));
    let address = addressed.then(|| {
        Rect::from_min_max(pos2(left, r.min.y), pos2((name.min.x - 8.0).max(left), r.max.y))
    });
    (mark, name, address)
}

#[cfg(test)]
mod hebrew_suggestion_layout_tests {
    use super::*;

    #[test]
    fn hebrew_rows_anchor_names_right_and_addresses_left_without_overlap() {
        for width in [140.0, 240.0, 420.0] {
            let row = Rect::from_min_max(pos2(10.0, 20.0), pos2(10.0 + width, 46.0));
            for colored in [false, true] {
                let (mark, name, address) = hebrew_suggestion_regions(row, colored, 75.0, true);
                let address = address.expect("person's address stays visible on the left");
                let room = width - 16.0 - if colored { MARK + 8.0 } else { 0.0 };
                assert!((name.width() - 75.0_f32.min(room * 0.6)).abs() < 0.001);
                assert!(address.min.x >= row.min.x + 8.0);
                assert!(address.max.x + 8.0 <= name.min.x);
                assert!(name.max.x <= row.max.x - 8.0);
                if colored {
                    let mark = mark.expect("category mark is to the right");
                    assert!(name.max.x + 8.0 <= mark.min.x);
                    assert_eq!(mark.center().x, row.max.x - 8.0 - MARK / 2.0);
                } else {
                    assert!(mark.is_none());
                    assert_eq!(name.max.x, row.max.x - 8.0);
                }
                let (_, long_name, _) = hebrew_suggestion_regions(row, colored, 2000.0, true);
                assert!(long_name.width() <= (width - 16.0) * 0.6);
            }
            let (_, single, none) = hebrew_suggestion_regions(row, false, 30.0, false);
            assert_eq!(single.max.x, row.max.x - 8.0);
            assert!(none.is_none());
        }
    }
}

impl Suggesting {
    fn store_id() -> egui::Id {
        egui::Id::unique("search_suggesting")
    }

    /// The list's popup, and its area.
    fn list_id() -> egui::Id {
        Self::store_id().with("list")
    }

    pub fn load(ctx: &Context) -> Suggesting {
        ctx.data(|d| d.get_temp(Self::store_id())).unwrap_or_default()
    }

    pub fn store(self, ctx: &Context) {
        ctx.data_mut(|d| d.insert_temp(Self::store_id(), self));
    }

    /// Whether the list shows, as egui last drew it.
    pub fn is_open(ctx: &Context) -> bool {
        egui::Popup::is_id_open(ctx, Self::list_id())
    }

    /// Whether the list will show in the next frame, while the search may be offered for (`free`).
    pub fn will_show(&self, ctx: &Context, free: bool) -> bool {
        free && !self.items.is_empty() && (Self::is_open(ctx) || self.moved || !self.wanted)
    }

    /// Whether the pointer is over the list.
    pub fn pointer_over(&self, ui: &Ui) -> bool {
        let list = egui::LayerId::new(egui::Order::Foreground, Self::list_id());
        ui.ctx().pointer_hover_pos().and_then(|p| ui.ctx().layer_id_at(p)) == Some(list)
    }

    /// The keys meant for the list while it shows, taken before the box sees them; which item to
    /// take, if one. (Escape is egui's: it closes the list.)
    pub fn keys(&mut self, ui: &Ui) -> Option<usize> {
        let n = self.items.len();
        let mut take = None;
        let mut back = false;
        ui.input_mut(|i| {
            // The box keeps every Tab while the list shows, so Shift+Tab is done here.
            if i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                back = true;
                return;
            }
            if !i.modifiers.is_none() {
                return;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                self.selected = Some(self.selected.map_or(0, |s| s.saturating_add(1) % n.max(1)));
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                self.selected = Some(self.selected.map_or(n.saturating_sub(1), |s| s.checked_sub(1).unwrap_or(n.saturating_sub(1))));
            }
            if self.selected.is_some() && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                take = self.selected;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                take = Some(self.selected.unwrap_or(0));
            }
        });
        if back {
            ui.memory_mut(|m| m.move_focus(egui::FocusDirection::Previous));
        }
        take
    }

    /// The list under `under` (the search box), when it is `wanted`: opened when it comes to be,
    /// or for a search or a cursor that changed; closed when it isn't. The item clicked, if one.
    /// A click outside it is no matter of the list's: one away from the box takes the keyboard
    /// from it, and that closes the list.
    pub fn show(&mut self, ui: &Ui, under: Rect, wanted: bool) -> Option<usize> {
        let t = Tokens::get(ui.ctx());
        let open = if !wanted {
            Some(egui::SetOpenCommand::Bool(false))
        } else if !self.wanted || self.moved {
            Some(egui::SetOpenCommand::Bool(true))
        } else {
            None
        };
        (self.wanted, self.moved) = (wanted, false);
        let frame = egui::Frame::NONE.fill(t.menu).stroke(Stroke::new(1.0, t.border_strong)).corner_radius(6.0).inner_margin(egui::Margin::same(4));
        let mut take = None;
        egui::Popup::new(Self::list_id(), ui.ctx().clone(), under, ui.layer_id())
            .gap(2.0)
            .frame(frame)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .open_memory(open)
            .show(|ui| {
                ui.set_width((under.width() - 8.0).max(160.0));
                for (i, s) in self.items.iter().enumerate() {
                    let (r, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                    let named = shows_address(s).map_or_else(|| s.label.clone(), |address| format!("{}, {address}", s.label));
                    row.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &named));
                    if self.selected == Some(i) || row.hovered() {
                        ui.painter().rect_filled(r, 4.0, if self.selected == Some(i) { t.selection } else { t.hover });
                    }
                    let address = shows_address(s);
                    if crate::i18n::current() == crate::i18n::Lang::Hebrew {
                        // The suggestions follow the Hebrew search box. Preserve Latin email
                        // addresses left-to-right on the left, and the original full-row hit area.
                        let font = regular(12.5);
                        let name_width = crate::theme::text_width(ui.painter(), &s.label, &font, 0.0, t.text);
                        let (mark, name, address_rect) = hebrew_suggestion_regions(r, s.color.is_some(), name_width, address.is_some());
                        if let (Some(mark), Some(color)) = (mark, s.color) {
                            ui.painter().rect_filled(mark, 2.0, c32(color));
                        }
                        let _ = paint_clipped(ui.painter(), pos2(name.min.x, name.center().y), name.width(), &s.label, font, t.text);
                        if let (Some(address), Some(rect)) = (address, address_rect) {
                            paint_clipped(ui.painter(), pos2(rect.min.x, rect.center().y), rect.width(), address, regular(11.5), t.text_dim);
                        }
                    } else {
                        let mut x = r.min.x + 8.0;
                        if let Some(color) = s.color {
                            ui.painter().rect_filled(Rect::from_center_size(pos2(x + MARK / 2.0, r.center().y), vec2(MARK, MARK)), 2.0, c32(color));
                            x += MARK + 8.0;
                        }
                        // Somebody: the name, and the address beside it, as the recipient suggestions show them.
                        let room = (r.max.x - x - 8.0).max(0.0);
                        let w = paint_clipped(
                            ui.painter(),
                            pos2(x, r.center().y),
                            if address.is_some() { room * 0.6 } else { room },
                            &s.label,
                            regular(12.5),
                            t.text,
                        );
                        if let Some(address) = address {
                            paint_clipped(ui.painter(), pos2(x + w + 8.0, r.center().y), (room - w - 8.0).max(0.0), address, regular(11.5), t.text_dim);
                        }
                    }
                    if row.clicked() {
                        take = Some(i);
                    }
                }
            });
        take
    }

    /// What to offer next frame, for `text` with the cursor at character `cursor`.
    pub fn offer(&mut self, text: &str, cursor: usize, session: &mailcraft_engine::Session) {
        self.moved |= (self.for_text.as_str(), self.for_cursor) != (text, cursor);
        let mailbox = (session.rev(), session.openings());
        if (self.for_text.as_str(), self.for_cursor, self.for_mailbox) == (text, cursor, mailbox)
            && self.for_categories.iter().eq(session.mailbox.all_categories())
        {
            return;
        }
        let byte = text.char_indices().nth(cursor).map_or(text.len(), |(b, _)| b);
        let items = mailcraft_engine::cmd::view::search_suggestions(session, text, byte);
        if items != self.items {
            self.selected = None;
        }
        self.items = items;
        (self.for_text, self.for_cursor, self.for_mailbox) = (text.to_string(), cursor, mailbox);
        self.for_categories = session.mailbox.all_categories().cloned().collect();
    }

    /// Closes the list and forgets what it offered: Advanced Search opened over it.
    pub fn close(ctx: &Context) {
        egui::Popup::close_id(ctx, Self::list_id());
        Suggesting::default().store(ctx);
    }

    pub fn clear(&mut self) {
        *self = Suggesting { wanted: self.wanted, ..Suggesting::default() };
    }
}

/// The address shown beside a person's name: not when the name is the address.
fn shows_address(s: &Suggestion) -> Option<&str> {
    s.address.as_deref().filter(|address| !address.eq_ignore_ascii_case(&s.label))
}

/// `text` with `s` taken, and where the cursor goes after it (in characters).
pub(crate) fn taken(text: &str, s: &Suggestion) -> (String, usize) {
    let (Some(before), Some(after)) = (text.get(..s.range.start), text.get(s.range.end..)) else { return (text.to_string(), text.chars().count()) };
    let cursor = before.chars().count().saturating_add(s.text.chars().count());
    (format!("{before}{}{after}", s.text), cursor)
}

#[cfg(test)]
#[path = "search_suggest_tests.rs"]
mod search_suggest_tests;
