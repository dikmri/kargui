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
