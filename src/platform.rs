//! Thin AppKit/Foundation wrappers for what `FileOps`, `NSAlert`, `NSOpenPanel` etc. did in Swift.

use block2::RcBlock;
use gpui::{Hsla, Window};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSAboutPanelOptionCredits, NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSColor, NSFont,
    NSFontAttributeName, NSModalResponseOK, NSOpenPanel, NSView, NSWindow, NSWorkspace,
};
use objc2_foundation::{NSAppleScript, NSArray, NSAttributedString, NSDictionary, NSFileManager, NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit calls run on the main thread")
}

pub fn ns_string(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

pub fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns_string(&path.to_string_lossy()))
}

pub fn ns_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    unsafe { Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }
}

pub fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    ns_view(window)?.window()
}

/// False while an AppKit control (a text field) holds the keyboard instead of GPUI's view.
pub fn gpui_has_keyboard(window: &Window) -> bool {
    let (Some(view), Some(ns_window)) = (ns_view(window), ns_window(window)) else {
        return true;
    };
    ns_window.firstResponder().is_some_and(|responder| {
        std::ptr::eq(
            Retained::as_ptr(&responder).cast::<u8>(),
            Retained::as_ptr(&view).cast::<u8>(),
        )
    })
}

/// `FileOps.openFiles`.
pub fn open_paths(paths: &[PathBuf]) {
    let workspace = NSWorkspace::sharedWorkspace();
    for path in paths {
        if !workspace.openURL(&file_url(path)) {
            eprintln!("Failed to open file at {}", path.display());
        }
    }
}

/// `FileOps.openFolder` — shows the items selected in Finder. Swift opened the folder and then
/// selected the item in a viewer rooted there, which could leave two Finder windows.
pub fn reveal_in_finder(paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let urls: Vec<Retained<NSURL>> = paths.iter().map(|p| file_url(p)).collect();
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&urls));
}

pub fn open_url(url: &str) -> bool {
    let Some(url) = NSURL::URLWithString(&ns_string(url)) else {
        return false;
    };
    NSWorkspace::sharedWorkspace().openURL(&url)
}

/// `FileOps.showFileInfo` — Finder's Get Info window, via AppleScript.
pub fn show_info(paths: &[PathBuf]) {
    for path in paths {
        let escaped = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "tell application \"Finder\"\n    activate\n    set theItem to POSIX file \"{escaped}\" as alias\n    open information window of theItem\nend tell"
        );
        let Some(script) = NSAppleScript::initWithSource(NSAppleScript::alloc(), &ns_string(&script)) else {
            continue;
        };
        let mut error = None;
        unsafe { script.executeAndReturnError(Some(&mut error)) };
        if let Some(error) = error {
            eprintln!("AppleScript Error: {error:?}");
        }
    }
}

pub fn move_to_trash(path: &Path) -> Result<(), String> {
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&file_url(path), None)
        .map_err(|e| e.localizedDescription().to_string())
}

pub fn copy_item(from: &Path, to: &Path) -> Result<(), String> {
    NSFileManager::defaultManager()
        .copyItemAtURL_toURL_error(&file_url(from), &file_url(to))
        .map_err(|e| e.localizedDescription().to_string())
}

pub fn move_item(from: &Path, to: &Path) -> Result<(), String> {
    NSFileManager::defaultManager()
        .moveItemAtURL_toURL_error(&file_url(from), &file_url(to))
        .map_err(|e| e.localizedDescription().to_string())
}

/// The system alert sound.
pub fn beep() {
    objc2_app_kit::NSBeep();
}

/// `NSAlert.runModal()` with a warning style; returns true for the first button. Runs a nested modal
/// loop, so call it from a spawned task, never while holding GPUI's `App` borrow.
pub fn confirm(message: &str, first: &str, second: &str) -> bool {
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&ns_string(message));
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.addButtonWithTitle(&ns_string(first));
    alert.addButtonWithTitle(&ns_string(second));
    alert.runModal() == NSAlertFirstButtonReturn
}

/// An informational `NSAlert` with a detail line; true for the first button. Runs a nested modal
/// loop, like [`confirm`].
pub fn ask(message: &str, detail: &str, first: &str, second: &str) -> bool {
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&ns_string(message));
    alert.setInformativeText(&ns_string(detail));
    alert.setAlertStyle(NSAlertStyle::Informational);
    alert.addButtonWithTitle(&ns_string(first));
    alert.addButtonWithTitle(&ns_string(second));
    alert.runModal() == NSAlertFirstButtonReturn
}

/// A warning `NSAlert` with only OK.
pub fn inform(message: &str, detail: &str) {
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&ns_string(message));
    alert.setInformativeText(&ns_string(detail));
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.runModal();
}

/// FastFind is the frontmost app.
pub fn app_is_active() -> bool {
    NSApplication::sharedApplication(mtm()).isActive()
}

/// A sheet (rename, batch rename) is up. AppKit refuses to quit the app meanwhile.
pub fn sheet_open() -> bool {
    NSApplication::sharedApplication(mtm())
        .windows()
        .iter()
        .any(|window| window.attachedSheet().is_some())
}

/// `NSOpenPanel.begin` for a single folder.
pub fn choose_directory(prompt: &str, message: &str) -> futures::channel::oneshot::Receiver<Option<PathBuf>> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let panel = NSOpenPanel::openPanel(mtm());
    panel.setPrompt(Some(&ns_string(prompt)));
    panel.setMessage(Some(&ns_string(message)));
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    let tx = RefCell::new(Some(tx));
    let held = panel.clone();
    let handler = RcBlock::new(move |response: isize| {
        let picked = (response == NSModalResponseOK)
            .then(|| held.URLs().firstObject())
            .flatten()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()));
        if let Some(tx) = tx.borrow_mut().take() {
            let _ = tx.send(picked);
        }
    });
    panel.beginWithCompletionHandler(&handler);
    rx
}

/// The standard About panel with the credits SearchEverything showed.
pub fn show_about_panel() {
    let mtm = mtm();
    let app = NSApplication::sharedApplication(mtm);
    let font = NSFont::boldSystemFontOfSize(NSFont::smallSystemFontSize());
    let attributes: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_retained_objects(
        &[unsafe { NSFontAttributeName }],
        &[Retained::into_super(Retained::into_super(font))],
    );
    let credits = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &ns_string("임바다 제작. 𝕏 @BD_MARU"),
            Some(&attributes),
        )
    };
    let copyright = ns_string("© 2024 BADA IM");
    let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_retained_objects(
        &[unsafe { NSAboutPanelOptionCredits }, &*ns_string("Copyright")],
        &[
            Retained::into_super(Retained::into_super(credits)),
            Retained::into_super(Retained::into_super(copyright)),
        ],
    );
    app.activate();
    unsafe { app.orderFrontStandardAboutPanelWithOptions(&options) };
}

/// `NSColor.alternatingContentBackgroundColors` resolved for the current appearance.
pub fn alternating_row_colors() -> (Hsla, Hsla) {
    let colors = NSColor::alternatingContentBackgroundColors();
    let pick = |i: usize| {
        colors
            .iter()
            .nth(i)
            .and_then(|c| native_gpui::tokens::srgb(&c))
            .unwrap_or(gpui::transparent_black())
    };
    (pick(0), pick(1))
}

pub fn app_version() -> String {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    bundle
        .objectForInfoDictionaryKey(&ns_string("CFBundleShortVersionString"))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|v| v.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}
