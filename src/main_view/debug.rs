//! Debug builds only: the `FASTFIND_DEBUG_*` environment variables for scripted UI checks.

use super::*;
use gpui::{Modifiers, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase};

impl MainView {
    /// Debug builds: `FASTFIND_DEBUG_DIR` picks the folder, `FASTFIND_DEBUG_AUTOINDEX=1` indexes it
    /// at launch and `FASTFIND_DEBUG_QUERY` is searched afterwards (for scripted UI checks).
    /// `FASTFIND_DEBUG_POINTER="x,y;click:x,y;scroll:dy;thumb:f;exit;query:text;sort:n"` then feeds
    /// the window pointer moves and clicks (window points), wheel scrolls where the pointer is, a
    /// scroller thumb drag to a fraction of its track, a pointer exit, a new query and a sort (not
    /// saved), 1.5 s apart, without touching the real mouse, then quits. `FASTFIND_DEBUG_BACKGROUND=1` (with `open -g`) shows the window
    /// without taking the keyboard from the active app.
    pub(super) fn apply_debug_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::env::var("FASTFIND_DEBUG_BACKGROUND").is_ok_and(|v| v == "1") {
            // Launched with `open -g`: on screen, but the keyboard stays with the active app.
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let _ = cx.update(|window, _| {
                    objc2_app_kit::NSApplication::sharedApplication(platform::mtm()).unhideWithoutActivation();
                    if let Some(w) = platform::ns_window(window) {
                        // Above other apps' windows, which would otherwise hide it and stop its drawing.
                        w.setLevel(objc2_app_kit::NSFloatingWindowLevel);
                        w.orderFrontRegardless();
                    }
                    eprintln!("debug background: shown");
                });
            })
            .detach();
        }
        if let Ok(script) = std::env::var("FASTFIND_DEBUG_POINTER") {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let point_of = |xy: &str| {
                    let (x, y) = xy.split_once(',')?;
                    Some(point(
                        px(x.trim().parse::<f32>().ok()?),
                        px(y.trim().parse::<f32>().ok()?),
                    ))
                };
                let modifiers = Modifiers::default();
                for step in script.split(';').map(str::trim) {
                    let mut events = Vec::new();
                    if step == "exit" {
                        let position = point(px(-1.), px(-1.));
                        events.push(PlatformInput::MouseExited(MouseExitEvent {
                            position,
                            pressed_button: None,
                            modifiers,
                        }));
                    } else if let Some(dy) = step.strip_prefix("scroll:").and_then(|dy| dy.parse::<f32>().ok()) {
                        let Ok(position) = cx.update(|window, _| window.mouse_position()) else {
                            continue;
                        };
                        let delta = ScrollDelta::Pixels(point(px(0.), px(dy)));
                        let touch_phase = TouchPhase::Moved;
                        events.push(PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position,
                            delta,
                            modifiers,
                            touch_phase,
                        }));
                    } else if let Some(position) = step.strip_prefix("click:").and_then(point_of) {
                        let (button, click_count) = (MouseButton::Left, 1);
                        events.push(PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: None,
                            modifiers,
                        }));
                        events.push(PlatformInput::MouseDown(MouseDownEvent {
                            button,
                            position,
                            modifiers,
                            click_count,
                            first_mouse: false,
                        }));
                        events.push(PlatformInput::MouseUp(MouseUpEvent {
                            button,
                            position,
                            modifiers,
                            click_count,
                        }));
                    } else if let Some(fraction) = step.strip_prefix("thumb:").and_then(|f| f.parse::<f64>().ok()) {
                        let _ = cx.update(|window, _| {
                            if let Some(view) = platform::ns_view(window) {
                                crate::list_scroller::debug_drag_thumb(&view, fraction);
                            }
                        });
                    } else if let Some((find, replace)) = step.strip_prefix("rename:").and_then(|s| s.split_once('=')) {
                        // The batch-rename preview for the results shown; nothing renames unless confirmed.
                        let _ = cx.update(|window, cx| {
                            let main = window.root::<MainView>().flatten();
                            let results = main.as_ref().map(|m| m.read(cx).results()).unwrap_or_default();
                            let pairs = sheets::preview_changes(&results, find, replace);
                            sheets::open_confirmation(window.window_handle(), main.map(|m| m.downgrade()), pairs, cx);
                        });
                    } else if let Some(text) = step.strip_prefix("query:") {
                        let text = text.to_string();
                        let _ = this.update(cx, |this, cx| {
                            this.search_text = text.clone();
                            this.search_all(Some(&text), false, cx);
                        });
                    } else if let Some(sort) = step.strip_prefix("sort:").and_then(|n| n.parse::<i64>().ok()) {
                        // Shown only, not saved: a test run leaves the user's settings alone.
                        let _ = this.update(cx, |this, cx| {
                            this.settings.sort = SortOption::from_index(sort);
                            this.resort(cx);
                        });
                    } else if let Some(position) = point_of(step) {
                        events.push(PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: None,
                            modifiers,
                        }));
                    }
                    let _ = cx.update(|window, cx| {
                        for event in events {
                            window.dispatch_event(event, cx);
                        }
                    });
                    eprintln!("debug pointer: {step}");
                    cx.background_executor().timer(Duration::from_millis(1500)).await;
                }
                eprintln!("debug pointer: done");
                let _ = cx.update(|_, cx| cx.quit());
            })
            .detach();
        }
        // `FASTFIND_DEBUG_OFFER=1.1.0` shows the update question for that version (nothing installs).
        if let Ok(version) = std::env::var("FASTFIND_DEBUG_OFFER") {
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(Duration::from_millis(1500)).await;
                eprintln!("debug offer: shown");
                let update = crate::update::offer(&version);
                eprintln!("debug offer: answered update={update}");
            })
            .detach();
        }
        if let Ok(dir) = std::env::var("FASTFIND_DEBUG_DIR") {
            let dir = PathBuf::from(dir);
            self.directory = Some(dir);
        }
        self.debug_query = std::env::var("FASTFIND_DEBUG_QUERY").ok();
        if std::env::var("FASTFIND_DEBUG_AUTOINDEX").is_ok_and(|v| v == "1") {
            self.cache_data(cx);
        }
    }
}
