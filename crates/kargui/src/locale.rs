//! kargui が自分で出す文字の言語。題の帯のボタン・ドッキングのメニュー・数の入力欄のボタン・日付と色の部品など。
//!
//! 言語は OS の表示の言語（Windows は GetUserDefaultUILanguage、Linux と macOS は LANGUAGE・LC_ALL・LC_MESSAGES・
//! LANG）で決め、アプリは Options::locale で変えられる。試験台はいつも英語から始める（試験が PC の言語に左右されない）。

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::color::ColorText;
use crate::date::DateText;

/// The language of the words kargui shows itself: the names of the title bar's buttons, the
/// docking menu, the buttons of number fields, the date, time and color pickers, and the names
/// screen readers hear for them. kargui follows the system's display language, and an app can
/// choose one with [`Options::locale`](crate::Options::locale). Read it with
/// [`Ui::locale`](crate::Ui::locale) to show your own words in the same language.
///
/// ```
/// use kargui::Locale;
///
/// assert_eq!(Locale::from_tag("ja-JP"), Some(Locale::Japanese));
/// assert_eq!(Locale::from_tag("en_US.UTF-8"), Some(Locale::English));
/// assert_eq!(Locale::from_tag("fr"), None);
/// assert_eq!(Locale::Japanese.date_text().empty, "日付を選ぶ");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Locale {
    /// English. kargui uses it when the system's language is not one it has.
    #[default]
    English,
    /// Japanese.
    Japanese,
}

impl Locale {
    /// The language of a language tag (`ja`, `ja-JP`, `en_US.UTF-8`, `C`), or `None` for a
    /// language kargui does not have. The C and POSIX locales are English.
    ///
    /// ```
    /// use kargui::Locale;
    /// assert_eq!(Locale::from_tag("ja_JP.UTF-8"), Some(Locale::Japanese));
    /// assert_eq!(Locale::from_tag("C"), Some(Locale::English));
    /// assert_eq!(Locale::from_tag(""), None);
    /// ```
    pub fn from_tag(tag: &str) -> Option<Locale> {
        // 言語の部分は、最初の「-」「_」「.」「@」の前（ja_JP.UTF-8@x → ja）
        let lang = tag.trim().split(['-', '_', '.', '@']).next().unwrap_or("").to_ascii_lowercase();
        match lang.as_str() {
            "ja" => Some(Locale::Japanese),
            "en" | "c" | "posix" => Some(Locale::English),
            _ => None,
        }
    }

    /// The system's display language (read once and kept), or English when it is not one kargui
    /// has.
    ///
    /// ```
    /// let locale = kargui::Locale::system();
    /// # let _ = locale;
    /// ```
    pub fn system() -> Locale {
        system_reading().0
    }

    /// The words of the date and time pickers in this language (see
    /// [`Ui::set_date_text`](crate::Ui::set_date_text)).
    ///
    /// ```
    /// assert_eq!(kargui::Locale::English.date_text().months[0], "January");
    /// ```
    pub fn date_text(self) -> DateText {
        match self {
            Locale::English => DateText::english(),
            Locale::Japanese => DateText::japanese(),
        }
    }

    /// The words of the color picker in this language (see
    /// [`Ui::set_color_text`](crate::Ui::set_color_text)).
    ///
    /// ```
    /// assert_eq!(kargui::Locale::Japanese.color_text(), kargui::ColorText::japanese());
    /// ```
    pub fn color_text(self) -> ColorText {
        match self {
            Locale::English => ColorText::english(),
            Locale::Japanese => ColorText::japanese(),
        }
    }

    /// kargui が自分で出す、そのほかの文字
    pub(crate) fn words(self) -> &'static Words {
        match self {
            Locale::English => &ENGLISH,
            Locale::Japanese => &JAPANESE,
        }
    }
}

/// kargui が自分で出す文字（部品の名前は読み上げにも使う）
#[derive(Debug, PartialEq)]
pub(crate) struct Words {
    pub minimize: &'static str,
    pub maximize: &'static str,
    pub restore: &'static str,
    pub close: &'static str,
    /// 数の入力欄の上下のボタン
    pub increase: &'static str,
    pub decrease: &'static str,
    /// 仕切りのつまみ・表の列の幅のつまみ
    pub resize: &'static str,
    pub resize_column: &'static str,
    /// ツールバーの入りきらない物のメニュー
    pub more_options: &'static str,
    /// ドッキングのタブのメニュー
    pub dock: &'static str,
    pub float: &'static str,
    pub move_to_new_group: &'static str,
    pub move_into_group: &'static str,
    pub move_to_new_window: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub above: &'static str,
    pub below: &'static str,
    /// 「{名前} を閉じる」の形（閉じる・名前の順か）
    close_named_first: bool,
}

impl Words {
    /// パネルの閉じるボタンの名前（Close Editor / Editor を閉じる）
    pub(crate) fn close_named(&self, name: &str) -> String {
        if self.close_named_first { format!("{name} を{}", self.close) } else { format!("{} {name}", self.close) }
    }
}

static ENGLISH: Words = Words {
    minimize: "Minimize",
    maximize: "Maximize",
    restore: "Restore",
    close: "Close",
    increase: "Increase",
    decrease: "Decrease",
    resize: "Resize",
    resize_column: "Resize column",
    more_options: "More options",
    dock: "Dock",
    float: "Float",
    move_to_new_group: "Move to a new group",
    move_into_group: "Move into group",
    move_to_new_window: "Move to a new window",
    left: "Left",
    right: "Right",
    above: "Above",
    below: "Below",
    close_named_first: false,
};

/// 日本語の Windows と同じ言葉（題の帯のボタンは Windows の吹き出しと同じ）
static JAPANESE: Words = Words {
    minimize: "最小化",
    maximize: "最大化",
    restore: "元に戻す (縮小)",
    close: "閉じる",
    increase: "増やす",
    decrease: "減らす",
    resize: "大きさを変える",
    resize_column: "列の幅を変える",
    more_options: "その他のオプション",
    dock: "ドッキング",
    float: "フロート",
    move_to_new_group: "新しいグループへ移動",
    move_into_group: "グループへ移動",
    move_to_new_window: "新しいウィンドウへ移動",
    left: "左",
    right: "右",
    above: "上",
    below: "下",
    close_named_first: true,
};

/// アプリの Options::locale（run が起動の時に入れる。0 は OS に従う、1 は英語、2 は日本語）
static OPTION: AtomicU8 = AtomicU8::new(0);

pub(crate) fn set_option(v: Option<Locale>) {
    OPTION.store(
        match v {
            None => 0,
            Some(Locale::English) => 1,
            Some(Locale::Japanese) => 2,
        },
        Ordering::Relaxed,
    );
}

/// 窓が使う言語: アプリが選んだ物、無ければ OS の物
pub(crate) fn resolved() -> Locale {
    match OPTION.load(Ordering::Relaxed) {
        1 => Locale::English,
        2 => Locale::Japanese,
        _ => Locale::system(),
    }
}

/// 窓の処理が、作った窓に言語を入れて記録する（起動の時と、別の窓を開いた時）
pub(crate) fn apply_to(cx: &mut crate::ui::context::Context, window: &str) {
    let locale = resolved();
    cx.set_locale(locale);
    let (system, raw) = system_reading();
    crate::log::event("locale", &format!("window={window:?} chosen_by_app={} system={system:?} {raw}", chosen()), &format!("locale={locale:?}"));
}

/// アプリが選んだか（記録に使う）
pub(crate) fn chosen() -> bool {
    OPTION.load(Ordering::Relaxed) != 0
}

/// OS の言語と、読んだ値（記録に使う）。1 回だけ読む
pub(crate) fn system_reading() -> &'static (Locale, String) {
    static SYSTEM: OnceLock<(Locale, String)> = OnceLock::new();
    SYSTEM.get_or_init(read_system)
}

#[cfg(windows)]
fn read_system() -> (Locale, String) {
    // windows クレートでは Win32_Globalization の機能が要るので、この 1 つだけを自分で宣言する
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    let id = unsafe { GetUserDefaultUILanguage() };
    (from_langid(id), format!("langid={id:#06x}"))
}

#[cfg(not(windows))]
fn read_system() -> (Locale, String) {
    // macOS は好みの言語の並び（システム設定の言語と地域。Finder から開いたアプリには LANG などが無い）。
    // kargui が持つ言語が並びに無い時だけ、環境変数を読む
    #[cfg(target_os = "macos")]
    {
        let list = crate::macos::preferred_languages();
        if let Some(l) = from_preferred(&list) {
            return (l, format!("preferredLanguages={list:?}"));
        }
    }
    let get = |k: &str| std::env::var(k).ok();
    let seen = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| get(k).filter(|v| !v.is_empty()).map(|v| format!("{k}={v}"))).collect::<Vec<_>>().join(" ");
    (from_env(get), seen)
}

/// 好みの言語の並び（macOS の NSLocale.preferredLanguages。「ja-JP」「en-US」など）から、kargui が持つ最初の言語
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn from_preferred(list: &[String]) -> Option<Locale> {
    list.iter().find_map(|t| Locale::from_tag(t))
}

/// Windows の言語の番号（LANGID）から。下の 10 ビットが主な言語（0x11 日本語、0x09 英語）
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn from_langid(id: u16) -> Locale {
    match id & 0x3ff {
        0x11 => Locale::Japanese,
        _ => Locale::English,
    }
}

/// 環境変数から（gettext と同じ順）。LANGUAGE は「ja:en」のような好みの並びなので、kargui が持つ最初の言語を選ぶ。
/// 次に LC_ALL・LC_MESSAGES・LANG の、空でない最初の物で決める（持たない言語なら英語）
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn from_env(get: impl Fn(&str) -> Option<String>) -> Locale {
    if let Some(list) = get("LANGUAGE")
        && let Some(l) = list.split(':').find_map(Locale::from_tag)
    {
        return l;
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(v) = get(key).filter(|v| !v.is_empty()) {
            return Locale::from_tag(&v).unwrap_or(Locale::English);
        }
    }
    Locale::English
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_languages_pick_the_first_one_kargui_has() {
        let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(from_preferred(&list(&["ja-JP", "en-US"])), Some(Locale::Japanese));
        assert_eq!(from_preferred(&list(&["en-JP", "ja-JP"])), Some(Locale::English));
        // 持たない言語は飛ばす（Apple のアプリと同じ選び方）
        assert_eq!(from_preferred(&list(&["fr-FR", "ja-JP"])), Some(Locale::Japanese));
        assert_eq!(from_preferred(&list(&["fr-FR"])), None);
        assert_eq!(from_preferred(&[]), None);
    }

    #[test]
    fn tags_name_their_language() {
        for (tag, want) in [
            ("ja", Some(Locale::Japanese)),
            ("ja-JP", Some(Locale::Japanese)),
            ("ja_JP.UTF-8", Some(Locale::Japanese)),
            ("JA_jp", Some(Locale::Japanese)),
            ("en-US", Some(Locale::English)),
            ("en_GB.UTF-8@euro", Some(Locale::English)),
            ("C", Some(Locale::English)),
            ("C.UTF-8", Some(Locale::English)),
            ("POSIX", Some(Locale::English)),
            ("fr_FR.UTF-8", None),
            ("", None),
        ] {
            assert_eq!(Locale::from_tag(tag), want, "{tag:?}");
        }
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn the_environment_decides_in_gettexts_order() {
        assert_eq!(from_env(env(&[("LANG", "ja_JP.UTF-8")])), Locale::Japanese);
        assert_eq!(from_env(env(&[("LANG", "ja_JP.UTF-8"), ("LC_MESSAGES", "en_US.UTF-8")])), Locale::English, "LC_MESSAGES before LANG");
        assert_eq!(from_env(env(&[("LC_MESSAGES", "en_US.UTF-8"), ("LC_ALL", "ja_JP.UTF-8")])), Locale::Japanese, "LC_ALL before LC_MESSAGES");
        assert_eq!(from_env(env(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "fr:ja:en")])), Locale::Japanese, "the first language kargui has in LANGUAGE");
        assert_eq!(from_env(env(&[("LANG", "ja_JP.UTF-8"), ("LANGUAGE", "fr")])), Locale::Japanese, "LANGUAGE with none of ours falls back to LANG");
        assert_eq!(from_env(env(&[("LANG", "fr_FR.UTF-8")])), Locale::English, "a language kargui does not have");
        assert_eq!(from_env(env(&[("LANG", ""), ("LC_ALL", "")])), Locale::English, "empty values are skipped");
        assert_eq!(from_env(env(&[])), Locale::English);
    }

    #[test]
    fn windows_language_ids_use_their_primary_language() {
        assert_eq!(from_langid(0x0411), Locale::Japanese, "ja-JP");
        assert_eq!(from_langid(0x0409), Locale::English, "en-US");
        assert_eq!(from_langid(0x0809), Locale::English, "en-GB");
        assert_eq!(from_langid(0x040c), Locale::English, "fr-FR: not ours");
    }

    #[test]
    fn every_word_is_there_in_both_languages() {
        let (en, ja) = (Locale::English.words(), Locale::Japanese.words());
        let all = |w: &Words| {
            [w.minimize, w.maximize, w.restore, w.close, w.increase, w.decrease, w.resize, w.resize_column, w.more_options, w.dock, w.float, w.move_to_new_group, w.move_into_group, w.move_to_new_window, w.left, w.right, w.above, w.below]
        };
        for (a, b) in all(en).iter().zip(all(ja)) {
            assert!(!a.is_empty() && !b.is_empty() && *a != b, "{a:?} / {b:?}");
        }
        assert_eq!(en.close_named("Editor"), "Close Editor");
        assert_eq!(ja.close_named("エディター"), "エディター を閉じる");
    }

    #[test]
    fn the_app_choice_comes_before_the_system() {
        set_option(Some(Locale::Japanese));
        assert_eq!(resolved(), Locale::Japanese);
        assert!(chosen());
        set_option(Some(Locale::English));
        assert_eq!(resolved(), Locale::English);
        set_option(None);
        assert_eq!(resolved(), Locale::system());
        assert!(!chosen());
    }
}
