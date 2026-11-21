//! Keyboard keys and the input events kargui handles.
//!
//! 窓（platform.rs）と試験台（test.rs）は、OS の入力をこの出来事に直して渡す。

use crate::ui::Id;

/// A key on the keyboard. Widgets use some keys themselves (text fields take letters and
/// Ctrl+A/C/V/X/Y/Z, sliders take the arrows); the rest reach the app through [`Ui::shortcut`](crate::Ui::shortcut).
///
/// ```
/// # use kargui::{App, Ui, Key, Mods};
/// # use kargui::test::Harness;
/// # #[derive(Default)]
/// # struct Notes {
/// #     input: String,
/// #     notes: Vec<String>,
/// # }
/// # impl App for Notes {
/// #     fn view(&mut self, ui: &mut Ui) {
/// #         ui.row(|ui| {
/// #             let field = ui.text_input(&mut self.input).hint("New note").grow();
/// #             if field.submitted() || ui.button("Add").clicked() {
/// #                 self.notes.push(std::mem::take(&mut self.input));
/// #             }
/// #         });
/// #         for note in &self.notes {
/// #             ui.label(note);
/// #         }
/// #     }
/// # }
///
/// let mut h = Harness::new(Notes::default());
/// h.click("New note").type_text("milk").press(Key::Enter);
/// assert_eq!(h.app().notes, ["milk"]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// Enter / Return.
    Enter,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Space.
    Space,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// The A key (select all with Ctrl in text fields).
    A,
    /// The B key.
    B,
    /// The C key (copy with Ctrl in text fields).
    C,
    /// The D key.
    D,
    /// The E key.
    E,
    /// The F key.
    F,
    /// The G key.
    G,
    /// The H key.
    H,
    /// The I key.
    I,
    /// The J key.
    J,
    /// The K key.
    K,
    /// The L key.
    L,
    /// The M key.
    M,
    /// The N key.
    N,
    /// The O key.
    O,
    /// The P key.
    P,
    /// The Q key.
    Q,
    /// The R key.
    R,
    /// The S key.
    S,
    /// The T key.
    T,
    /// The U key.
    U,
    /// The V key (paste with Ctrl in text fields).
    V,
    /// The W key.
    W,
    /// The X key (cut with Ctrl in text fields).
    X,
    /// The Y key (redo with Ctrl in text fields).
    Y,
    /// The Z key (undo with Ctrl in text fields).
    Z,
    /// The 0 key on the main keyboard.
    Num0,
    /// The 1 key on the main keyboard.
    Num1,
    /// The 2 key on the main keyboard.
    Num2,
    /// The 3 key on the main keyboard.
    Num3,
    /// The 4 key on the main keyboard.
    Num4,
    /// The 5 key on the main keyboard.
    Num5,
    /// The 6 key on the main keyboard.
    Num6,
    /// The 7 key on the main keyboard.
    Num7,
    /// The 8 key on the main keyboard.
    Num8,
    /// The 9 key on the main keyboard.
    Num9,
    /// F1.
    F1,
    /// F2.
    F2,
    /// F3.
    F3,
    /// F4.
    F4,
    /// F5.
    F5,
    /// F6.
    F6,
    /// F7.
    F7,
    /// F8.
    F8,
    /// F9.
    F9,
    /// F10.
    F10,
    /// F11.
    F11,
    /// F12.
    F12,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert.
    Insert,
}

/// Modifier keys held during a key press.
///
/// ```
/// use kargui::{Key, Mods};
///
/// let ctrl = Mods { ctrl: true, ..Mods::default() };
/// assert!(ctrl.ctrl && !ctrl.shift && !ctrl.alt);
/// // Ctrl+Z, as sent to `Harness::press_with`.
/// let _ = (Key::Z, ctrl);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    /// Shift.
    pub shift: bool,
    /// Ctrl.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
}

impl Mods {
    /// No modifier keys.
    ///
    /// ```
    /// assert_eq!(kargui::Mods::NONE, kargui::Mods::default());
    /// ```
    pub const NONE: Mods = Mods { shift: false, ctrl: false, alt: false };
    /// Shift. Combine with `|`: `Mods::CTRL | Mods::SHIFT`.
    ///
    /// ```
    /// assert!(kargui::Mods::SHIFT.shift);
    /// ```
    pub const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };
    /// Ctrl. Combine with `|`: `Mods::CTRL | Mods::SHIFT`.
    ///
    /// ```
    /// use kargui::Mods;
    /// let m = Mods::CTRL | Mods::SHIFT;
    /// assert!(m.ctrl && m.shift && !m.alt);
    /// ```
    pub const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false };
    /// Alt. Combine with `|`: `Mods::ALT | Mods::SHIFT`.
    ///
    /// ```
    /// assert!(kargui::Mods::ALT.alt);
    /// ```
    pub const ALT: Mods = Mods { shift: false, ctrl: false, alt: true };
}

impl std::ops::BitOr for Mods {
    type Output = Mods;

    /// Both sets of modifier keys.
    ///
    /// ```
    /// use kargui::Mods;
    /// assert_eq!(Mods::CTRL | Mods::ALT, Mods { shift: false, ctrl: true, alt: true });
    /// ```
    fn bitor(self, other: Mods) -> Mods {
        Mods { shift: self.shift || other.shift, ctrl: self.ctrl || other.ctrl, alt: self.alt || other.alt }
    }
}

/// 読み上げソフトなどから届いた操作
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum A11yAction {
    Click,
    Focus,
    /// スライダーの値を 1 段上げる・下げる
    Increment,
    Decrement,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    /// 位置は物理ピクセル
    PointerDown { x: f32, y: f32, shift: bool },
    PointerUp { x: f32, y: f32 },
    /// 押したまま動かした（スクロールの棒をつかんでいる間と、部品を押している間だけ届く）
    PointerMove { x: f32, y: f32 },
    /// dy は論理ピクセル（下へ回すと正）
    Wheel { x: f32, y: f32, dy: f32 },
    Key { key: Key, mods: Mods },
    /// 確定した文字（日本語入力を使わない入力）
    Char(char),
    /// 変換中の文字（空なら変換が終わった）
    ImePreedit(String),
    /// 変換中の文字の文節と、今変換している文節（ImePreedit のすぐ後に届く）
    ImeClauses { bounds: Vec<usize>, target: Option<(usize, usize)>, cursor: Option<usize> },
    /// 確定した変換の結果
    ImeCommit(String),
    /// 再変換で IME が決めた範囲を選ぶ（入力欄の文字の UTF-8 の位置）
    ImeSelect { start: usize, end: usize },
    A11y { target: Id, action: A11yAction },
}
