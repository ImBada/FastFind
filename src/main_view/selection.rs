//! Selection, the keyboard, mouse presses and drags on rows, and Quick Look.

use super::*;

impl MainView {
    pub(super) fn list_focused(&self, window: &Window) -> bool {
        window.is_window_active() && self.focus.is_focused(window) && platform::gpui_has_keyboard(window)
    }

    pub(super) fn item_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        item_bounds(&self.list_state, ix)
    }

    pub(super) fn selected_items(&self) -> Vec<Arc<FileInfo>> {
        self.results
            .iter()
            .filter(|r| self.selection.contains(&r.path))
            .cloned()
            .collect()
    }

    /// The item Quick Look shows: the row last clicked or moved to, else the first selected.
    pub(super) fn preview_item(&self) -> Option<(usize, Arc<FileInfo>)> {
        let lead = self
            .lead
            .as_ref()
            .filter(|l| self.selection.contains(*l))
            .and_then(|l| self.positions.get(l).copied());
        let ix = lead.or_else(|| self.results.iter().position(|r| self.selection.contains(&r.path)))?;
        Some((ix, self.results[ix].clone()))
    }

    pub(super) fn select_only(&mut self, ix: usize) {
        let Some(item) = self.results.get(ix) else { return };
        self.selection.clear();
        self.selection.insert(item.path.clone());
        self.anchor = Some(item.path.clone());
        self.lead = Some(item.path.clone());
    }

    pub(super) fn select_range(&mut self, from: usize, to: usize, keep: bool) {
        if !keep {
            self.selection.clear();
        }
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        for item in &self.results[lo..=hi.min(self.results.len().saturating_sub(1))] {
            self.selection.insert(item.path.clone());
        }
    }

    pub(super) fn selection_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() && quick_look_visible() {
            if let Some((ix, item)) = self.preview_item() {
                quick_look_update(window, vec![item.os_path.clone()], 0, self.item_bounds(ix));
            }
        }
        cx.notify();
    }

    pub(super) fn move_selection(&mut self, delta: isize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.results.len();
        if count == 0 {
            return;
        }
        let current = self.lead.as_ref().and_then(|l| self.positions.get(l).copied());
        let next = match current {
            Some(ix) => (ix as isize + delta).clamp(0, count as isize - 1) as usize,
            None if delta > 0 => 0,
            None => count - 1,
        };
        if extend {
            let anchor = self
                .anchor
                .as_ref()
                .and_then(|a| self.positions.get(a).copied())
                .unwrap_or(next);
            self.select_range(anchor, next, false);
            if self.anchor.is_none() {
                self.anchor = Some(self.results[next].path.clone());
            }
            self.lead = Some(self.results[next].path.clone());
        } else {
            self.select_only(next);
        }
        self.list_state.scroll_to_reveal_item(next);
        self.selection_changed(window, cx);
    }

    pub(super) fn row_mouse_down(
        &mut self,
        path: &Arc<str>,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.focus, cx);
        // A deferred click whose release never reached a row is forgotten.
        self.pending_click = None;
        // The item pressed, wherever it is now; gone from the results, the press does nothing.
        let Some(&ix) = self.positions.get(path) else { return };
        let Some(item) = self.results.get(ix).cloned() else {
            return;
        };
        let modifiers = event.modifiers;
        if event.click_count >= 2 && !modifiers.platform && !modifiers.shift {
            // primaryAction: double-click opens the selection.
            self.open_selection();
            return;
        }
        if modifiers.platform {
            if !self.selection.remove(&item.path) {
                self.selection.insert(item.path.clone());
            }
            self.anchor = Some(item.path.clone());
            self.lead = Some(item.path.clone());
        } else if modifiers.shift {
            let anchor = self
                .anchor
                .as_ref()
                .and_then(|a| self.positions.get(a).copied())
                .unwrap_or(ix);
            self.select_range(anchor, ix, false);
            // With no anchor yet, this row becomes it, so the next shift-click extends from here.
            if self.anchor.is_none() {
                self.anchor = Some(item.path.clone());
            }
            self.lead = Some(item.path.clone());
        } else if self.selection.contains(&item.path) && self.selection.len() > 1 {
            // Defer collapsing a multi-selection until mouse-up, so it can still be dragged.
            self.pending_click = Some(item.path.clone());
            self.lead = Some(item.path.clone());
        } else {
            self.select_only(ix);
        }
        self.drag_start = Some((event.position, item));
        self.selection_changed(window, cx);
    }

    pub(super) fn row_mouse_up(&mut self, path: &Arc<str>, window: &mut Window, cx: &mut Context<Self>) {
        self.drag_start = None;
        if self.pending_click.take().as_ref() != Some(path) {
            return;
        }
        if let Some(&ix) = self.positions.get(path) {
            self.select_only(ix);
            self.selection_changed(window, cx);
        }
    }

    pub(super) fn list_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, _: &mut Context<Self>) {
        let Some((start, item)) = self.drag_start.clone() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag_start = None;
            return;
        }
        let (dx, dy) = (
            f32::from(event.position.x - start.x),
            f32::from(event.position.y - start.y),
        );
        if dx * dx + dy * dy < 16. {
            return;
        }
        self.drag_start = None;
        self.pending_click = None;
        // The item pressed, even if the results changed under the pointer meanwhile.
        let paths: Vec<PathBuf> = if self.selection.contains(&item.path) {
            self.selected_items().iter().map(|i| i.os_path.clone()).collect()
        } else {
            vec![item.os_path.clone()]
        };
        native_gpui::sidebar::drag_files(window, &paths);
    }

    pub(super) fn open_selection(&mut self) {
        let items = self.selected_items();
        if !items.is_empty() {
            platform::open_paths(&items.iter().map(|i| i.os_path.clone()).collect::<Vec<_>>());
        }
    }

    pub(super) fn toggle_quick_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.original_filename {
            cx.propagate();
            return;
        }
        if quick_look_visible() {
            quick_look_close();
        } else if let Some((ix, item)) = self.preview_item() {
            let keys = self.quick_look_keys.clone();
            quick_look(
                window,
                vec![item.os_path.clone()],
                0,
                self.item_bounds(ix),
                move |key| {
                    let _ = keys.unbounded_send(key);
                },
            );
        }
    }

    pub(super) fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selection = self.results.iter().map(|r| r.path.clone()).collect();
        // The range first…last, as a table's select-all: Shift-arrows then adjust it from its end.
        self.anchor = self.results.first().map(|r| r.path.clone());
        self.lead = self.results.last().map(|r| r.path.clone());
        self.selection_changed(window, cx);
    }
}
