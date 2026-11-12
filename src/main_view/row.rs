//! One result row (`FileListItemView`).

use super::*;

impl MainView {
    pub(super) fn row_model(&mut self, item: &Arc<FileInfo>) -> Rc<RowModel> {
        if let Some((built_from, model)) = self.row_cache.get(&item.path) {
            if Arc::ptr_eq(built_from, item) {
                return model.clone();
            }
        }
        let path = item.os_path.to_string_lossy().into_owned();
        let is_link = item.is_symlink;
        let extension = item.os_path.extension().map(|e| e.to_string_lossy().to_lowercase());
        let icon = if is_link || tokens::has_custom_icon(&item.os_path) {
            Icon::Path(path)
        } else if item.is_dir {
            // Packages (.app, .bundle…) draw their own icon.
            if extension.is_some() {
                Icon::Path(path)
            } else {
                Icon::Folder
            }
        } else {
            match extension {
                Some(ext) if !ext.is_empty() => Icon::Extension(ext),
                _ => Icon::Path(path),
            }
        };
        let model = Rc::new(RowModel {
            data: parse_row(item),
            size_text: if item.size == 0 {
                String::new()
            } else {
                format_file_size(item.size, crate::i18n::is_korean())
            },
            icon,
        });
        self.row_cache.insert(item.path.clone(), (item.clone(), model.clone()));
        model
    }

    pub(super) fn render_row(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.results.get(ix).cloned() else {
            return div().into_any_element();
        };
        let model = self.row_model(&item);
        let p = palette();
        let selected = self.selection.contains(&item.path);
        let emphasized = selected && self.list_emphasis;
        let text_color = if emphasized { p.selected_text } else { p.label };
        let words = &self.shown.strings;

        let icon = match &model.icon {
            Icon::Folder => tokens::file_icon(FileType::Folder, px(16.)),
            Icon::Path(path) => tokens::file_icon(FileType::Path(path), px(16.)),
            Icon::Extension(ext) => tokens::file_icon(FileType::Extension(ext), px(16.)),
        };
        let icon: AnyElement = match icon.or_else(|| tokens::sf_symbol("doc", px(13.), FontWeight::NORMAL)) {
            Some(symbol) => div()
                .flex_none()
                .w(px(16.))
                .flex()
                .justify_center()
                .child(symbol.img())
                .into_any_element(),
            None => div().flex_none().size(px(16.)).into_any_element(),
        };

        let name: AnyElement = if self.original_filename {
            let text = self.edits.get(&item.path).cloned().unwrap_or_else(|| item.disk_name());
            let (for_change, for_submit, for_cancel) = (item.path.clone(), item.clone(), item.path.clone());
            div()
                .flex_1()
                .min_w_0()
                .child(
                    native_text_field(("ff.rename", stable_id(&item.path)), &text)
                        .stretch()
                        .placeholder("Filename")
                        .on_change(cx.listener(move |this, text: &String, _, _| {
                            this.edits.insert(for_change.clone(), text.clone());
                        }))
                        .on_submit(cx.listener(move |this, text: &String, _, cx| {
                            this.inline_rename(for_submit.clone(), text.clone(), cx);
                        }))
                        .on_cancel(cx.listener(move |this, _: &String, _, cx| {
                            this.edits.remove(&for_cancel);
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            div()
                .min_w_0()
                .flex_shrink(1.)
                .child(highlighted(model.data.parsed_output.clone(), words, p.yellow))
                .into_any_element()
        };

        let size_text = |p: &Palette| {
            div()
                .flex_none()
                .text_size(px(10.))
                .line_height(px(13.))
                .text_color(if emphasized { p.selected_text } else { p.label })
                .child(model.size_text.clone())
        };

        let name_line = div()
            .flex()
            .flex_row()
            .items_start()
            .pb(px(4.))
            .child(name)
            .when(!self.original_filename && !model.data.parsed_type.is_empty(), |line| {
                line.child(div().flex_1().min_w(px(4.))).child(
                    div()
                        .flex_none()
                        .max_w(relative(0.45))
                        .px(px(4.))
                        .py(px(1.))
                        .rounded(px(6.))
                        .bg(p.mint)
                        .text_color(gpui::black())
                        .text_size(px(10.))
                        .line_height(px(13.))
                        .italic()
                        .line_clamp(2)
                        .child(model.data.parsed_type.clone()),
                )
            })
            .when(self.hide_tags, |line| line.child(div().flex_1()).child(size_text(&p)));

        let tags_line = (!self.hide_tags).then(|| {
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap(px(1.))
                .mt(px(-3.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(5.))
                        .min_w_0()
                        .flex_shrink(1.)
                        .children(model.data.tags.iter().map(|tag| {
                            div()
                                .min_w_0()
                                .flex_shrink(1.)
                                .px(px(4.))
                                .py(px(1.))
                                .rounded(px(6.))
                                .bg(if tag.is_grandparent { p.red } else { p.indigo })
                                .text_color(gpui::white())
                                .text_size(px(10.))
                                .line_height(px(13.))
                                .line_clamp(2)
                                .text_ellipsis()
                                .child(highlighted(tag.text.clone(), words, p.yellow))
                        })),
                )
                .child(div().flex_1().min_w_0())
                .child(size_text(&p))
        });

        let decks = (!item.is_dir && !self.deck_numbers.is_empty()).then(|| self.deck_numbers.clone());
        let deck_buttons = decks.map(|decks| {
            div()
                .flex_none()
                .flex()
                .flex_row()
                .gap(px(3.))
                .children(decks.into_iter().map(|deck| {
                    let target = item.clone();
                    div()
                        .id(("ff.deck", ix * 8 + deck as usize))
                        .size(px(20.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(p.gray.opacity(0.15))
                        .text_color(p.secondary)
                        .hover(|s| s.bg(p.gray.opacity(0.3)).text_color(p.label))
                        .active(|s| s.bg(p.accent).text_color(gpui::white()))
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .font_family("Menlo")
                        .child(deck.to_string())
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_: &ClickEvent, _, _| {
                            file_ops::vjdeck_import(std::slice::from_ref(&target), deck)
                        })
                }))
        });

        let row_bg = if selected {
            Some(if emphasized { p.selection } else { p.selection_inactive })
        } else if ix.is_multiple_of(2) {
            Some(p.stripe)
        } else {
            None
        };
        let menu_ring = self.context_row.as_ref() == Some(&item.path);
        let drop_target = item.clone();

        let hover_tint = p.yellow.opacity(0.1);
        div()
            .w_full()
            // Clear of an always-shown scroller, so pointing at it lights no row.
            .pr(self.row_estimate.borrow().reserve())
            .child(
                // The hover area is the whole row line: no dead band between rows or at the sides,
                // as SwiftUI's cell-content hover had.
                div().group(ROW_GROUP).w_full().px(px(ROW_INSET)).child(
                    div()
                        .w_full()
                        .rounded(px(ROW_RADIUS))
                        .when_some(row_bg, |d, bg| d.bg(bg))
                        .relative()
                        .when(self.pointer_in_list, |d| {
                            // Tints the whole row shape, under its content. The id gives it hover
                            // state, without which GPUI repaints on no hover change.
                            d.child(
                                div()
                                    .id(("ff.row.hover", ix))
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(ROW_RADIUS))
                                    .group_hover(ROW_GROUP, move |s| s.bg(hover_tint)),
                            )
                        })
                        .when(menu_ring, |d| {
                            // The right-clicked row's focus ring, drawn over it without moving its content.
                            d.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(ROW_RADIUS))
                                    .border_2()
                                    .border_color(p.accent),
                            )
                        })
                        .px(px(6.))
                        .py(px(4.))
                        .text_color(text_color)
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(icon)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(name_line)
                                        .children(tags_line),
                                )
                                .children(deck_buttons),
                        ),
                ),
            )
            // By the item, not the row number: these handlers stay in place until the next frame,
            // and new results can land before it, putting another file at this row.
            .on_mouse_down(MouseButton::Left, {
                let path = item.path.clone();
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.row_mouse_down(&path, event, window, cx)
                })
            })
            .on_mouse_up(MouseButton::Left, {
                let path = item.path.clone();
                cx.listener(move |this, _: &MouseUpEvent, window, cx| this.row_mouse_up(&path, window, cx))
            })
            .on_mouse_down(MouseButton::Right, {
                let path = item.path.clone();
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.context_row = Some(path.clone());
                    cx.notify();
                })
            })
            .when(item.is_dir, |row| {
                row.on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                    this.drop_into(drop_target.clone(), paths.paths().to_vec(), cx);
                }))
            })
            .into_any_element()
    }
}

/// A stable element id for a path (pooled native views are keyed by it).
pub(super) fn stable_id(path: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish() as usize
}

/// `createHighlightedText` — search words in yellow.
pub(super) fn highlighted(text: String, words: &[String], color: Hsla) -> StyledText {
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for range in highlight_ranges(&text, words) {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    let style = HighlightStyle {
        color: Some(color),
        ..Default::default()
    };
    StyledText::new(text).with_highlights(merged.into_iter().map(move |r| (r, style)))
}
