//! `RenameView`, `FindAndReplaceView` and `ConfirmationSheet` — GPUI windows attached as real sheets.

use crate::file_ops;
use crate::i18n::tr;
use crate::main_view::{Engine, MainView};
use crate::platform;
use fastfind_core::text::{nfc, replacing_path_colon};
use fastfind_core::{FileInfo, Index};
use gpui::prelude::*;
use gpui::{
    div, list, point, px, size, AnyWindowHandle, App, Bounds, Context, Entity, FontWeight, ListAlignment, ListState,
    Pixels, Render, Size, WeakEntity, Window, WindowBounds, WindowOptions,
};
use native_gpui::tokens::{self, SystemColor};
use native_gpui::{
    begin_sheet, native_button, native_checkbox, native_root, native_text_field, window_options, CheckState,
};
use std::path::PathBuf;
use std::sync::Arc;

fn main_view(window: &Window) -> Option<WeakEntity<MainView>> {
    window.root::<MainView>().flatten().map(|v| v.downgrade())
}

fn open_sheet<V: Render + 'static>(
    parent: AnyWindowHandle,
    content: Size<Pixels>,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) {
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: content,
            })),
            show: false,
            is_minimizable: false,
            is_resizable: false,
            ..window_options()
        },
        build,
    );
    if let Ok(opened) = opened {
        begin_sheet(parent, opened.into(), cx);
    }
}

/// Ends the sheet and removes its GPUI window, a turn later (AppKit re-enters GPUI as it animates).
fn close_sheet(window: &mut Window, cx: &mut App) {
    let sheet = platform::ns_window(window);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        if let Some(sheet) = sheet {
            if let Some(parent) = sheet.sheetParent() {
                parent.endSheet(&sheet);
            }
        }
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    })
    .detach();
}

fn index(cx: &App) -> Arc<Index> {
    cx.global::<Engine>().index.clone()
}

/// Rename in the background, then `onFileActionComplete` (forced re-search) on the main view.
pub(crate) fn rename_then_refresh(pairs: Vec<(PathBuf, PathBuf)>, main: Option<WeakEntity<MainView>>, cx: &mut App) {
    let index = index(cx);
    cx.spawn(async move |cx| {
        let renamed = cx
            .background_executor()
            .spawn(async move { file_ops::rename_items(&index, &pairs) })
            .await;
        if let Some(main) = main {
            let _ = main.update(cx, |main, cx| {
                main.follow_renames(&renamed);
                main.search_all(None, true, cx);
            });
        }
    })
    .detach();
}

fn sheet_root(child: impl IntoElement, cx: &App) -> impl IntoElement {
    let family = tokens::text_font_family(cx).unwrap_or_else(tokens::system_font_family);
    native_root(
        div()
            .size_full()
            .font_family(family)
            .text_size(px(13.))
            .line_height(px(16.))
            .text_color(tokens::system_color(SystemColor::Label))
            .bg(tokens::system_color(SystemColor::WindowBackground))
            .child(child),
    )
}

// MARK: Rename

pub struct RenameSheet {
    main: Option<WeakEntity<MainView>>,
    original: PathBuf,
    filename: String,
    fresh: bool,
}

pub fn open_rename(window: &mut Window, cx: &mut App, original: PathBuf) {
    let main = main_view(window);
    let filename = original
        .file_name()
        .map(|n| nfc(&n.to_string_lossy()))
        .unwrap_or_default();
    open_sheet(window.window_handle(), size(px(340.), px(132.)), cx, |_, cx| {
        cx.new(|_| RenameSheet {
            main,
            original,
            filename,
            fresh: true,
        })
    });
}

impl RenameSheet {
    fn perform(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        eprintln!("new name: {}", self.filename);
        // Compared normalised, as Swift did: an unchanged name stored decomposed (NFD) is no rename.
        let unchanged = self
            .original
            .file_name()
            .is_some_and(|n| nfc(&n.to_string_lossy()) == nfc(&self.filename));
        if unchanged {
            close_sheet(window, cx);
            return;
        }
        let Some(target) = file_ops::rename_target(&self.original, &self.filename) else {
            // Empty, "." or "..": nothing to rename to; the sheet stays for another try.
            platform::beep();
            return;
        };
        rename_then_refresh(vec![(self.original.clone(), target)], self.main.clone(), cx);
        close_sheet(window, cx);
    }
}

impl Render for RenameSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut field = native_text_field("ff.rename.name", &self.filename)
            .stretch()
            .placeholder("Filename")
            .on_change(cx.listener(|this, text: &String, _, cx| {
                this.filename = text.clone();
                cx.notify();
            }))
            .on_submit(cx.listener(|this, _, window, cx| this.perform(window, cx)))
            .on_cancel(cx.listener(|_, _, window, cx| close_sheet(window, cx)));
        if std::mem::take(&mut self.fresh) {
            // Select the stem, as Finder does.
            let stem = std::path::Path::new(&self.filename)
                .file_stem()
                .map(|s| s.to_string_lossy().chars().count())
                .unwrap_or(0);
            field = field.editing(0..stem);
        }
        sheet_root(
            div()
                .size_full()
                .p(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(div().text_size(px(22.)).line_height(px(28.)).child(tr("Rename")))
                .child(div().w_full().child(field))
                .child(
                    native_button("ff.rename.change", tr("CHANGE"))
                        .on_click(cx.listener(|this, _, window, cx| this.perform(window, cx))),
                ),
            cx,
        )
    }
}

// MARK: Find & Replace

pub struct FindReplaceSheet {
    /// Read for its results at each CHANGE, as SwiftUI handed this sheet the live list: after a
    /// rename the list has the new names.
    main: Option<WeakEntity<MainView>>,
    find: String,
    replace: String,
    fresh: bool,
}

pub fn open_find_replace(window: &mut Window, cx: &mut App) {
    let main = main_view(window);
    open_sheet(window.window_handle(), size(px(420.), px(124.)), cx, |_, cx| {
        cx.new(|_| FindReplaceSheet {
            main,
            find: String::new(),
            replace: String::new(),
            fresh: true,
        })
    });
}

/// `previewChanges()` — case-sensitive literal replacement in each shown item's name.
pub fn preview_changes(results: &[Arc<FileInfo>], find: &str, replace: &str) -> Vec<(PathBuf, PathBuf)> {
    if find.is_empty() {
        return Vec::new();
    }
    // A `:` on disk is shown as `/`: find what the list shows.
    let find = nfc(find).replace('/', ":");
    results
        .iter()
        .filter_map(|item| {
            let name = nfc(&item.os_path.file_name()?.to_string_lossy());
            let renamed = name.replace(&find, replace);
            if renamed == name {
                return None;
            }
            // A replacement that leaves no file name renames nothing.
            Some((item.os_path.clone(), file_ops::rename_target(&item.os_path, &renamed)?))
        })
        .collect()
}

impl FindReplaceSheet {
    fn show_confirmation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        eprintln!("Find text: {}", self.find);
        eprintln!("Replace text: {}", self.replace);
        let results = self
            .main
            .as_ref()
            .and_then(|main| main.upgrade())
            .map(|main| main.read(cx).results())
            .unwrap_or_default();
        let pairs = preview_changes(&results, &self.find, &self.replace);
        open_confirmation(window.window_handle(), self.main.clone(), pairs, cx);
    }
}

impl Render for FindReplaceSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = |text: &'static str| div().flex_none().w(px(54.)).child(text);
        let mut find = native_text_field("ff.find", &self.find)
            .stretch()
            .placeholder(tr("Find"))
            .on_change(cx.listener(|this, text: &String, _, _| this.find = text.clone()))
            .on_cancel(cx.listener(|_, _, window, cx| close_sheet(window, cx)));
        if std::mem::take(&mut self.fresh) {
            find = find.editing(0..0);
        }
        let replace = native_text_field("ff.replace", &self.replace)
            .stretch()
            .placeholder(tr("Replace with"))
            .on_change(cx.listener(|this, text: &String, _, _| this.replace = text.clone()))
            .on_submit(cx.listener(|this, _, window, cx| this.show_confirmation(window, cx)))
            .on_cancel(cx.listener(|_, _, window, cx| close_sheet(window, cx)));
        sheet_root(
            div()
                .size_full()
                .p(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(label("Find:"))
                                        .child(div().flex_1().child(find)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(label("Replace:"))
                                        .child(div().flex_1().child(replace)),
                                ),
                        )
                        .child(
                            native_button("ff.find.change", tr("CHANGE"))
                                .on_click(cx.listener(|this, _, window, cx| this.show_confirmation(window, cx))),
                        ),
                )
                .child("검색 결과에 표시된 파일들에만 반영됩니다")
                .child("case sensitive, cannot undo"),
            cx,
        )
    }
}

// MARK: Confirmation

/// The list of planned renames, every one checked, as a sheet on `parent`.
pub(crate) fn open_confirmation(
    parent: AnyWindowHandle,
    main: Option<WeakEntity<MainView>>,
    pairs: Vec<(PathBuf, PathBuf)>,
    cx: &mut App,
) {
    let checked = vec![true; pairs.len()];
    open_sheet(parent, size(px(432.), px(700.)), cx, |_, cx| {
        let rows = ListState::new(pairs.len(), ListAlignment::Top, px(200.));
        cx.new(|_| ConfirmSheet {
            main,
            pairs,
            checked,
            rows,
        })
    });
}

pub struct ConfirmSheet {
    main: Option<WeakEntity<MainView>>,
    pairs: Vec<(PathBuf, PathBuf)>,
    checked: Vec<bool>,
    rows: ListState,
}

impl ConfirmSheet {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected: Vec<(PathBuf, PathBuf)> = self
            .pairs
            .iter()
            .zip(&self.checked)
            .filter(|(_, on)| **on)
            .map(|(pair, _)| pair.clone())
            .collect();
        rename_then_refresh(selected, self.main.clone(), cx);
        close_sheet(window, cx);
    }
}

impl ConfirmSheet {
    /// One planned rename: its checkbox, old → new name, and the folder it is in.
    fn render_pair(&mut self, i: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = tokens::system_color;
        let (red, green) = (c(SystemColor::SystemRed), c(SystemColor::SystemGreen));
        // As Finder and the results list show names: a `:` on disk reads as `/`.
        let name = |p: &PathBuf| {
            p.file_name()
                .map(|n| replacing_path_colon(&n.to_string_lossy()))
                .unwrap_or_default()
        };
        let Some((from, to)) = self.pairs.get(i) else {
            return div().into_any_element();
        };
        let folder = from
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .p(px(6.))
            .rounded(px(4.))
            .when(i % 2 == 1, |row| row.bg(gpui::black().opacity(0.3)))
            .child(
                div()
                    .flex_none()
                    .child(
                        native_checkbox(("ff.confirm.check", i), self.checked[i]).on_change(cx.listener(
                            move |this, state: &CheckState, _, cx| {
                                this.checked[i] = *state == CheckState::On;
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_h(px(24.))
                            .p(px(2.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().text_color(red).child(name(from)))
                            .child(div().flex_none().w(px(20.)).child("→"))
                            .child(div().flex_1().min_w_0().text_color(green).child(name(to))),
                    )
                    .child(
                        div().w_full().min_w_0().flex().child(
                            div()
                                .min_w_0()
                                .p(px(3.))
                                .rounded(px(5.))
                                .bg(gpui::black().opacity(0.7))
                                .text_color(gpui::white())
                                .text_size(px(10.))
                                .line_height(px(13.))
                                .child(folder),
                        ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ConfirmSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = tokens::system_color;
        let (red, green) = (c(SystemColor::SystemRed), c(SystemColor::SystemGreen));
        // Only the rows in view are built (each holds a native checkbox), however many renames.
        let changes = if self.pairs.is_empty() {
            div()
                .text_size(px(22.))
                .line_height(px(28.))
                .child("Nothing to change! 🎉")
                .into_any_element()
        } else {
            list(
                self.rows.clone(),
                cx.processor(|this, i: usize, _, cx| this.render_pair(i, cx)),
            )
            .size_full()
            .into_any_element()
        };
        sheet_root(
            div()
                .size_full()
                .p(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(div().font_weight(FontWeight::BOLD).child(tr("Preview Changes")))
                .child(div().w(px(400.)).h(px(600.)).p(px(3.)).child(changes))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.))
                        .child(
                            div().rounded(px(8.)).bg(red).child(
                                native_button("ff.confirm.cancel", tr("Cancel"))
                                    .on_click(cx.listener(|_, _, window, cx| close_sheet(window, cx))),
                            ),
                        )
                        .child(
                            div().rounded(px(8.)).bg(green).child(
                                native_button("ff.confirm.ok", tr("Confirm"))
                                    .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                            ),
                        ),
                ),
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_skips_unchanged_and_empty_find() {
        let items: Vec<Arc<FileInfo>> = ["/a/foo bar.txt", "/a/baz.txt"]
            .iter()
            .map(|p| Arc::new(FileInfo::new(PathBuf::from(p), 1, false)))
            .collect();
        assert!(preview_changes(&items, "", "x").is_empty());
        let changes = preview_changes(&items, "bar", "qux");
        assert_eq!(
            changes,
            vec![(PathBuf::from("/a/foo bar.txt"), PathBuf::from("/a/foo qux.txt"))]
        );
        assert!(preview_changes(&items, "BAR", "x").is_empty(), "case sensitive");
        // A `/` is stored as Finder stores it; a replacement that leaves no name is dropped.
        let changes = preview_changes(&items, "foo bar", "AC/DC");
        assert_eq!(
            changes,
            vec![(PathBuf::from("/a/foo bar.txt"), PathBuf::from("/a/AC:DC.txt"))]
        );
        // …and is found the way the list shows it.
        let slashed = [Arc::new(FileInfo::new(PathBuf::from("/a/AC:DC.mp4"), 1, false))];
        let changes = preview_changes(&slashed, "AC/DC", "ACDC");
        assert_eq!(
            changes,
            vec![(PathBuf::from("/a/AC:DC.mp4"), PathBuf::from("/a/ACDC.mp4"))]
        );
        assert!(preview_changes(&items, "baz.txt", "").is_empty());
    }
}
