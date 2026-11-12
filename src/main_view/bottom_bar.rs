//! The bar under the results: folder, status, reverse and sort.

use super::*;

impl MainView {
    pub(super) fn render_bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let dot = if self.is_searching {
            p.cyan
        } else if self.is_indexing {
            p.red
        } else {
            p.green
        };
        let path_text: SharedString = match &self.directory {
            Some(dir) => dir.to_string_lossy().into_owned().into(),
            None => tr("No path").into(),
        };
        let status = if self.is_searching {
            tr("Searching Now... ").to_string()
        } else if self.index_failed && self.results.is_empty() {
            tr("Could not read this folder. Check its permissions.").to_string()
        } else if !self.results.is_empty() {
            format!(
                "{} ({} {})",
                tr("Search complete. "),
                self.results.len(),
                tr("results.")
            )
        } else if self.directory.is_some() {
            tr("Search ready. ").to_string()
        } else {
            tr("Please select a path first").to_string()
        } + &self.terminal_output;

        let check = if self.settings.reverse {
            "checkmark.square.fill"
        } else {
            "square"
        };
        let sort_labels: Vec<String> = SortOption::ALL.iter().map(|o| tr(o.label_key()).to_string()).collect();

        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_none().size(px(10.)).rounded_full().bg(dot))
                            .child(div().min_w_0().truncate().child(path_text)),
                    )
                    .child(div().text_size(px(11.)).line_height(px(14.)).truncate().child(status)),
            )
            .child(
                div()
                    .id("ff.reverse")
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .cursor_default()
                    .children(tokens::sf_symbol(check, px(13.), FontWeight::NORMAL).map(|s| s.img()))
                    .child(tr("Reverse"))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_reverse(cx))),
            )
            .child(
                div().flex_none().w(px(100.)).child(
                    native_popup("ff.sort", self.settings.sort as usize)
                        .items(sort_labels)
                        .width(px(100.))
                        .on_change(
                            cx.listener(|this, ix: &usize, _, cx| {
                                this.set_sort(SortOption::from_index(*ix as i64), cx)
                            }),
                        ),
                ),
            )
    }
}
