//! The results list's scroll bar: a real `NSScroller`, as SwiftUI's `List` (an `NSTableView` in an
//! `NSScrollView`) showed. NativeGPUI's `native_scroll_bars` follows a `ScrollHandle`; this follows
//! the variable-height `ListState` the rows live in.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    point, px, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, ListOffset,
    ListState, Pixels, Size, Style, Window,
};
use native_gpui::{element_key, Layer, NativeHost, NativeRead, Target};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSControlSize, NSEvent, NSResponder, NSScrollView, NSScrollViewDidEndLiveScrollNotification,
    NSScrollViewWillStartLiveScrollNotification, NSScroller, NSScrollerStyle, NSView,
    NSViewBoundsDidChangeNotification,
};
use objc2_foundation::{MainThreadMarker, NSNotificationCenter, NSObject, NSPoint, NSRect, NSSize};

const FLASH_EVERY: Duration = Duration::from_millis(500);
/// Until a row has been measured: a name line plus a tag line.
const FIRST_GUESS: f32 = 44.;

/// What the scroll bar learns about the rows, shared with the view that resets the list.
pub struct RowEstimate {
    hint: Pixels,
    width: Option<Pixels>,
    layout_key: Option<u64>,
    /// Room the rows keep clear on the right for an always-shown (legacy) scroller. GPUI's
    /// `list` sizes rows to its full width whatever its padding, so the rows leave it themselves.
    reserve: Pixels,
}

impl RowEstimate {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            hint: px(FIRST_GUESS),
            width: None,
            layout_key: None,
            reserve: px(0.),
        }))
    }

    /// Height given to rows not yet rendered, so the thumb is sized for the whole list.
    pub fn hint(&self) -> Pixels {
        self.hint
    }

    pub fn reserve(&self) -> Pixels {
        self.reserve
    }
}

/// `inset` is the list's top (and bottom) padding. `layout_key` changes whenever the rows' height
/// does (tags shown or hidden, …), which re-estimates the rows not on screen.
pub fn list_scroll_bars(
    id: impl Into<ElementId>,
    list: &ListState,
    inset: Pixels,
    layout_key: u64,
    estimate: &Rc<RefCell<RowEstimate>>,
) -> ListScrollBars {
    ListScrollBars {
        id: id.into(),
        list: list.clone(),
        inset,
        layout_key,
        estimate: estimate.clone(),
    }
}

pub struct ListScrollBars {
    id: ElementId,
    list: ListState,
    inset: Pixels,
    layout_key: u64,
    estimate: Rc<RefCell<RowEstimate>>,
}

impl ListScrollBars {
    /// Runs after the list's own layout: its rows on screen are measured by now. True when the
    /// list was re-estimated and wants drawing again.
    fn estimate_rows(&self) -> bool {
        let list = &self.list;
        let count = list.item_count();
        let viewport = list.viewport_bounds();
        let (mut total, mut rows) = (0f32, 0usize);
        for ix in list.logical_scroll_top().item_ix..count {
            let Some(row) = list.bounds_for_item(ix) else { break };
            if row.top() > viewport.bottom() {
                break;
            }
            total += f32::from(row.size.height);
            rows += 1;
        }
        let mut estimate = self.estimate.borrow_mut();
        let width = viewport.size.width;
        if rows > 0 && estimate.layout_key != Some(self.layout_key) {
            // Every row changed height: re-guess the ones off screen, keeping the scroll position.
            estimate.layout_key = Some(self.layout_key);
            estimate.hint = px(total / rows as f32);
            estimate.width = Some(width);
            let top = list.logical_scroll_top();
            list.reset_with_uniform_height(count, estimate.hint);
            list.scroll_to(top);
            return true;
        }
        if estimate.width != Some(width) {
            // A new width drops every measured height (GPUI re-measures on demand): guess again.
            estimate.width = Some(width);
            let _ = list.clone().with_uniform_item_height(estimate.hint);
        }
        false
    }

    /// (how far the list can scroll, how far it has), both from the top.
    fn travel(&self) -> (f64, f64) {
        let list = &self.list;
        let offset = -list.scroll_px_offset_for_scrollbar().y;
        // `max_offset_for_scrollbar` leaves out the list's padding.
        let rows_past = list.max_offset_for_scrollbar().y;
        let max = if rows_past > px(0.) {
            rows_past + self.inset * 2.
        } else {
            let count = list.item_count();
            let content_bottom = (count > 0)
                .then(|| list.bounds_for_item(count - 1))
                .flatten()
                .map(|last| last.bottom() + self.inset * 2.);
            match content_bottom {
                Some(bottom) => (offset + bottom - list.viewport_bounds().bottom()).max(px(0.)),
                None => offset.max(px(0.)),
            }
        };
        let max = f64::from(f32::from(max));
        (max, f64::from(f32::from(offset)).clamp(0., max))
    }

    fn paint(&mut self, id: &GlobalElementId, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let mut redraw = self.estimate_rows();
        let (max, offset) = self.travel();
        let key = element_key(Some(id));
        let host = NativeHost::get(window, cx);
        let mask = window.content_mask().bounds;
        let bars = host.place(key, Layer::Above, bounds, mask, |mtm, target| {
            ListBars::new(mtm, target, key)
        });
        let content = NSSize::new(
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)) + max,
        );
        let shown = max >= 1.;
        bars.show(content, shown, NSPoint::new(0., offset));

        let reserve = if shown && bars.scrollerStyle() == NSScrollerStyle::Legacy {
            px(NSScroller::scrollerWidthForControlSize_scrollerStyle(
                NSControlSize::Regular,
                NSScrollerStyle::Legacy,
                MainThreadMarker::from(&*bars),
            ) as f32)
        } else {
            px(0.)
        };
        {
            let mut estimate = self.estimate.borrow_mut();
            if estimate.reserve != reserve {
                // The rows narrow (or widen) by the scroller: their heights may change.
                estimate.reserve = reserve;
                self.list.remeasure();
                redraw = true;
            }
        }
        if redraw {
            // `refresh` is ignored mid-draw.
            window.defer(cx, |window, _| window.refresh());
        }

        let list = self.list.clone();
        host.on_event(
            key,
            read(),
            Some(Rc::new(move |value, window, _| {
                let Ok(moved) = value.downcast::<Moved>() else { return };
                match moved.phase {
                    Phase::Started => {
                        if !list.is_scrollbar_dragging() {
                            list.scrollbar_drag_started();
                        }
                    }
                    Phase::Ended => list.scrollbar_drag_ended(),
                    // At the end of its travel the thumb means the list's end. Rows not yet drawn only
                    // have estimated heights (and GPUI holds the total still during a drag), so the
                    // offset alone would land short of it, by however much those estimates were off.
                    Phase::Moved if moved.at_end => list.scroll_to(ListOffset {
                        item_ix: list.item_count(),
                        offset_in_item: px(0.),
                    }),
                    Phase::Moved => list.set_offset_from_scrollbar(point(px(0.), px(-moved.y as f32))),
                }
                window.refresh();
            })),
        );
    }
}

impl Element for ListScrollBars {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            size: Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(id) = id {
            ListScrollBars::paint(self, id, bounds, window, cx);
        }
    }
}

impl IntoElement for ListScrollBars {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Started,
    Moved,
    Ended,
}

struct Moved {
    phase: Phase,
    y: f64,
    /// The thumb is at the bottom of its track.
    at_end: bool,
}

#[cfg(debug_assertions)]
/// Debug builds: drags the results scroller's thumb to `fraction` (0…1) of its travel, posting the
/// live-scroll notifications a real drag does, for scripted checks without the real mouse.
pub fn debug_drag_thumb(root: &NSView, fraction: f64) {
    fn find(view: &NSView) -> Option<Retained<ListBars>> {
        for sub in view.subviews().iter() {
            if objc2_foundation::NSObjectProtocol::isKindOfClass(&*sub, <ListBars as objc2::ClassType>::class()) {
                return Some(unsafe { Retained::cast_unchecked::<ListBars>(sub) });
            }
            if let Some(found) = find(&sub) {
                return Some(found);
            }
        }
        None
    }
    let Some(bars) = find(root) else { return };
    let center = NSNotificationCenter::defaultCenter();
    let clip = bars.contentView();
    let document = bars.documentView().map_or(0., |d| d.frame().size.height);
    let room = (document - clip.bounds().size.height).max(0.);
    unsafe {
        center.postNotificationName_object(NSScrollViewWillStartLiveScrollNotification, Some(&bars));
    }
    clip.scrollToPoint(NSPoint::new(0., room * fraction));
    bars.reflectScrolledClipView(&clip);
    unsafe {
        center.postNotificationName_object(NSScrollViewDidEndLiveScrollNotification, Some(&bars));
    }
}

fn read() -> NativeRead {
    Rc::new(|sender| {
        let bars = unsafe { &*(sender as *const AnyObject).cast::<ListBars>() };
        let clip = bars.contentView().bounds();
        let document = bars.documentView().map_or(0., |d| d.frame().size.height);
        Box::new(Moved {
            phase: bars.ivars().phase.get(),
            y: clip.origin.y,
            at_end: document > clip.size.height && clip.origin.y + clip.size.height >= document - 0.5,
        })
    })
}

struct Ivars {
    key: u64,
    target: Retained<Target>,
    syncing: Cell<bool>,
    shown: Cell<bool>,
    flashed: Cell<Option<Instant>>,
    phase: Cell<Phase>,
    /// The thumb is being dragged: the list follows the scroller, not the other way round.
    live: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSScrollView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FFListScrollBars"]
    #[ivars = Ivars]
    struct ListBars;

    impl ListBars {
        /// NativeGPUI's action target finds the handler by the sender's tag.
        #[unsafe(method(tag))]
        fn tag(&self) -> isize {
            self.ivars().key as isize
        }

        /// Only the scroller takes the mouse; the rows underneath are GPUI's.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, at: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: at] };
            let Some(view) = (unsafe { hit.as_ref() }) else {
                return hit;
            };
            let on_the_bar = self.verticalScroller().is_some_and(|bar| view.isDescendantOf(&bar));
            if on_the_bar { hit } else { std::ptr::null_mut() }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            if let Some(next) = unsafe { self.nextResponder() } {
                next.scrollWheel(event);
            }
        }

        #[unsafe(method(ffClipMoved))]
        fn clip_moved(&self) {
            if !self.ivars().syncing.get() {
                self.report(Phase::Moved);
            }
        }

        #[unsafe(method(ffLiveStarted))]
        fn live_started(&self) {
            self.ivars().live.set(true);
            self.report(Phase::Started);
        }

        #[unsafe(method(ffLiveEnded))]
        fn live_ended(&self) {
            self.ivars().live.set(false);
            self.report(Phase::Ended);
        }
    }
);

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FFListScrollDocument"]
    struct Document;

    impl Document {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl ListBars {
    fn new(mtm: MainThreadMarker, target: &Target, key: u64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            key,
            target: target.retain(),
            syncing: Cell::new(false),
            shown: Cell::new(false),
            flashed: Cell::new(None),
            phase: Cell::new(Phase::Moved),
            live: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setDrawsBackground(false);
        this.setAutohidesScrollers(true);
        this.setHasHorizontalScroller(false);
        let document: Retained<Document> = unsafe { msg_send![Document::alloc(mtm), initWithFrame: NSRect::ZERO] };
        this.setDocumentView(Some(&document));
        let clip = this.contentView();
        clip.setPostsBoundsChangedNotifications(true);
        let center = NSNotificationCenter::defaultCenter();
        unsafe {
            center.addObserver_selector_name_object(
                &this,
                sel!(ffClipMoved),
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip),
            );
            // Dragging the thumb: hold the list's height still meanwhile, as GPUI asks.
            center.addObserver_selector_name_object(
                &this,
                sel!(ffLiveStarted),
                Some(NSScrollViewWillStartLiveScrollNotification),
                Some(&this),
            );
            center.addObserver_selector_name_object(
                &this,
                sel!(ffLiveEnded),
                Some(NSScrollViewDidEndLiveScrollNotification),
                Some(&this),
            );
        }
        this
    }

    fn report(&self, phase: Phase) {
        self.ivars().phase.set(phase);
        let _: () = unsafe { msg_send![&*self.ivars().target, ngAction: self] };
        self.ivars().phase.set(Phase::Moved);
    }

    fn show(&self, content: NSSize, down: bool, offset: NSPoint) {
        // Everything here follows the list; none of it is the user scrolling. (A shorter document
        // also makes the clip view clamp its origin, which would read as a scroll.)
        self.ivars().syncing.set(true);
        self.setHasVerticalScroller(down);
        if let Some(document) = self.documentView() {
            if document.frame().size != content {
                document.setFrameSize(content);
            }
        }
        let clip = self.contentView();
        let moved = !self.ivars().live.get() && clip.bounds().origin != offset;
        if moved {
            clip.scrollToPoint(offset);
            self.reflectScrolledClipView(&clip);
        }
        self.ivars().syncing.set(false);
        if moved {
            // Scrolling with the wheel or the keyboard shows the overlay scroller, as in a table.
            // Each flash invalidates the window's cursor rects, so it is rate-limited.
            let due = self.ivars().flashed.get().is_none_or(|at| at.elapsed() >= FLASH_EVERY);
            if self.ivars().shown.get() && due {
                self.flashScrollers();
                self.ivars().flashed.set(Some(Instant::now()));
            }
        }
        self.ivars().shown.set(true);
    }
}
