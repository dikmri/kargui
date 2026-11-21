//! kargui: a light native GUI framework for Rust (work in progress).
//!
//! - Write the screen as plain code every frame ([`App::view`]); keep state in your own struct.
//! - Draws with the CPU by default and only what changed, so it starts fast and uses little memory.
//! - Japanese and other input methods work as users expect: composing text is shown in the field,
//!   and the Enter that confirms a conversion does not submit.
//! - Every widget appears to screen readers with a role and a name.
//!
//! Windows 10 and 11 only for now.
//!
//! In the repository, `docs/widgets.md` shows every widget in light and dark mode and
//! `docs/llms.txt` is an index for AI assistants. Both are generated from the source.
//!
//! 説明の決まり: 公開項目に説明が無いと組み立てが落ちる（deny(missing_docs)）。
//! 部品を作る関数には動く例が要る（cargo run -p xtask -- doc-check）。
#![deny(missing_docs)]

mod app;
mod cpu_frame;
mod damage;
mod gdi;
mod geom;
mod gpu;
mod ime;
mod input;
pub mod log;
mod paint;
mod platform;
mod raster;
mod runtime;
pub mod test;
mod text;
mod theme;
pub mod ui;

pub use app::{App, Frame, Options, Renderer, run};
pub use geom::{Color, IRect, Rect, rgb};
pub use input::{Key, Mods};
pub use log::set_logger;
pub use theme::Theme;
pub use ui::{Align, Id, Size, SliderValue, Ui};

/// An error from the operating system while opening or drawing the window.
///
/// ```no_run
/// use kargui::{App, Options, Ui};
///
/// struct Hello;
///
/// impl App for Hello {
///     fn view(&mut self, ui: &mut Ui) {
///         ui.heading("Hello");
///     }
/// }
///
/// match kargui::run(Options::new("Hello"), Hello) {
///     Ok(()) => {}
///     Err(e) => eprintln!("could not open the window: {e}"),
/// }
/// ```
#[derive(Debug)]
pub struct Error(pub(crate) String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Error(format!("{e:?}"))
    }
}

/// ガイド（docs/guide/ の日本語と英語）と README（リポジトリの直下）のコードも、説明の試験（cargo test --doc）で動かす
#[cfg(doctest)]
mod guide {
    #[doc = include_str!("../../../docs/guide/ja/01-first-app.md")]
    struct Ja01FirstApp;
    #[doc = include_str!("../../../docs/guide/ja/02-widgets.md")]
    struct Ja02Widgets;
    #[doc = include_str!("../../../docs/guide/ja/03-layout.md")]
    struct Ja03Layout;
    #[doc = include_str!("../../../docs/guide/ja/04-state-and-ids.md")]
    struct Ja04StateAndIds;
    #[doc = include_str!("../../../docs/guide/ja/05-lists-and-tables.md")]
    struct Ja05ListsAndTables;
    #[doc = include_str!("../../../docs/guide/ja/06-scroll-and-text.md")]
    struct Ja06ScrollAndText;
    #[doc = include_str!("../../../docs/guide/ja/07-japanese-input.md")]
    struct Ja07JapaneseInput;
    #[doc = include_str!("../../../docs/guide/ja/08-accessibility.md")]
    struct Ja08Accessibility;
    #[doc = include_str!("../../../docs/guide/ja/09-testing.md")]
    struct Ja09Testing;
    #[doc = include_str!("../../../docs/guide/ja/README.md")]
    struct JaReadme;
    #[doc = include_str!("../../../docs/guide/en/01-first-app.md")]
    struct En01FirstApp;
    #[doc = include_str!("../../../docs/guide/en/02-widgets.md")]
    struct En02Widgets;
    #[doc = include_str!("../../../docs/guide/en/03-layout.md")]
    struct En03Layout;
    #[doc = include_str!("../../../docs/guide/en/04-state-and-ids.md")]
    struct En04StateAndIds;
    #[doc = include_str!("../../../docs/guide/en/05-lists-and-tables.md")]
    struct En05ListsAndTables;
    #[doc = include_str!("../../../docs/guide/en/06-scroll-and-text.md")]
    struct En06ScrollAndText;
    #[doc = include_str!("../../../docs/guide/en/07-japanese-input.md")]
    struct En07JapaneseInput;
    #[doc = include_str!("../../../docs/guide/en/08-accessibility.md")]
    struct En08Accessibility;
    #[doc = include_str!("../../../docs/guide/en/09-testing.md")]
    struct En09Testing;
    #[doc = include_str!("../../../docs/guide/en/README.md")]
    struct EnReadme;
    #[doc = include_str!("../../../README.md")]
    struct RootReadme;
    #[doc = include_str!("../../../README.ja.md")]
    struct RootReadmeJa;
}
