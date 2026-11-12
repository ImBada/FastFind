//! Target/action and text-delegate object routing AppKit controls (in the options popover) back
//! into GPUI, deferred so no handler ever runs under an AppKit callback's stack.

use gpui::{App, AsyncWindowContext, Window};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSControl, NSControlTextEditingDelegate, NSPopoverDelegate, NSTextField, NSTextFieldDelegate};
use objc2_foundation::{NSNotification, NSObject};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
type TextHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct TargetIvars {
    handlers: RefCell<Vec<Handler>>,
    text_handlers: RefCell<Vec<TextHandler>>,
    closed_at: Cell<Option<Instant>>,
    cx: AsyncWindowContext,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FFActionTarget"]
    #[ivars = TargetIvars]
    pub struct ActionTarget;

    unsafe impl NSObjectProtocol for ActionTarget {}

    unsafe impl NSControlTextEditingDelegate for ActionTarget {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let Some(field) = notification.object().and_then(|o| o.downcast::<NSTextField>().ok()) else {
                return;
            };
            let Some(handler) = usize::try_from(field.tag())
                .ok()
                .and_then(|tag| self.ivars().text_handlers.borrow().get(tag).cloned())
            else {
                return;
            };
            let text = field.stringValue().to_string();
            self.defer(move |window, cx| handler(text, window, cx));
        }
    }

    unsafe impl NSTextFieldDelegate for ActionTarget {}

    unsafe impl NSPopoverDelegate for ActionTarget {
        #[unsafe(method(popoverWillClose:))]
        fn popover_will_close(&self, _: &NSNotification) {
            self.ivars().closed_at.set(Some(Instant::now()));
        }
    }

    impl ActionTarget {
        #[unsafe(method(ffAction:))]
        fn ff_action(&self, sender: *mut AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            let Some(handler) = usize::try_from(tag)
                .ok()
                .and_then(|tag| self.ivars().handlers.borrow().get(tag).cloned())
            else {
                return;
            };
            self.defer(move |window, cx| handler(window, cx));
        }
    }
);

impl ActionTarget {
    pub fn new(mtm: MainThreadMarker, cx: AsyncWindowContext) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars {
            handlers: RefCell::new(Vec::new()),
            text_handlers: RefCell::new(Vec::new()),
            closed_at: Cell::new(None),
            cx,
        });
        unsafe { msg_send![super(this), init] }
    }

    fn defer(&self, f: impl FnOnce(&mut Window, &mut App) + 'static) {
        self.ivars()
            .cx
            .spawn(async move |cx| {
                let _ = cx.update(|window, app| f(window, app));
            })
            .detach();
    }

    /// True right after a popover this object delegates for began closing.
    pub fn closed_within(&self, window: std::time::Duration) -> bool {
        self.ivars().closed_at.get().is_some_and(|at| at.elapsed() < window)
    }

    /// Takes over `control`'s tag, target and action.
    pub fn on_action(&self, control: &NSControl, handler: impl Fn(&mut Window, &mut App) + 'static) {
        let tag = {
            let mut handlers = self.ivars().handlers.borrow_mut();
            handlers.push(Rc::new(handler));
            handlers.len() - 1
        };
        control.setTag(tag as isize);
        unsafe {
            control.setTarget(Some(self.as_ref()));
            control.setAction(Some(sel!(ffAction:)));
        }
    }

    /// Calls `handler` on every edit of `field` (`controlTextDidChange:`), like a SwiftUI binding.
    pub fn on_text_change(&self, field: &NSTextField, handler: impl Fn(String, &mut Window, &mut App) + 'static) {
        let tag = {
            let mut handlers = self.ivars().text_handlers.borrow_mut();
            handlers.push(Rc::new(handler));
            handlers.len() - 1
        };
        field.setTag(tag as isize);
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
    }
}
