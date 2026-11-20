//! Colors for light and dark mode.

use crate::geom::{Color, rgb};

/// The colors widgets are drawn with.
///
/// kargui starts in the mode the operating system uses (Settings > Personalization > Colors)
/// and an app can switch with [`Ui::set_dark`](crate::Ui::set_dark).
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Window background.
    pub background: Color,
    /// Normal text.
    pub text: Color,
    /// Less important text (hints, counts).
    pub weak_text: Color,
    /// Text of finished or disabled items.
    pub done_text: Color,
    /// Borders of fields and buttons.
    pub border: Color,
    /// Lines between list rows.
    pub separator: Color,
    /// Main color: primary buttons, checked boxes, focus rings.
    pub accent: Color,
    /// Text on the main color.
    pub on_accent: Color,
    /// Background of buttons.
    pub button: Color,
    /// Background of buttons under the pointer.
    pub button_hover: Color,
    /// Background of buttons while pressed.
    pub button_pressed: Color,
    /// Background of text fields.
    pub field: Color,
    /// Background of selected text.
    pub selection: Color,
    /// Background of the selected part of a segmented control.
    pub selected: Color,
}

impl Theme {
    /// The light palette.
    pub fn light() -> Self {
        Self {
            background: rgb(0xffffff),
            text: rgb(0x1f2328),
            weak_text: rgb(0x59636e),
            done_text: rgb(0x8c959f),
            border: rgb(0xc8d1da),
            separator: rgb(0xeef1f4),
            accent: rgb(0x0969da),
            on_accent: rgb(0xffffff),
            button: rgb(0xf6f8fa),
            button_hover: rgb(0xeef1f4),
            button_pressed: rgb(0xe1e6eb),
            field: rgb(0xffffff),
            selection: rgb(0xb6d7ff),
            selected: rgb(0xddeeff),
        }
    }

    /// The dark palette.
    pub fn dark() -> Self {
        Self {
            background: rgb(0x16191d),
            text: rgb(0xe6e8eb),
            weak_text: rgb(0x9aa4ae),
            done_text: rgb(0x6e7781),
            border: rgb(0x3d444d),
            separator: rgb(0x262b31),
            accent: rgb(0x4493f8),
            on_accent: rgb(0x0d1117),
            button: rgb(0x21262d),
            button_hover: rgb(0x2a3038),
            button_pressed: rgb(0x343b44),
            field: rgb(0x0d1117),
            selection: rgb(0x1f4a7a),
            selected: rgb(0x1c3352),
        }
    }
}
