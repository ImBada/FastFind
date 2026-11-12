//! The main window (`ContentView` + `TableListView` + `FileListItemView` + `SortManagerView`): its
//! state, construction and frame. The rest is split by concern, each file another `impl MainView`:
//! `toolbar` (toolbar, options, folder picker), `indexing`, `search` (searching, sorting, keeping the
//! selection in view), `selection` (selection, keyboard, mouse, Quick Look), `actions` (file
//! operations and the context menu), `row`, `table` and `bottom_bar` (drawing), `palette`, and
//! `debug` (debug builds only).

use crate::file_ops;
use crate::i18n::tr;
use crate::list_scroller::{list_scroll_bars, RowEstimate};
use crate::options::{OptionEvent, OptionHandler, OptionsPopover, OptionsState};
use crate::platform;
use crate::settings::Settings;
use crate::sheets;
use fastfind_core::file_info::set_convert_full_width;
use fastfind_core::index::IndexError;
use fastfind_core::row::{parse_row, RowData};
use fastfind_core::search::{plan, run};
use fastfind_core::sort::sort_file_infos;
use fastfind_core::text::{format_file_size, highlight_ranges, nfc};
use fastfind_core::{FileInfo, Index, Plan, SearchOptions, Shown, SortOption};
use futures::channel::mpsc;
use futures::StreamExt;
use gpui::prelude::*;
use gpui::{
    actions, canvas, div, fill, list, point, px, relative, size, AnyElement, App, Bounds, ClickEvent, Context, Entity,
    ExternalPaths, FocusHandle, FontWeight, HighlightStyle, Hsla, ListAlignment, ListOffset, ListState, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString, StyledText,
    Task, WeakEntity, Window,
};
use native_gpui::tokens::{self, FileType, SystemColor, SystemTheme};
use native_gpui::{
    native_button, native_context_menu, native_popup, native_root, native_spinner, native_symbol, native_text_field,
    quick_look, quick_look_close, quick_look_update, quick_look_visible, set_toolbar, titlebar_band, ControlSize,
    MenuEntry, QuickLookKey, ToolbarItem, ToolbarStyle,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod actions;
mod bottom_bar;
#[cfg(debug_assertions)]
mod debug;
mod indexing;
mod palette;
mod row;
mod search;
mod selection;
mod table;
mod toolbar;

use palette::{palette, Palette};
use table::item_bounds;

actions!(
    fastfind,
    [
        SelectPrevious,
        SelectNext,
        ExtendPrevious,
        ExtendNext,
        ToggleQuickLook,
        OpenSelection,
        CopyNames,
        SelectAllRows,
        ShowAbout,
    ]
);

pub const LIST_CONTEXT: &str = "ResultList";
pub const PATH_ITEM: &str = "ff.path";
pub const OPTIONS_ITEM: &str = "ff.options";

const ROW_INSET: f32 = 10.;
/// Each row's hover group: the whole row line, side margins included.
const ROW_GROUP: &str = "ff.row";
const STRIPE_HEIGHT: f32 = 24.;
const ROW_RADIUS: f32 = 5.;

/// Engine state that outlives a window (the Swift cache was a static).
pub struct Engine {
    pub index: Arc<Index>,
}

impl gpui::Global for Engine {}

#[derive(Clone)]
enum Icon {
    Folder,
    Path(String),
    Extension(String),
}

struct PendingAnchor {
    path: Arc<str>,
    /// Its top, relative to the list's top.
    want: Pixels,
    tries: u8,
}

struct RowModel {
    data: RowData,
    size_text: String,
    icon: Icon,
}

#[derive(Clone, Copy, Debug)]
enum MenuAction {
    Open,
    ShowInfo,
    Reveal,
    CopyPath,
    CopyName,
    Deck(u8),
    BatchRename,
    Rename,
    Trash,
}

enum SearchOutcome {
    Unchanged,
    Results(Vec<Arc<FileInfo>>, Shown),
}

pub struct MainView {
    index: Arc<Index>,
    settings: Settings,
    directory: Option<PathBuf>,
    search_text: String,
    results: Arc<Vec<Arc<FileInfo>>>,
    positions: HashMap<Arc<str>, usize>,
    /// What `results` came from: the last query and the narrowing basis.
    shown: Shown,
    is_searching: bool,
    terminal_output: String,
    is_indexing: bool,
    indexed_count: usize,
    hide_tags: bool,
    original_filename: bool,
    debounce: Option<Task<()>>,
    /// The search in `debounce` is a forced one.
    force_pending: bool,
    /// A re-sort of the results on screen; separate from `debounce` so it never cancels a search.
    sort_task: Option<Task<()>>,
    /// The index generation this view's walk fills (`Index::clear`'s), to ignore an older walk.
    index_epoch: u64,
    /// The folder could not be read by the last indexing.
    index_failed: bool,
    index_cancel: Option<Arc<AtomicBool>>,
    index_task: Option<Task<()>>,
    list_state: ListState,
    row_estimate: Rc<RefCell<RowEstimate>>,
    /// GPUI keeps the pointer's last position when it leaves the window, so its hover styles
    /// would leave the row it left through lit. False from that exit until the pointer moves in
    /// the list again.
    pointer_in_list: bool,
    focus: FocusHandle,
    selection: HashSet<Arc<str>>,
    anchor: Option<Arc<str>>,
    lead: Option<Arc<str>>,
    /// A press inside a multi-selection, collapsed to that item on release unless it starts a drag.
    /// Items, not row numbers: results may change between press and release.
    pending_click: Option<Arc<str>>,
    /// A selected item `set_results` is still placing on screen.
    pending_anchor: Option<PendingAnchor>,
    /// Old path → new path of items just renamed, so the next results keep them selected.
    renamed: HashMap<Arc<str>, Arc<str>>,
    drag_start: Option<(Point<Pixels>, Arc<FileInfo>)>,
    /// The item the context menu was opened on (its focus ring).
    context_row: Option<Arc<str>>,
    menu_actions: Vec<Option<MenuAction>>,
    menu_targets: Vec<Arc<FileInfo>>,
    /// The row the menu was opened on: Rename takes it, not the first of a multi-selection.
    menu_clicked: Option<Arc<FileInfo>>,
    /// The VJDeck buttons each row shows, parsed from the setting once per frame.
    deck_numbers: Vec<u8>,
    /// Drops still copying or moving; the "Copying..." overlay stays while any is.
    copying: usize,
    /// Per path, with the item it was built from: a replaced or renamed-over file is a new item.
    row_cache: HashMap<Arc<str>, (Arc<FileInfo>, Rc<RowModel>)>,
    edits: HashMap<Arc<str>, String>,
    options: Option<OptionsPopover>,
    focus_search: bool,
    search_was_enabled: bool,
    quick_look_keys: mpsc::UnboundedSender<QuickLookKey>,
    /// Debug builds only: `FASTFIND_DEBUG_QUERY` searched once indexing finishes.
    debug_query: Option<String>,
    /// Whether selected rows draw emphasized (list has the keyboard), computed once per frame.
    list_emphasis: bool,
    _subscriptions: Vec<gpui::Subscription>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine = cx.global::<Engine>();
        let index = engine.index.clone();
        let settings = Settings::load();
        set_convert_full_width(settings.convert_full_width);
        // A saved folder that is not there now (an unmounted drive) is not searched, but stays saved.
        let directory = settings.directory.clone().filter(|dir| dir.is_dir());

        let (quick_look_keys, mut keys) = mpsc::unbounded::<QuickLookKey>();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(key) = keys.next().await {
                let delta = match key {
                    QuickLookKey::Up => -1,
                    QuickLookKey::Down => 1,
                    _ => continue,
                };
                if this
                    .update_in(cx, |this, window, cx| this.move_selection(delta, false, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let subscriptions = vec![
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
            cx.observe_global_in::<SystemTheme>(window, |_, _, cx| cx.notify()),
        ];

        #[allow(unused_mut)] // changed only by the debug hooks
        let mut this = MainView {
            index,
            settings,
            directory,
            search_text: String::new(),
            results: Arc::new(Vec::new()),
            positions: HashMap::new(),
            shown: Shown::none(),
            is_searching: false,
            terminal_output: String::new(),
            is_indexing: false,
            indexed_count: 0,
            hide_tags: false,
            original_filename: false,
            debounce: None,
            force_pending: false,
            sort_task: None,
            index_epoch: 0,
            index_failed: false,
            index_cancel: None,
            index_task: None,
            list_state: ListState::new(0, ListAlignment::Top, px(400.)),
            row_estimate: RowEstimate::new(),
            pointer_in_list: false,
            focus: cx.focus_handle(),
            selection: HashSet::new(),
            anchor: None,
            lead: None,
            pending_click: None,
            pending_anchor: None,
            renamed: HashMap::new(),
            drag_start: None,
            context_row: None,
            menu_actions: Vec::new(),
            menu_targets: Vec::new(),
            menu_clicked: None,
            copying: 0,
            deck_numbers: Vec::new(),
            row_cache: HashMap::new(),
            edits: HashMap::new(),
            options: None,
            focus_search: true,
            search_was_enabled: false,
            quick_look_keys,
            debug_query: None,
            list_emphasis: false,
            _subscriptions: subscriptions,
        };
        #[cfg(debug_assertions)]
        this.apply_debug_env(window, cx);
        this
    }
}

impl Drop for MainView {
    /// Closing the window ends its indexing walk, which otherwise runs on in its thread.
    fn drop(&mut self) {
        if let Some(cancel) = &self.index_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        self.settle_anchor(window);
        self.sync_options();
        self.deck_numbers = if self.settings.vjdeck_enabled {
            file_ops::parse_deck_numbers(&self.settings.vjdeck_numbers)
        } else {
            Vec::new()
        };
        self.list_emphasis = self.list_focused(window);
        let family = tokens::text_font_family(cx).unwrap_or_else(tokens::system_font_family);
        let top = titlebar_band(window) + px(6.);

        let search_enabled = !(self.is_indexing || self.should_show_index_button());
        if search_enabled && !self.search_was_enabled {
            self.focus_search = true;
        }
        self.search_was_enabled = search_enabled;
        let mut field = native_text_field("ff.search", &self.search_text)
            .borderless()
            .stretch()
            .placeholder(tr("Search"))
            .enabled(search_enabled)
            .on_change(cx.listener(|this, text: &String, _, cx| {
                this.search_text = text.clone();
                this.search_all(Some(text), false, cx);
            }))
            .on_submit(cx.listener(|this, text: &String, _, cx| this.search_all(Some(text), false, cx)));
        if search_enabled && std::mem::take(&mut self.focus_search) {
            let len = self.search_text.chars().count();
            field = field.editing(len..len);
        }

        let body: AnyElement = if self.is_indexing {
            self.render_indexing().into_any_element()
        } else if self.should_show_index_button() {
            self.render_index_button(cx).into_any_element()
        } else {
            self.render_table(cx).into_any_element()
        };

        native_root(
            div()
                .size_full()
                .relative()
                .font_family(family)
                .text_size(px(13.))
                .line_height(px(16.))
                .text_color(p.label)
                .bg(p.window_bg)
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(top)
                .px(px(16.))
                .pb(px(16.))
                .on_action(cx.listener(|this, _: &CopyNames, _, cx| this.copy_names(cx)))
                .on_action(cx.listener(|this, _: &SelectAllRows, window, cx| this.select_all(window, cx)))
                .child(
                    div()
                        .flex_none()
                        .h(px(32.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .bg(p.control_bg)
                        .border_1()
                        .border_color(p.separator)
                        .flex()
                        .items_center()
                        .child(div().flex_1().mt(px(4.)).child(field)),
                )
                .child(body)
                .child(self.render_bottom_bar(cx)),
        )
    }
}

pub fn open_main_window(cx: &mut App) {
    use gpui::{Bounds, WindowBounds, WindowOptions};
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(664.), px(705.)),
            cx,
        ))),
        window_min_size: Some(size(px(300.), px(300.))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(crate::APP_NAME.into()),
            appears_transparent: true,
            ..Default::default()
        }),
        ..native_gpui::window_options()
    };
    let opened = cx.open_window(options, |window, cx| {
        native_gpui::remember_frame(window, "FastFind.Main");
        let view: Entity<MainView> = cx.new(|cx| MainView::new(window, cx));
        MainView::install_toolbar(window, cx, view.downgrade());
        view
    });
    if let Err(e) = opened {
        eprintln!("failed to open window: {e}");
    }
}
