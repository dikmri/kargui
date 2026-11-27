//! macOS のキーの番号（NSEvent の keyCode。配列に左右されない、キーの場所の番号）と修飾キーを、kargui のキーに直す。
//! 計算だけなので、どの OS の上でも試験する（macos.rs が使う）。

use crate::input::{Key, Mods};

/// keyCode → kargui のキー（Carbon の kVK_ の番号。US の配列の場所で決まる）
pub(crate) fn key_of(code: u16) -> Option<Key> {
    Some(match code {
        0 => Key::A,
        1 => Key::S,
        2 => Key::D,
        3 => Key::F,
        4 => Key::H,
        5 => Key::G,
        6 => Key::Z,
        7 => Key::X,
        8 => Key::C,
        9 => Key::V,
        11 => Key::B,
        12 => Key::Q,
        13 => Key::W,
        14 => Key::E,
        15 => Key::R,
        16 => Key::Y,
        17 => Key::T,
        18 => Key::Num1,
        19 => Key::Num2,
        20 => Key::Num3,
        21 => Key::Num4,
        22 => Key::Num6,
        23 => Key::Num5,
        25 => Key::Num9,
        26 => Key::Num7,
        28 => Key::Num8,
        29 => Key::Num0,
        31 => Key::O,
        32 => Key::U,
        34 => Key::I,
        35 => Key::P,
        37 => Key::L,
        38 => Key::J,
        40 => Key::K,
        45 => Key::N,
        46 => Key::M,
        // Return とテンキーの Enter
        36 | 76 => Key::Enter,
        48 => Key::Tab,
        49 => Key::Space,
        // Mac の delete（左を消す）は Backspace、forward delete（fn+delete）は Delete
        51 => Key::Backspace,
        117 => Key::Delete,
        53 => Key::Escape,
        123 => Key::Left,
        124 => Key::Right,
        125 => Key::Down,
        126 => Key::Up,
        115 => Key::Home,
        119 => Key::End,
        116 => Key::PageUp,
        121 => Key::PageDown,
        // help（古いキーボードの Insert の場所）
        114 => Key::Insert,
        122 => Key::F1,
        120 => Key::F2,
        99 => Key::F3,
        118 => Key::F4,
        96 => Key::F5,
        97 => Key::F6,
        98 => Key::F7,
        100 => Key::F8,
        101 => Key::F9,
        109 => Key::F10,
        103 => Key::F11,
        111 => Key::F12,
        // テンキーの数字
        82 => Key::Num0,
        83 => Key::Num1,
        84 => Key::Num2,
        85 => Key::Num3,
        86 => Key::Num4,
        87 => Key::Num5,
        88 => Key::Num6,
        89 => Key::Num7,
        91 => Key::Num8,
        92 => Key::Num9,
        _ => return None,
    })
}

/// NSEventModifierFlags のビット（Shift 1<<17・Control 1<<18・Option 1<<19・Command 1<<20）→ kargui の修飾キー。
/// Mac の近道は Command で押す（Command+C で写す）ので、Command は Ctrl として渡す（Control も Ctrl）。Option は Alt
pub(crate) fn mods_of(flags: usize) -> Mods {
    Mods { shift: flags & (1 << 17) != 0, ctrl: flags & ((1 << 18) | (1 << 20)) != 0, alt: flags & (1 << 19) != 0 }
}

/// キーの知らせの文字のうち、入力欄に入れる物（制御文字と、矢印や F キーが持つ Unicode の私用の文字を外す）
pub(crate) fn typed(c: char) -> bool {
    !c.is_control() && !('\u{F700}'..='\u{F8FF}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_codes_follow_the_us_layout_positions() {
        assert_eq!(key_of(0), Some(Key::A));
        assert_eq!(key_of(8), Some(Key::C));
        assert_eq!(key_of(9), Some(Key::V));
        assert_eq!(key_of(6), Some(Key::Z));
        assert_eq!(key_of(29), Some(Key::Num0));
        assert_eq!(key_of(18), Some(Key::Num1));
        assert_eq!((key_of(36), key_of(76)), (Some(Key::Enter), Some(Key::Enter)), "Return と テンキーの Enter");
        assert_eq!((key_of(51), key_of(117)), (Some(Key::Backspace), Some(Key::Delete)), "delete と forward delete");
        assert_eq!((key_of(123), key_of(124), key_of(125), key_of(126)), (Some(Key::Left), Some(Key::Right), Some(Key::Down), Some(Key::Up)));
        assert_eq!((key_of(122), key_of(111)), (Some(Key::F1), Some(Key::F12)));
        assert_eq!(key_of(57), None, "Caps Lock は渡さない");
    }

    #[test]
    fn command_is_the_shortcut_key() {
        assert_eq!(mods_of(1 << 20), Mods { shift: false, ctrl: true, alt: false }, "Command は Ctrl");
        assert_eq!(mods_of(1 << 18), Mods { shift: false, ctrl: true, alt: false }, "Control も Ctrl");
        assert_eq!(mods_of((1 << 17) | (1 << 19)), Mods { shift: true, ctrl: false, alt: true }, "Shift と Option");
        assert_eq!(mods_of(0), Mods::NONE);
    }

    #[test]
    fn typed_text_skips_control_and_function_key_characters() {
        assert!(typed('a') && typed('あ') && typed(' '));
        assert!(!typed('\u{8}') && !typed('\r') && !typed('\u{1b}'));
        // 矢印（NSLeftArrowFunctionKey）と F1
        assert!(!typed('\u{F702}') && !typed('\u{F704}'));
    }
}
