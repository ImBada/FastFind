//! `OptionsPopoverView` — a real `NSPopover` on the toolbar's gear item, with the same rows.

use crate::appkit_bridge::ActionTarget;
use crate::i18n::tr;
use crate::platform::ns_string;
use gpui::{App, AsyncWindowContext, Window};
use objc2::rc::Retained;
use objc2::{ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSBox, NSBoxType, NSButton, NSCellImagePosition, NSColor, NSControlSize, NSFont, NSImage,
    NSImageSymbolConfiguration, NSLayoutAttribute, NSLayoutConstraint, NSPopover, NSPopoverBehavior, NSStackView,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindow,
};
use objc2_foundation::{NSEdgeInsets, NSObjectProtocol};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsState {
    pub by_folder: bool,
    pub by_folder_double: bool,
    pub hide_tags: bool,
    pub original_filename: bool,
    pub convert_full_width: bool,
    pub indexing: bool,
    pub vjdeck_enabled: bool,
    pub vjdeck_numbers: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionEvent {
    ByFolder,
    ByFolderDouble,
    HideTags,
    OriginalFilename,
    ConvertFullWidth,
    VjdeckEnabled,
    VjdeckNumbers(String),
}

pub type OptionHandler = Rc<dyn Fn(OptionEvent, &mut Window, &mut App)>;

pub struct OptionsPopover {
    popover: Retained<NSPopover>,
    _target: Retained<ActionTarget>,
    stack: Retained<NSStackView>,
    by_folder: Retained<NSButton>,
    by_folder_double: Retained<NSButton>,
    hide_tags: Retained<NSButton>,
    original_filename: Retained<NSButton>,
    convert_full_width: Retained<NSButton>,
    vjdeck: Retained<NSButton>,
    deck_row: Retained<NSStackView>,
    deck_field: Retained<NSTextField>,
    min_width: Retained<NSLayoutConstraint>,
    last: std::cell::RefCell<Option<OptionsState>>,
}

fn symbol(name: &str, point_size: f64) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns_string(name), None)?;
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight_scale(
        point_size,
        0.0,
        objc2_app_kit::NSImageSymbolScale::Medium,
    );
    image.imageWithSymbolConfiguration(&config)
}

/// `CheckboxView`: an SF Symbol square plus the title, in a plain (borderless) button.
fn checkbox(mtm: MainThreadMarker, title: &str) -> Retained<NSButton> {
    let button = NSButton::new(mtm);
    button.setBordered(false);
    button.setTitle(&ns_string(&format!(" {title}")));
    button.setImagePosition(NSCellImagePosition::ImageLeading);
    button.setImageHugsTitle(true);
    button.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
    button.setContentTintColor(Some(&NSColor::labelColor()));
    button
}

fn set_checked(button: &NSButton, checked: bool) {
    let name = if checked { "checkmark.square.fill" } else { "square" };
    button.setImage(symbol(name, NSFont::systemFontSize()).as_deref());
}

fn separator(mtm: MainThreadMarker) -> Retained<NSBox> {
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    line
}

impl OptionsPopover {
    pub fn show(
        window: &NSWindow,
        toolbar_item_id: &str,
        state: &OptionsState,
        cx: AsyncWindowContext,
        on_event: OptionHandler,
    ) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = window
            .toolbar()?
            .items()
            .iter()
            .find(|item| item.itemIdentifier().to_string() == toolbar_item_id)?;
        let target = ActionTarget::new(mtm, cx);
        let event = |e: OptionEvent| {
            let on_event = on_event.clone();
            move |window: &mut Window, cx: &mut App| on_event(e.clone(), window, cx)
        };

        let stack = NSStackView::new(mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(8.0);
        stack.setEdgeInsets(NSEdgeInsets {
            top: 10.0,
            left: 10.0,
            bottom: 10.0,
            right: 10.0,
        });

        let by_folder = checkbox(mtm, tr("Show all files in matching folders"));
        target.on_action(&by_folder, event(OptionEvent::ByFolder));
        let by_folder_double = checkbox(mtm, tr("Deep folder search"));
        target.on_action(&by_folder_double, event(OptionEvent::ByFolderDouble));
        let hide_tags = checkbox(mtm, tr("Hide folder-info tags under file names"));
        target.on_action(&hide_tags, event(OptionEvent::HideTags));
        let original_filename = checkbox(mtm, tr("Original Filename"));
        target.on_action(&original_filename, event(OptionEvent::OriginalFilename));
        let convert_full_width = checkbox(mtm, tr("Convert Full-width characters to ascii"));
        target.on_action(&convert_full_width, event(OptionEvent::ConvertFullWidth));
        let vjdeck = checkbox(mtm, "Import to VJDeck");
        target.on_action(&vjdeck, event(OptionEvent::VjdeckEnabled));

        let deck_label = NSTextField::labelWithString(&ns_string("Deck numbers"), mtm);
        deck_label.setFont(Some(&NSFont::systemFontOfSize(10.0)));
        deck_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let deck_field = NSTextField::new(mtm);
        deck_field.setControlSize(NSControlSize::Small);
        deck_field.setFont(Some(&NSFont::systemFontOfSize(10.0)));
        deck_field.setPlaceholderString(Some(&ns_string("e.g. 1,2")));
        deck_field.setStringValue(&ns_string(&state.vjdeck_numbers));
        deck_field.widthAnchor().constraintEqualToConstant(80.0).setActive(true);
        {
            let on_event = on_event.clone();
            target.on_text_change(&deck_field, move |text, window, cx| {
                on_event(OptionEvent::VjdeckNumbers(text), window, cx)
            });
        }
        let deck_row = NSStackView::new(mtm);
        deck_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        deck_row.setSpacing(8.0);
        deck_row.setEdgeInsets(NSEdgeInsets {
            top: 0.0,
            left: 20.0,
            bottom: 0.0,
            right: 0.0,
        });
        deck_row.addArrangedSubview(&deck_label);
        deck_row.addArrangedSubview(&deck_field);

        let vj_separator = separator(mtm);
        let views: [&NSView; 8] = [
            &by_folder,
            &by_folder_double,
            &hide_tags,
            &original_filename,
            &convert_full_width,
            &vj_separator,
            &vjdeck,
            &deck_row,
        ];
        for view in views {
            stack.addArrangedSubview(view);
        }
        vj_separator
            .widthAnchor()
            .constraintEqualToAnchor_constant(&stack.widthAnchor(), -20.0)
            .setActive(true);

        let min_width = stack.widthAnchor().constraintGreaterThanOrEqualToConstant(0.0);
        min_width.setActive(true);

        let controller = NSViewController::new(mtm);
        controller.setView(&stack);
        let popover = NSPopover::new(mtm);
        popover.setContentViewController(Some(&controller));
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*target)));
        popover.setAnimates(true);

        let this = OptionsPopover {
            popover,
            _target: target,
            stack,
            by_folder,
            by_folder_double,
            hide_tags,
            original_filename,
            convert_full_width,
            vjdeck,
            deck_row,
            deck_field,
            min_width,
            last: Default::default(),
        };
        this.sync(state);
        this.popover.showRelativeToToolbarItem(&item);
        Some(this)
    }

    /// Mirror `state` into the controls (rows appear/disappear like the SwiftUI `if`s).
    pub fn sync(&self, state: &OptionsState) {
        if self.last.borrow().as_ref() == Some(state) {
            return;
        }
        set_checked(&self.by_folder, state.by_folder);
        set_checked(&self.by_folder_double, state.by_folder_double);
        self.by_folder_double.setHidden(!state.by_folder);
        set_checked(&self.hide_tags, state.hide_tags);
        set_checked(&self.original_filename, state.original_filename);
        set_checked(&self.convert_full_width, state.convert_full_width);
        self.convert_full_width.setEnabled(!state.indexing);
        set_checked(&self.vjdeck, state.vjdeck_enabled);
        self.deck_row.setHidden(!state.vjdeck_enabled);
        if self.deck_field.currentEditor().is_none()
            && self.deck_field.stringValue().to_string() != state.vjdeck_numbers
        {
            self.deck_field.setStringValue(&ns_string(&state.vjdeck_numbers));
        }
        // NSStackView's fitting width comes out one edge inset short with these intrinsic-width rows,
        // pushing the widest row off its inset; pin a minimum width from the widest visible row.
        let insets = self.stack.edgeInsets();
        let widest = self
            .stack
            .arrangedSubviews()
            .iter()
            .filter(|view| !view.isHidden() && !view.isKindOfClass(NSBox::class()))
            .map(|view| view.fittingSize().width)
            .fold(0.0_f64, f64::max);
        self.min_width.setConstant(widest + insets.left + insets.right);
        self.stack.layoutSubtreeIfNeeded();
        self.popover.setContentSize(self.stack.fittingSize());
        *self.last.borrow_mut() = Some(state.clone());
    }

    pub fn is_shown(&self) -> bool {
        self.popover.isShown()
    }

    pub fn close(&self) {
        self.popover.close();
    }

    /// The gear's click that lands right after a transient close must not reopen it.
    pub fn closed_recently(&self) -> bool {
        self._target.closed_within(std::time::Duration::from_millis(350))
    }
}
