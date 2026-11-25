//! Colors for light and dark mode.

use crate::geom::{Color, rgb};

/// The colors widgets are drawn with.
///
/// kargui starts in the mode the operating system uses (Settings > Personalization > Colors)
/// and an app can switch with [`Ui::set_dark`](crate::Ui::set_dark).
///
/// ```
/// use kargui::Theme;
///
/// let light = Theme::light();
/// let dark = Theme::dark();
/// assert_ne!(light.background, dark.background);
/// assert_eq!(light.accent[3], 1.0);
/// ```
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
    ///
    /// ```
    /// let theme = kargui::Theme::light();
    /// assert_eq!(theme.background, kargui::rgb(0xffffff));
    /// ```
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
    ///
    /// ```
    /// let theme = kargui::Theme::dark();
    /// assert_eq!(theme.background, kargui::rgb(0x16191d));
    /// ```
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

    /// The high contrast palette with a black background (white text, cyan highlight), as
    /// Windows' classic High Contrast Black. kargui uses it when the system asks for high
    /// contrast in dark mode and gives no colors of its own (Linux), or when
    /// [`Options::high_contrast`](crate::Options::high_contrast) forces it.
    ///
    /// ```
    /// let theme = kargui::Theme::high_contrast_dark();
    /// assert_eq!(theme.background, kargui::rgb(0x000000));
    /// assert_eq!(theme.text, kargui::rgb(0xffffff));
    /// ```
    pub fn high_contrast_dark() -> Self {
        Self::from_system(&SystemColors::BLACK)
    }

    /// The high contrast palette with a white background (black text, purple highlight), as
    /// Windows' classic High Contrast White.
    ///
    /// ```
    /// let theme = kargui::Theme::high_contrast_light();
    /// assert_eq!(theme.background, kargui::rgb(0xffffff));
    /// assert_eq!(theme.text, kargui::rgb(0x000000));
    /// ```
    pub fn high_contrast_light() -> Self {
        Self::from_system(&SystemColors::WHITE)
    }

    /// OS のハイコントラストの色から作る。文字はどれも WindowText（薄い文字を作らない）、縁と区切りも
    /// WindowText（見える線）、強調は Highlight。乗った・押した・選んだ時の背景は、ボタンの色と Highlight を混ぜて、
    /// 文字が読める濃さにする
    pub(crate) fn from_system(c: &SystemColors) -> Self {
        Self {
            background: c.window,
            text: c.window_text,
            weak_text: c.window_text,
            done_text: c.gray_text,
            border: c.button_text,
            separator: c.window_text,
            accent: c.highlight,
            on_accent: c.highlight_text,
            button: c.button_face,
            button_hover: mix(c.button_face, c.highlight, 0.25),
            button_pressed: mix(c.button_face, c.highlight, 0.4),
            field: c.window,
            selection: c.highlight,
            selected: mix(c.button_face, c.highlight, 0.35),
        }
    }
}

impl Theme {
    /// ハイコントラストで押せない部品を描く配色。縁・文字・強調をどれも GrayText（done_text）にし、塗りは背景の色に
    /// する（Windows のハイコントラストの決まり）。何度かけても同じ
    pub(crate) fn disabled_high_contrast(&self) -> Theme {
        let (gray, bg) = (self.done_text, self.background);
        Theme {
            text: gray,
            weak_text: gray,
            border: gray,
            separator: gray,
            accent: gray,
            on_accent: bg,
            button: bg,
            button_hover: bg,
            button_pressed: bg,
            field: bg,
            selected: bg,
            ..self.clone()
        }
    }
}

/// アプリの Options::high_contrast（run が起動の時に入れ、Context が作られる時に読む。0 は OS に従う、1 は切る、
/// 2 は強制）
static OPTION: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn set_option(v: Option<bool>) {
    OPTION.store(match v {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    }, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn option() -> Option<bool> {
    match OPTION.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// 明暗・ハイコントラスト・OS の色から配色を決める。OS がハイコントラストの色をくれる時（Windows）はそれを使い、
/// くれない時（Linux、アプリが強制した時）は明暗に合わせた kargui の配色を使う
pub(crate) fn theme_for(dark: bool, high_contrast: bool, system: Option<&SystemColors>) -> Theme {
    match (high_contrast, system) {
        (true, Some(c)) => Theme::from_system(c),
        (true, None) if dark => Theme::high_contrast_dark(),
        (true, None) => Theme::high_contrast_light(),
        (false, _) if dark => Theme::dark(),
        (false, _) => Theme::light(),
    }
}

/// OS のハイコントラストの色（Windows の GetSysColor の 8 色）
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SystemColors {
    pub window: Color,
    pub window_text: Color,
    pub highlight: Color,
    pub highlight_text: Color,
    pub button_face: Color,
    pub button_text: Color,
    pub gray_text: Color,
    /// リンクの色（今は使わない。kargui のリンクは強調の色で描く）
    #[allow(dead_code)]
    pub hotlight: Color,
}

impl SystemColors {
    /// Windows の古典の High Contrast Black
    pub(crate) const BLACK: SystemColors = SystemColors {
        window: hex(0x000000),
        window_text: hex(0xffffff),
        highlight: hex(0x1aebff),
        highlight_text: hex(0x000000),
        button_face: hex(0x000000),
        button_text: hex(0xffffff),
        gray_text: hex(0x3ff23f),
        hotlight: hex(0xffff00),
    };
    /// Windows の古典の High Contrast White
    pub(crate) const WHITE: SystemColors = SystemColors {
        window: hex(0xffffff),
        window_text: hex(0x000000),
        highlight: hex(0x37006e),
        highlight_text: hex(0xffffff),
        button_face: hex(0xffffff),
        button_text: hex(0x000000),
        gray_text: hex(0x600000),
        hotlight: hex(0x00009f),
    };
}

/// const で使える rgb（geom::rgb と同じ）
const fn hex(v: u32) -> Color {
    [((v >> 16) & 0xff) as f32 / 255.0, ((v >> 8) & 0xff) as f32 / 255.0, (v & 0xff) as f32 / 255.0, 1.0]
}

/// 2 色を混ぜる（t は b の割合）
pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// WCAG の相対輝度
#[cfg(test)]
fn luminance(c: Color) -> f32 {
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG の対比（1〜21）
#[cfg(test)]
pub(crate) fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_contrast_palettes_keep_every_text_readable() {
        for (name, t) in [("dark", Theme::high_contrast_dark()), ("light", Theme::high_contrast_light())] {
            // 文字は 7:1（WCAG の AAA）、強調の上の文字・押せない文字・押した時は 4.5:1、縁と強調の色は 3:1
            let need = |what: &str, a: Color, b: Color, min: f32| {
                let c = contrast(a, b);
                assert!(c >= min, "{name}: {what} {c:.2} < {min}");
            };
            need("text / background", t.text, t.background, 7.0);
            need("weak text / background", t.weak_text, t.background, 7.0);
            need("text / button", t.text, t.button, 7.0);
            need("text / hovered button", t.text, t.button_hover, 7.0);
            need("text / pressed button", t.text, t.button_pressed, 4.5);
            need("text / selected", t.text, t.selected, 4.5);
            need("text / field", t.text, t.field, 7.0);
            need("accent text / selected", t.accent, t.selected, 4.5);
            need("on accent / accent", t.on_accent, t.accent, 4.5);
            need("on accent / selection", t.on_accent, t.selection, 4.5);
            need("disabled text / background", t.done_text, t.background, 4.5);
            need("border / background", t.border, t.background, 3.0);
            need("accent / background", t.accent, t.background, 3.0);
        }
    }

    #[test]
    fn system_colors_become_the_theme() {
        let c = SystemColors::BLACK;
        let t = Theme::from_system(&c);
        assert_eq!((t.background, t.text, t.done_text), (c.window, c.window_text, c.gray_text));
        assert_eq!((t.accent, t.on_accent, t.selection), (c.highlight, c.highlight_text, c.highlight));
        assert_eq!((t.button, t.border, t.field), (c.button_face, c.button_text, c.window));
        // 乗った・押した・選んだ時は、ボタンの色と強調の色の間
        assert!(contrast(t.button_hover, t.button) > 1.1 && contrast(t.button_pressed, t.button) > contrast(t.button_hover, t.button));
        assert_eq!(Theme::high_contrast_dark(), Theme::from_system(&SystemColors::BLACK));
        assert_eq!(Theme::high_contrast_light(), Theme::from_system(&SystemColors::WHITE));
    }

    #[test]
    fn disabled_high_contrast_draws_everything_in_gray_text() {
        let t = Theme::high_contrast_light();
        let off = t.disabled_high_contrast();
        assert_eq!((off.text, off.border, off.accent), (t.done_text, t.done_text, t.done_text));
        assert_eq!((off.button, off.field, off.on_accent), (t.background, t.background, t.background));
        assert_eq!(off.disabled_high_contrast(), off, "the same however many times");
    }

    #[test]
    fn contrast_is_the_wcag_ratio() {
        assert!((contrast(rgb(0x000000), rgb(0xffffff)) - 21.0).abs() < 0.01);
        assert!((contrast(rgb(0x777777), rgb(0x777777)) - 1.0).abs() < 0.01);
        // #767676 は白の上で 4.54:1（よく知られた値）
        assert!((contrast(rgb(0x767676), rgb(0xffffff)) - 4.54).abs() < 0.02);
        assert_eq!(mix(rgb(0x000000), rgb(0xffffff), 0.0), rgb(0x000000));
        assert_eq!(mix(rgb(0x000000), rgb(0xffffff), 1.0), rgb(0xffffff));
    }
}
