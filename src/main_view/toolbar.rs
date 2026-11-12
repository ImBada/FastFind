//! The toolbar, the options popover and the folder picker.

use super::*;

impl MainView {
    pub fn install_toolbar(window: &mut Window, cx: &mut App, view: WeakEntity<MainView>) {
        let (for_path, for_options) = (view.clone(), view);
        set_toolbar(
            window,
            cx,
            Some(ToolbarStyle::Unified),
            &[
                ToolbarItem::flexible_space(),
                ToolbarItem::symbol(PATH_ITEM, "folder.badge.gear", tr("Select a path")).on_click(move |_, cx| {
                    let _ = for_path.update(cx, |this, cx| this.request_permission(cx));
                }),
                ToolbarItem::symbol(OPTIONS_ITEM, "gear", tr("Options")).on_click(move |window, cx| {
                    let _ = for_options.update(cx, |this, cx| this.toggle_options(window, cx));
                }),
            ],
        );
    }

    pub(super) fn options_state(&self) -> OptionsState {
        OptionsState {
            by_folder: self.settings.by_folder,
            by_folder_double: self.settings.by_folder_double,
            hide_tags: self.hide_tags,
            original_filename: self.original_filename,
            convert_full_width: self.settings.convert_full_width,
            indexing: self.is_indexing,
            vjdeck_enabled: self.settings.vjdeck_enabled,
            vjdeck_numbers: self.settings.vjdeck_numbers.clone(),
        }
    }

    pub(super) fn sync_options(&self) {
        if let Some(popover) = self.options.as_ref().filter(|p| p.is_shown()) {
            popover.sync(&self.options_state());
        }
    }

    pub(super) fn toggle_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popover) = self.options.take() {
            if popover.is_shown() {
                popover.close();
                return;
            }
            // A transient popover closes on the mouse-down that is about to press this button.
            if popover.closed_recently() {
                return;
            }
        }
        let Some(ns_window) = platform::ns_window(window) else {
            return;
        };
        let weak = cx.entity().downgrade();
        let handler: OptionHandler = Rc::new(move |event, window, cx| {
            let _ = weak.update(cx, |this, cx| this.on_option(event, window, cx));
        });
        self.options = OptionsPopover::show(
            &ns_window,
            OPTIONS_ITEM,
            &self.options_state(),
            window.to_async(cx),
            handler,
        );
    }

    pub(super) fn on_option(&mut self, event: OptionEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event {
            OptionEvent::ByFolder => {
                self.settings.by_folder = !self.settings.by_folder;
                self.settings.save();
                self.search_all(None, true, cx);
            }
            OptionEvent::ByFolderDouble => {
                self.settings.by_folder_double = !self.settings.by_folder_double;
                self.settings.save();
                self.search_all(None, true, cx);
            }
            // Display only: the rows are drawn again, no search (Swift re-searched to redraw its table).
            OptionEvent::HideTags => {
                self.hide_tags = !self.hide_tags;
                cx.notify();
            }
            OptionEvent::OriginalFilename => {
                self.original_filename = !self.original_filename;
                self.edits.clear();
                cx.notify();
            }
            OptionEvent::ConvertFullWidth => {
                if self.is_indexing {
                    return;
                }
                self.settings.convert_full_width = !self.settings.convert_full_width;
                self.settings.save();
                set_convert_full_width(self.settings.convert_full_width);
                self.cache_data(cx);
            }
            OptionEvent::VjdeckEnabled => {
                self.settings.vjdeck_enabled = !self.settings.vjdeck_enabled;
                self.settings.save();
            }
            OptionEvent::VjdeckNumbers(numbers) => {
                self.settings.vjdeck_numbers = numbers;
                self.settings.save();
            }
        }
        self.sync_options();
        cx.notify();
    }

    /// `requestPermission()` — pick the folder to search, then index it.
    pub(super) fn request_permission(&mut self, cx: &mut Context<Self>) {
        let rx = platform::choose_directory(tr("Select"), "Choose a path to search");
        cx.spawn(async move |this, cx| {
            let Ok(Some(dir)) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.directory = Some(dir.clone());
                this.settings.directory = Some(dir);
                this.settings.save();
                this.cache_data(cx);
            });
        })
        .detach();
    }
}
