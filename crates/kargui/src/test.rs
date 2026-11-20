//! Running an app without a window, for tests and documentation.
//!
//! [`Harness`] runs your [`App`] the same way the window does, but you send the input yourself
//! and look at the result. Widgets are found by the name a screen reader would read, so tests
//! also check that every control has a name.
//!
//! ```
//! use kargui::test::Harness;
//! use kargui::{App, Ui};
//!
//! #[derive(Default)]
//! struct Notes {
//!     input: String,
//!     notes: Vec<String>,
//! }
//!
//! impl App for Notes {
//!     fn view(&mut self, ui: &mut Ui) {
//!         ui.row(|ui| {
//!             let field = ui.text_input(&mut self.input).hint("New note").grow();
//!             if field.submitted() || ui.button("Add").clicked() {
//!                 self.notes.push(std::mem::take(&mut self.input));
//!             }
//!         });
//!         for note in &self.notes {
//!             ui.label(note);
//!         }
//!     }
//! }
//!
//! let mut h = Harness::new(Notes::default());
//! h.click("New note").type_text("milk").press(kargui::Key::Enter);
//! assert_eq!(h.app().notes, ["milk"]);
//! // The input method: composing does not change the text, committing does.
//! h.ime_compose("ぎゅうにゅう");
//! assert_eq!(h.app().input, "");
//! h.ime_commit("牛乳").press(kargui::Key::Enter);
//! assert_eq!(h.app().notes, ["milk", "牛乳"]);
//! ```

use crate::app::App;
use crate::geom::{IRect, Rect};
use crate::input::{Event, Key, Mods};
use crate::raster::Canvas;
use crate::runtime::Runtime;
use crate::text::Text;
use crate::ui::context::MemoryClipboard;
use crate::ui::{Ui, a11y};

/// Runs an [`App`] without a window. See the [module documentation](self).
pub struct Harness<A: App> {
    app: A,
    rt: Runtime,
}

impl<A: App> Harness<A> {
    /// A 900×640 screen in light mode at 100% scale, after the first frame.
    pub fn new(app: A) -> Self {
        Self::with_size(app, 900.0, 640.0, 1.0)
    }

    /// A screen of the given size (logical pixels) and scale, after the first frame.
    pub fn with_size(app: A, width: f32, height: f32, scale: f32) -> Self {
        let text = Text::new().expect("DirectWrite is available on Windows");
        let mut rt = Runtime::new(text, false, Box::new(MemoryClipboard::default()), "test".into());
        rt.resize((width * scale).round(), (height * scale).round(), scale);
        let mut h = Self { app, rt };
        h.frame();
        h
    }

    /// The app.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The app, to change its state between frames.
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Runs a frame without input.
    pub fn frame(&mut self) -> &mut Self {
        self.send(Vec::new())
    }

    fn send(&mut self, events: Vec<Event>) -> &mut Self {
        let out = self.rt.frame(&mut self.app, events);
        // 一覧の高さが決まった直後など、もう 1 フレーム要るときは続けて描く（アニメーションは除く）
        if out.want_frame && self.rt.frames < 1_000_000 {
            self.rt.frame(&mut self.app, Vec::new());
        }
        self
    }

    /// The names a screen reader would read, in screen order.
    pub fn names(&self) -> Vec<String> {
        (0..self.rt.tree.info.len()).filter_map(|i| a11y::name_of(&self.rt.tree, i)).collect()
    }

    /// Where the widget with this name is (physical pixels), if it is on screen.
    pub fn find(&self, name: &str) -> Option<Rect> {
        (0..self.rt.tree.info.len()).find(|&i| a11y::name_of(&self.rt.tree, i).as_deref() == Some(name)).map(|i| self.rt.tree.layout[i].rect)
    }

    fn center(&self, name: &str) -> (f32, f32) {
        let r = self.find(name).unwrap_or_else(|| panic!("no widget named {name:?}; on screen: {:?}", self.names()));
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// Clicks the widget with this name (and focuses it if it can take focus).
    pub fn click(&mut self, name: &str) -> &mut Self {
        let (x, y) = self.center(name);
        self.send(vec![Event::PointerDown { x, y, shift: false }, Event::PointerUp { x, y }])
    }

    /// Types text into the focused field, as if typed without an input method.
    pub fn type_text(&mut self, text: &str) -> &mut Self {
        self.send(text.chars().map(Event::Char).collect())
    }

    /// Presses a key.
    pub fn press(&mut self, key: Key) -> &mut Self {
        self.press_with(key, Mods::default())
    }

    /// Presses a key with modifiers (for example Ctrl+Z).
    pub fn press_with(&mut self, key: Key, mods: Mods) -> &mut Self {
        self.send(vec![Event::Key { key, mods }])
    }

    /// The input method shows `text` as being composed (not yet converted or committed).
    pub fn ime_compose(&mut self, text: &str) -> &mut Self {
        self.send(vec![Event::ImePreedit(text.to_string())])
    }

    /// The input method commits `text` (the user pressed Enter to accept the conversion).
    /// This Enter is handled by the input method and does not reach the app.
    pub fn ime_commit(&mut self, text: &str) -> &mut Self {
        self.send(vec![Event::ImePreedit(String::new()), Event::ImeCommit(text.to_string())])
    }

    /// Scrolls the list under the widget with this name by `dy` logical pixels (down is positive).
    pub fn scroll(&mut self, name: &str, dy: f32) -> &mut Self {
        let (x, y) = self.center(name);
        self.send(vec![Event::Wheel { x, y, dy }])
    }

    /// Draws the screen with the CPU renderer.
    pub fn render(&mut self) -> Image {
        let (w, h) = (self.rt.cx.width as i32, self.rt.cx.height as i32);
        let mut pixels = vec![0u32; (w * h) as usize];
        let bg = self.rt.cx.theme.background;
        let atlas = &self.rt.cx.text.atlas;
        Canvas { px: &mut pixels, width: w, height: h }.draw(&self.rt.draw, &atlas.pixels, atlas.size, IRect::new(0, 0, w, h), bg);
        Image { width: w as u32, height: h as u32, pixels }
    }
}

/// An image drawn by [`Harness::render`]: pixels as `0xAARRGGBB`, row by row from the top.
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels.
    pub pixels: Vec<u32>,
}

impl Image {
    /// The pixel at `(x, y)` as `0xRRGGBB`.
    pub fn rgb(&self, x: u32, y: u32) -> u32 {
        self.pixels[(y * self.width + x) as usize] & 0x00ff_ffff
    }

    /// Saves as PNG (uncompressed, for screenshots in documentation and tests).
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, png(self))
    }
}

/// 圧縮しない PNG を作る（依存を増やさないため、zlib の「そのまま入れる」ブロックで書く）
fn png(img: &Image) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((img.width * img.height * 3 + img.height) as usize);
    for y in 0..img.height {
        raw.push(0);
        for x in 0..img.width {
            let p = img.pixels[(y * img.width + x) as usize];
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (k, block) in blocks.iter().enumerate() {
        z.push(if k + 1 == blocks.len() { 1 } else { 0 });
        let n = block.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// 説明の中の例で使う、閉じた画面（関数の中身を 1 回だけ動かす）
struct Once<F: FnMut(&mut Ui)>(F);

impl<F: FnMut(&mut Ui) + 'static> App for Once<F> {
    fn view(&mut self, ui: &mut Ui) {
        (self.0)(ui)
    }
}

/// Runs `f` once against a throwaway screen. Used by the examples in this documentation.
#[doc(hidden)]
pub fn example(f: impl FnOnce(&mut Ui)) {
    let text = Text::new().expect("DirectWrite is available on Windows");
    let mut rt = Runtime::new(text, false, Box::new(MemoryClipboard::default()), "example".into());
    rt.resize(900.0, 640.0, 1.0);
    let mut tree = crate::ui::Tree::new(crate::runtime::ROOT, 1.0);
    f(&mut Ui::new(&mut rt.cx, &mut tree));
    crate::ui::draw::layout(&mut tree, &mut rt.cx);
    crate::ui::draw::paint(&tree, &mut rt.cx, &mut rt.draw);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Size;

    struct Book {
        input: String,
        items: Vec<(String, bool)>,
        dark: bool,
        filter: u8,
    }

    impl App for Book {
        fn view(&mut self, ui: &mut Ui) {
            ui.set_dark(self.dark);
            ui.row(|ui| {
                ui.heading("Book");
                ui.fill();
                ui.toggle(&mut self.dark, "Dark");
            });
            ui.row(|ui| {
                let field = ui.text_input(&mut self.input).hint("New item").grow();
                if field.submitted() || ui.button("Add").primary().clicked() {
                    if !self.input.trim().is_empty() {
                        self.items.insert(0, (std::mem::take(&mut self.input), false));
                    }
                }
            });
            ui.segmented(&mut self.filter, &[(0, "All"), (1, "Open")]);
            let shown: Vec<usize> = (0..self.items.len()).filter(|&i| self.filter == 0 || !self.items[i].1).collect();
            let mut remove = None;
            let items = &mut self.items;
            ui.list(shown.len(), 32.0).grow().show(|ui, row| {
                let i = shown[row];
                let (text, done) = &mut items[i];
                let name = text.clone();
                ui.checkbox(done, name.clone());
                ui.fill();
                if ui.button("Delete").small().label(format!("Delete {name}")).clicked() {
                    remove = Some(i);
                }
            });
            if let Some(i) = remove {
                self.items.remove(i);
            }
        }
    }

    fn book(n: usize) -> Harness<Book> {
        let items = (0..n).map(|i| (format!("Item {i}"), i % 3 == 2)).collect();
        Harness::new(Book { input: String::new(), items, dark: false, filter: 0 })
    }

    #[test]
    fn typing_enter_and_ime_add_items() {
        let mut h = book(0);
        h.click("New item").type_text("milk").press(Key::Enter);
        assert_eq!(h.app().items[0].0, "milk");
        h.ime_compose("ぎゅうにゅう");
        assert_eq!(h.app().input, "");
        h.ime_commit("牛乳");
        assert_eq!(h.app().input, "牛乳");
        h.press(Key::Enter);
        assert_eq!(h.app().items[0].0, "牛乳");
        // 空のまま追加しても足さない
        h.click("Add");
        assert_eq!(h.app().items.len(), 2);
    }

    #[test]
    fn list_only_creates_visible_rows_and_scrolls() {
        let mut h = book(10_000);
        let rows = h.names().iter().filter(|n| n.starts_with("Delete Item")).count();
        // 一覧の高さ（約 520）/ 32 → 17 行前後
        assert!((15..=19).contains(&rows), "{rows}");
        assert!(h.find("Item 0").is_some());
        h.scroll("Item 0", 3200.0);
        assert!(h.find("Item 0").is_none());
        assert!(h.find("Item 100").is_some());
    }

    #[test]
    fn checkbox_delete_and_segment_change_state() {
        let mut h = book(3);
        h.click("Item 0");
        assert!(h.app().items[0].1);
        h.click("Delete Item 1");
        assert_eq!(h.app().items.len(), 2);
        h.click("Open");
        assert_eq!(h.app().filter, 1);
        // Item 0 と Item 2 は完了なので、未完了だけにすると消える
        assert!(h.find("Item 0").is_none());
    }

    #[test]
    fn tab_moves_focus_and_space_presses() {
        let mut h = book(0);
        h.click("Dark");
        assert!(h.app().dark);
        // Tab で「Dark」の次（入力欄）へ、もう一度で「Add」へ。Space で押す
        h.press(Key::Tab).press(Key::Tab);
        h.app_mut().input = "x".into();
        h.press(Key::Space);
        assert_eq!(h.app().items.len(), 1);
    }

    #[test]
    fn editing_keys_and_clipboard() {
        let mut h = book(0);
        h.click("New item").type_text("abc def");
        h.press_with(Key::Backspace, Mods { ctrl: true, ..Default::default() });
        assert_eq!(h.app().input, "abc ");
        h.press_with(Key::A, Mods { ctrl: true, ..Default::default() });
        h.press_with(Key::X, Mods { ctrl: true, ..Default::default() });
        assert_eq!(h.app().input, "");
        h.press_with(Key::V, Mods { ctrl: true, ..Default::default() });
        h.press_with(Key::V, Mods { ctrl: true, ..Default::default() });
        assert_eq!(h.app().input, "abc abc ");
        h.press_with(Key::Z, Mods { ctrl: true, ..Default::default() });
        assert_eq!(h.app().input, "abc ");
    }

    #[test]
    fn render_draws_dark_background_and_saves_png() {
        let mut h = book(5);
        let light = h.render();
        assert_eq!(light.rgb(899, 639), 0xffffff);
        h.click("Dark");
        let dark = h.render();
        assert_eq!(dark.rgb(899, 639), 0x16191d);
        let dir = std::env::temp_dir().join("kargui-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.png");
        dark.save_png(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }

    #[test]
    fn fixed_sizes_are_scaled() {
        struct Fixed;
        impl App for Fixed {
            fn view(&mut self, ui: &mut Ui) {
                ui.row(|ui| {
                    ui.button("Wide").width(Size::Fixed(200.0));
                });
            }
        }
        let h = Harness::with_size(Fixed, 400.0, 300.0, 1.5);
        assert_eq!(h.find("Wide").unwrap().w, 300.0);
    }
}
