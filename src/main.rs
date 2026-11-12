//! FastFind — SearchEverything rebuilt in Rust on GPUI + NativeGPUI.

mod appkit_bridge;
mod file_ops;
mod i18n;
mod list_scroller;
mod main_view;
mod options;
mod platform;
mod settings;
mod sheets;
mod update;

use fastfind_core::Index;
use gpui::{App, KeyBinding};
use main_view::{
    open_main_window, CopyNames, Engine, ExtendNext, ExtendPrevious, OpenSelection, SelectAllRows, SelectNext,
    SelectPrevious, ShowAbout, ToggleQuickLook, LIST_CONTEXT,
};
use native_gpui::{install_menus, replace_about_row, replace_edit_rows, set_wording, tokens, MenuCommand, TextCommand};
use std::sync::Arc;

pub const APP_NAME: &str = "FastFind";

fn install_app_menus(cx: &mut App) {
    set_wording(i18n::menu_wording);
    install_menus(cx);
    replace_about_row(ShowAbout);
    // Copy / Select All act on the results list unless a text field is editing.
    replace_edit_rows([
        MenuCommand::appkit("Undo", c"undo:").key("cmd-z"),
        MenuCommand::appkit("Redo", c"redo:").key("cmd-shift-z"),
        MenuCommand::separator(),
        MenuCommand::appkit("Cut", c"cut:").key("cmd-x"),
        MenuCommand::new("Copy", CopyNames)
            .key("cmd-c")
            .edits_text(TextCommand::Copy),
        MenuCommand::appkit("Paste", c"paste:").key("cmd-v"),
        MenuCommand::appkit("Paste and Match Style", c"pasteAsPlainText:").key("cmd-alt-shift-v"),
        MenuCommand::appkit("Delete", c"delete:"),
        MenuCommand::new("Select All", SelectAllRows)
            .key("cmd-a")
            .edits_text(TextCommand::SelectAll),
    ]);
}

fn main() {
    let app = gpui_platform::application();
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_main_window(cx);
        }
    });
    app.run(|cx: &mut App| {
        tokens::init(cx);
        cx.set_global(Engine {
            index: Arc::new(Index::new()),
        });
        install_app_menus(cx);
        cx.on_action(|_: &ShowAbout, _| platform::show_about_panel());
        cx.bind_keys([
            KeyBinding::new("up", SelectPrevious, Some(LIST_CONTEXT)),
            KeyBinding::new("down", SelectNext, Some(LIST_CONTEXT)),
            KeyBinding::new("shift-up", ExtendPrevious, Some(LIST_CONTEXT)),
            KeyBinding::new("shift-down", ExtendNext, Some(LIST_CONTEXT)),
            KeyBinding::new("space", ToggleQuickLook, Some(LIST_CONTEXT)),
            KeyBinding::new("enter", OpenSelection, Some(LIST_CONTEXT)),
            KeyBinding::new("cmd-c", CopyNames, Some(LIST_CONTEXT)),
            KeyBinding::new("cmd-a", SelectAllRows, Some(LIST_CONTEXT)),
        ]);
        open_main_window(cx);
        update::start(cx);
        // Debug builds: `FASTFIND_DEBUG_BACKGROUND=1` leaves the keyboard with whatever app has it.
        if !(cfg!(debug_assertions) && std::env::var("FASTFIND_DEBUG_BACKGROUND").is_ok_and(|v| v == "1")) {
            cx.activate(true);
        }
    });
}
