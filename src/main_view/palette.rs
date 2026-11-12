//! System colours the views use, cached until the appearance changes.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub(super) label: Hsla,
    pub(super) secondary: Hsla,
    pub(super) window_bg: Hsla,
    pub(super) control_bg: Hsla,
    pub(super) separator: Hsla,
    pub(super) stripe: Hsla,
    pub(super) selection: Hsla,
    pub(super) selection_inactive: Hsla,
    pub(super) selected_text: Hsla,
    pub(super) yellow: Hsla,
    pub(super) indigo: Hsla,
    pub(super) red: Hsla,
    pub(super) mint: Hsla,
    pub(super) accent: Hsla,
    pub(super) gray: Hsla,
    pub(super) blue: Hsla,
    pub(super) green: Hsla,
    pub(super) cyan: Hsla,
}

thread_local! {
    static PALETTE: RefCell<Option<(u64, Palette)>> = const { RefCell::new(None) };
}

pub(super) fn palette() -> Palette {
    let generation = tokens::generation();
    PALETTE.with_borrow_mut(|cached| {
        if let Some((at, palette)) = cached.as_ref() {
            if *at == generation {
                return *palette;
            }
        }
        let c = tokens::system_color;
        let (_, stripe) = platform::alternating_row_colors();
        let palette = Palette {
            label: c(SystemColor::Label),
            secondary: c(SystemColor::SecondaryLabel),
            window_bg: c(SystemColor::WindowBackground),
            control_bg: c(SystemColor::ControlBackground),
            separator: c(SystemColor::Separator),
            stripe,
            selection: c(SystemColor::SelectedContentBackground),
            selection_inactive: c(SystemColor::UnemphasizedSelectedContentBackground),
            selected_text: c(SystemColor::AlternateSelectedControlText),
            yellow: c(SystemColor::SystemYellow),
            indigo: c(SystemColor::SystemIndigo),
            red: c(SystemColor::SystemRed),
            mint: c(SystemColor::SystemMint),
            accent: c(SystemColor::ControlAccent),
            gray: c(SystemColor::SystemGray),
            blue: c(SystemColor::SystemBlue),
            green: c(SystemColor::SystemGreen),
            cyan: c(SystemColor::SystemCyan),
        };
        *cached = Some((generation, palette));
        palette
    })
}
