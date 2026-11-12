//! The results table, and the indexing states shown in its place.

use super::*;

impl MainView {
    pub(super) fn render_table(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let list_state = self.list_state.clone();
        let count = self.results.len();
        let reserve = self.row_estimate.borrow().reserve();
        let stripes = canvas(
            |_, _, _| (),
            move |bounds: Bounds<Pixels>, _, window, _| {
                // NSTableView keeps alternating stripes going below the last row.
                let top = if count == 0 {
                    bounds.top() + px(ROW_INSET)
                } else {
                    match item_bounds(&list_state, count - 1) {
                        Some(last) => last.bottom(),
                        None => return,
                    }
                };
                let mut y = top;
                let mut row = count;
                while y < bounds.bottom() {
                    if row.is_multiple_of(2) {
                        let stripe = Bounds::new(
                            point(bounds.left() + px(ROW_INSET), y),
                            size(bounds.size.width - reserve - px(2. * ROW_INSET), px(STRIPE_HEIGHT)),
                        );
                        window.paint_quad(fill(stripe, p.stripe).corner_radii(px(ROW_RADIUS)));
                    }
                    y += px(STRIPE_HEIGHT);
                    row += 1;
                }
            },
        )
        .absolute()
        .inset_0();

        let weak = cx.entity().downgrade();
        let weak_select = weak.clone();
        let weak_close = weak.clone();
        let rows = list(
            self.list_state.clone(),
            cx.processor(|this, ix: usize, window, cx| this.render_row(ix, window, cx)),
        )
        .size_full()
        .py(px(ROW_INSET));
        let layout_key = u64::from(self.hide_tags) | u64::from(self.original_filename) << 1;
        let scroll_bars = div().absolute().inset_0().child(list_scroll_bars(
            "ff.results.scroll_bars",
            &self.list_state,
            px(ROW_INSET),
            layout_key,
            &self.row_estimate,
        ));
        let menu = native_context_menu("ff.results.menu", rows)
            .entries_at(move |_, _, cx| weak.update(cx, |this, _| this.context_entries()).unwrap_or_default())
            .on_select(move |index: &usize, window, cx| {
                let _ = weak_select.update(cx, |this, cx| this.perform_menu(*index, window, cx));
            })
            .on_close(move |_, cx| {
                let _ = weak_close.update(cx, |this, cx| {
                    this.context_row = None;
                    cx.notify();
                });
            });

        div()
            .id("ff.table")
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus)
            .key_context(LIST_CONTEXT)
            .on_action(cx.listener(|this, _: &SelectPrevious, window, cx| this.move_selection(-1, false, window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, window, cx| this.move_selection(1, false, window, cx)))
            .on_action(cx.listener(|this, _: &ExtendPrevious, window, cx| this.move_selection(-1, true, window, cx)))
            .on_action(cx.listener(|this, _: &ExtendNext, window, cx| this.move_selection(1, true, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleQuickLook, window, cx| this.toggle_quick_look(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSelection, _, _| this.open_selection()))
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, _| {
                if event.button == MouseButton::Right {
                    this.context_row = None;
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    // A click on empty space clears the selection, as in a table.
                    window.focus(&this.focus, cx);
                    this.selection.clear();
                    this.anchor = None;
                    this.lead = None;
                    cx.notify();
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &MouseMoveEvent, window, cx| this.list_mouse_move(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                if !this.pointer_in_list {
                    this.pointer_in_list = true;
                    cx.notify();
                }
            }))
            .on_mouse_exit(cx.listener(|this, _: &MouseExitEvent, _, cx| {
                this.pointer_in_list = false;
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    // Released off the rows: no drag, and no deferred click either.
                    this.drag_start = None;
                    this.pending_click = None;
                }),
            )
            .child(stripes)
            .child(menu)
            .child(scroll_bars)
            .when(self.copying > 0, |table| {
                table.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::black().opacity(0.4))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(native_spinner("ff.copying.spinner"))
                        .child(
                            div()
                                .p(px(16.))
                                .text_size(px(22.))
                                .line_height(px(28.))
                                .text_color(gpui::white())
                                .child(tr("Copying...")),
                        ),
                )
            })
            .bg(p.window_bg)
    }

    pub(super) fn render_indexing(&self) -> impl IntoElement {
        let p = palette();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(20.))
            .child(native_spinner("ff.indexing.spinner"))
            .child(
                div()
                    .text_size(px(15.))
                    .line_height(px(19.))
                    .child("Indexing is in progress..."),
            )
            .child(
                div()
                    .text_size(px(17.))
                    .line_height(px(22.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(p.blue)
                    .child(format!("{} items indexed", self.indexed_count)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(p.secondary)
                    .child("Please wait while indexing continues"),
            )
    }

    pub(super) fn render_index_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(20.))
            .child(
                native_symbol("ff.magnifier", "magnifyingglass", 60., FontWeight::NORMAL)
                    .tint(SystemColor::SecondaryLabel),
            )
            .child(
                native_button("ff.start-indexing", "Start Indexing")
                    .symbol("arrow.clockwise")
                    .default_button(true)
                    .control_size(ControlSize::ExtraLarge)
                    .font_size(px(15.))
                    .on_click(cx.listener(|this, _, _, cx| this.cache_data(cx))),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(p.secondary)
                    .child("Click to start indexing files in the selected directory"),
            )
    }
}

/// `ListState::bounds_for_item` leaves out the list's own top padding.
pub(super) fn item_bounds(list_state: &ListState, ix: usize) -> Option<Bounds<Pixels>> {
    list_state.bounds_for_item(ix).map(|mut b| {
        b.origin.y += px(ROW_INSET);
        b
    })
}
