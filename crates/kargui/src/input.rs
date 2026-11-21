//! Keyboard keys and the input events kargui handles.
//!
//! 窓（platform.rs）と試験台（test.rs）は、OS の入力をこの出来事に直して渡す。

use crate::ui::Id;

/// A key that widgets react to. Letters are only listed when they are used as shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// The A key (select all with Ctrl).
    A,
    /// The C key (copy with Ctrl).
    C,
    /// The V key (paste with Ctrl).
    V,
    /// The X key (cut with Ctrl).
    X,
    /// The Y key (redo with Ctrl).
    Y,
    /// The Z key (undo with Ctrl).
    Z,
}

/// Modifier keys held during a key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// Shift.
    pub shift: bool,
    /// Ctrl.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
}

/// 読み上げソフトなどから届いた操作
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum A11yAction {
    Click,
    Focus,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    /// 位置は物理ピクセル
    PointerDown { x: f32, y: f32, shift: bool },
    PointerUp { x: f32, y: f32 },
    /// 押したまま動かした（スクロールの棒をつかんでいる間だけ届く）
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
