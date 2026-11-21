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
    fn clicking_a_checkbox_label_toggles_it() {
        // 作者の手の確かめで、チェックの横の文字を押しても切り替わらなかった（文字を別の部品にしていた）。
        // 文字をチェックの一部にすれば、文字の右端を押しても切り替わる
        struct Task {
            done: bool,
        }
        impl App for Task {
            fn view(&mut self, ui: &mut Ui) {
                ui.row(|ui| {
                    let done = self.done;
                    ui.checkbox(&mut self.done, "00001 企画書を作る").strike(done);
                    ui.fill();
                });
            }
        }
        let mut h = Harness::new(Task { done: false });
        let r = h.find("00001 企画書を作る").unwrap();
        let (x, y) = (r.right() - 3.0, r.y + r.h / 2.0);
        h.send(vec![Event::PointerDown { x, y, shift: false }, Event::PointerUp { x, y }]);
        assert!(h.app().done);
        // 文字の外（右の空き）を押しても切り替わらない
        let (x, y) = (r.right() + 40.0, r.y + r.h / 2.0);
        h.send(vec![Event::PointerDown { x, y, shift: false }, Event::PointerUp { x, y }]);
        assert!(h.app().done);
    }

    #[test]
    fn drawing_only_changes_matches_drawing_everything_while_scrolling() {
        // 作者の手の確かめで、スクロールするとスクロールバーのつまみが何個も出た。
        // CPU の道の「変わった所だけ描く」絵が、毎フレーム「全部描き直した」絵と同じかを比べる
        let mut h = book(100);
        let (w, hh) = (h.rt.cx.width as i32, h.rt.cx.height as i32);
        let mut px = vec![0u32; (w * hh) as usize];
        let mut summary = crate::damage::Summary::default();
        let mut step = |h: &mut Harness<Book>, what: &str| {
            let bg = h.rt.cx.theme.background;
            let out = crate::cpu_frame::draw(&mut px, w, hh, &h.rt.draw, &h.rt.cx.text.atlas, bg, &summary, false);
            summary = out.summary;
            let full = h.render();
            let mut diff = 0;
            let mut area = IRect::default();
            for (i, (a, b)) in px.iter().zip(&full.pixels).enumerate() {
                if a != b {
                    diff += 1;
                    area = area.union(&IRect::new(i as i32 % w, i as i32 / w, 1, 1));
                }
            }
            let thumb = h.rt.draw.quads.iter().rev().find(|q| q.rect[0] > (w - 20) as f32).map(|q| q.rect);
            assert_eq!(diff, 0, "{what}: {diff} 画素が違う。範囲 {area:?}、今のつまみ {thumb:?}（{}）", out.mode);
        };
        step(&mut h, "最初");
        let list = h.rt.cx.prev.lists[0].rect;
        let (x, y) = (list.x + list.w / 2.0, list.y + list.h / 2.0);
        for k in 0..20 {
            h.send(vec![Event::Wheel { x, y, dy: 96.0 }]);
            step(&mut h, &format!("下へ {} 回目", k + 1));
        }
        for k in 0..10 {
            h.send(vec![Event::Wheel { x, y, dy: -64.0 }]);
            step(&mut h, &format!("上へ {} 回目", k + 1));
        }
        // スクロールの途中で、ほかの操作も混ぜる
        let first_row = h.names().into_iter().find(|n| n.starts_with("Item ")).unwrap();
        h.click(&first_row);
        step(&mut h, "チェック");
        let delete = h.names().into_iter().find(|n| n.starts_with("Delete Item")).unwrap();
        h.click(&delete);
        step(&mut h, "削除");
        let add = h.find("Add").unwrap();
        h.rt.cx.hover = h.rt.hover_at(add.x + 2.0, add.y + 2.0);
        h.frame();
        step(&mut h, "ホバー");
        h.click("New item").type_text("milk");
        step(&mut h, "文字");
        h.ime_compose("ぎゅうにゅう");
        step(&mut h, "変換中");
        h.ime_commit("牛乳");
        step(&mut h, "確定");
        h.click("Open");
        step(&mut h, "絞り込み");
        h.send(vec![Event::Wheel { x, y, dy: 2000.0 }]);
        step(&mut h, "大きなスクロール");
        h.click("All");
        h.app_mut().items.extend((0..300).map(|i| (format!("More {i}"), false)));
        h.frame();
        step(&mut h, "件数を増やす");
        h.send(vec![Event::Wheel { x, y, dy: 96.0 }]);
        step(&mut h, "増やしたあとのスクロール");
        // スクロールの棒をつかんで動かす（一度に大きく動く。つまみの濃さも変わる）
        let l = h.rt.cx.prev.lists[0].clone();
        let t = l.thumb.unwrap();
        let tx = t.x + t.w / 2.0;
        h.rt.cx.bar_hover = Some(l.id);
        h.frame();
        step(&mut h, "棒のホバー");
        h.send(vec![Event::PointerDown { x: tx, y: t.y + 3.0, shift: false }]);
        step(&mut h, "つまみをつかむ");
        for k in 1..=6 {
            h.send(vec![Event::PointerMove { x: tx, y: t.y + 3.0 + k as f32 * 7.0 }]);
            step(&mut h, &format!("つまみを動かす {k} 回目"));
        }
        h.send(vec![Event::PointerMove { x: tx, y: l.rect.bottom() + 50.0 }]);
        step(&mut h, "つまみを下の端へ");
        h.send(vec![Event::PointerUp { x: tx, y: l.rect.bottom() + 50.0 }]);
        step(&mut h, "つまみを離す");
        h.send(vec![Event::PointerDown { x: tx, y: l.rect.y + 3.0, shift: false }, Event::PointerUp { x: tx, y: l.rect.y + 3.0 }]);
        step(&mut h, "棒の空いた所を押す");
        h.rt.cx.bar_hover = None;
        h.frame();
        step(&mut h, "棒から離れる");
    }

    fn press(h: &mut Harness<Book>, x: f32, y: f32) {
        h.send(vec![Event::PointerDown { x, y, shift: false }]);
    }
    fn drag_to(h: &mut Harness<Book>, x: f32, y: f32) {
        h.send(vec![Event::PointerMove { x, y }]);
    }
    fn release(h: &mut Harness<Book>, x: f32, y: f32) {
        h.send(vec![Event::PointerUp { x, y }]);
    }
    fn done_count(h: &Harness<Book>) -> usize {
        h.app().items.iter().filter(|(_, d)| *d).count()
    }

    #[test]
    fn tab_walks_through_list_rows_and_scrolls_them_into_view() {
        // 段階 0 の宿題: Tab で一覧の行の部品へ移る。見えている行の下へも、1 行ずつスクロールして進む
        let mut h = book(100);
        h.click("New item");
        let list = h.rt.cx.prev.lists[0].clone();
        // Tab を押して、焦点のある部品の名前を返す。一覧の行の部品なら、全部見えていることも確かめる
        let step = |h: &mut Harness<Book>, back: bool| -> String {
            h.press_with(Key::Tab, Mods { shift: back, ..Default::default() });
            let f = h.rt.cx.focus.expect("Tab で焦点がある");
            let name = (0..h.rt.tree.info.len()).find(|&i| h.rt.tree.info[i].id == f).and_then(|i| a11y::name_of(&h.rt.tree, i)).unwrap();
            if name.starts_with("Item ") || name.starts_with("Delete Item") {
                let r = h.find(&name).unwrap();
                assert!(r.y >= list.rect.y - 0.5 && r.bottom() <= list.rect.bottom() + 0.5, "{name} が見えていない: {r:?} {:?}", list.rect);
            }
            name
        };
        let seen: Vec<String> = (0..80).map(|_| step(&mut h, false)).collect();
        // Add → All → Open → Item 0 → Delete Item 0 → Item 1 → … と、見えていた 16 行より先まで進む
        assert_eq!(&seen[..5], &["Add", "All", "Open", "Item 0", "Delete Item 0"]);
        assert!(seen.iter().any(|n| n == "Item 30"), "{seen:?}");
        // Shift+Tab で戻ると、上の行も 1 行ずつ見えるようにスクロールして戻り、一覧の前の「Open」に着く
        let back: Vec<String> = (0..80).map(|_| step(&mut h, true)).take_while(|n| n != "Open").collect();
        assert_eq!(back.last().map(String::as_str), Some("Item 0"), "{back:?}");
        assert!(h.find("Item 0").is_some());
    }

    struct TwoFields {
        a: String,
        b: String,
    }

    impl App for TwoFields {
        fn view(&mut self, ui: &mut Ui) {
            ui.text_input(&mut self.a).hint("A");
            ui.text_input(&mut self.b).hint("B");
            ui.button("OK");
        }
    }

    #[test]
    fn composing_text_stays_in_its_field_when_focus_moves() {
        // 計画 2.3 の日本語入力 (5): 変換の途中で別の部品を押しても、変換中の文字が消えない。
        // 元の入力欄に確定し、IME の変換は取り消す（取り消しの知らせで何も変わらない）
        let mut h = Harness::new(TwoFields { a: String::new(), b: String::new() });
        h.click("A").ime_compose("ぎゅうにゅう");
        assert_eq!(h.app().a, "");
        h.click("B");
        assert_eq!(h.app().a, "ぎゅうにゅう");
        assert_eq!(h.app().b, "");
        assert!(h.rt.cx.ime_cancel, "IME の変換を取り消すように頼む");
        h.rt.cx.ime_cancel = false;
        h.ime_compose("");
        assert_eq!((h.app().a.as_str(), h.app().b.as_str()), ("ぎゅうにゅう", ""));
        // ボタンを押した時も同じ
        h.ime_compose("かう");
        h.click("OK");
        assert_eq!(h.app().b, "かう");
        // 同じ入力欄の中を押した時も、確定してからキャレットを動かす
        h.click("A").ime_compose("を");
        h.click("A");
        assert_eq!(h.app().a, "ぎゅうにゅうを");
    }

    #[test]
    fn composing_over_a_selection_replaces_it() {
        // 選んだ文字の上で変換を始めると、選んだ文字は消えて変換中の文字になる（再変換もこの形）
        let mut h = book(0);
        h.click("New item").type_text("牛乳を買う");
        h.press_with(Key::A, Mods { ctrl: true, ..Default::default() });
        h.ime_compose("とうにゅう");
        assert_eq!(h.app().input, "");
        h.ime_commit("豆乳");
        assert_eq!(h.app().input, "豆乳");
    }

    #[test]
    fn reconversion_selects_the_range_the_ime_confirmed() {
        let mut h = book(0);
        h.click("New item").type_text("牛乳を買う");
        // IME が「牛乳」を変換し直すと決めた（UTF-8 の範囲で届く）
        h.send(vec![Event::ImeSelect { start: 0, end: "牛乳".len() }]);
        let f = h.rt.cx.ime_field.clone().expect("焦点のある入力欄の写し");
        assert_eq!((f.anchor, f.cursor), (0, "牛乳".len()));
        // その上で変換すると、選んだ所だけが置き換わる
        h.ime_compose("とうにゅう").ime_commit("豆乳");
        assert_eq!(h.app().input, "豆乳を買う");
    }

    #[test]
    fn the_clause_being_converted_gets_a_thick_underline() {
        // 作者の手の確かめ（段階 2）の写真で、どの文節を変換しているかが入力欄から分からなかった
        let mut h = book(0);
        h.click("New item");
        let clauses = Event::ImeClauses { bounds: vec![0, 3, 6, 8], target: Some((3, 6)), cursor: Some(6) };
        h.send(vec![Event::ImePreedit("明日は東京に行く".into()), clauses]);
        let f = h.rt.cx.ime_field.clone().unwrap();
        let p = h.rt.cx.preedit_rect.unwrap();
        let img = h.render();
        // 文節の真ん中の列で、下線のあたり（縦 6 画素）の暗さを合計して比べる。太い下線はおよそ 2 倍暗い
        let bottom = p.bottom().round() as u32;
        let dark = |x: f32| -> u32 {
            (bottom - 4..bottom + 2)
                .map(|y| {
                    let c = img.rgb(x as u32, y);
                    765 - (((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff))
                })
                .sum()
        };
        let mid = |a: usize, b: usize| (f.preedit_xs[a] + f.preedit_xs[b]) / 2.0;
        let (first, target, last) = (dark(mid(0, 3)), dark(mid(3, 6)), dark(mid(6, 8)));
        assert!(first > 300 && last > 300, "ほかの文節にも下線がある: {first} {last}");
        assert!(target * 2 > first * 3 && target * 2 > last * 3, "変換中の文節 {target} / ほかの文節 {first} {last}");
        // キャレットは IME が示した位置（変換中の文節の終わり）
        assert_eq!(h.rt.cx.caret.unwrap().x, f.preedit_xs[6]);
        // 新しい変換中の文字が届くと、前の文節の区切りは捨てる
        h.ime_compose("あ");
        assert!(h.rt.cx.preedit_clauses.is_none());
    }

    #[test]
    fn ime_field_remembers_where_each_composing_character_is() {
        let mut h = book(0);
        h.click("New item").type_text("ab");
        h.ime_compose("かな");
        let f = h.rt.cx.ime_field.clone().unwrap();
        assert_eq!(f.value, "ab");
        assert_eq!(f.cursor, 2);
        assert_eq!(f.preedit_xs.len(), 3, "2 文字なら境目は 3 つ");
        assert!(f.preedit_xs[0] < f.preedit_xs[1] && f.preedit_xs[1] < f.preedit_xs[2]);
        // 境目の両端は、下線を引く範囲（変換中の文字の範囲）と同じ
        let p = h.rt.cx.preedit_rect.unwrap();
        assert_eq!(f.preedit_xs[0], p.x);
        assert!((f.preedit_xs[2] - p.right()).abs() < 1.0, "{:?} {p:?}", f.preedit_xs);
    }

    #[test]
    fn dragging_the_scroll_thumb_moves_the_list() {
        // 作者の手の確かめ（5 回目）で、1 万件の一覧のつまみを押しても動かなかった（ホイールでしか動かせなかった）
        let mut h = book(10_000);
        let done = done_count(&h);
        let list = h.rt.cx.prev.lists[0].clone();
        let thumb = list.thumb.expect("1 万件ならつまみがある");
        let x = thumb.x + thumb.w / 2.0;
        // つまみの上から 5 画素の所をつかみ、一覧の下の端より下まで動かす（端で止まる）
        press(&mut h, x, thumb.y + 5.0);
        drag_to(&mut h, x, list.rect.bottom() + 200.0);
        assert!(h.find("Item 9999").is_some(), "下の端まで動かすと最後の行が見える: {:?}", h.names());
        release(&mut h, x, list.rect.bottom() + 200.0);
        // 真ん中まで戻すと、真ん中あたりの行が見える
        let thumb = h.rt.cx.prev.lists[0].thumb.unwrap();
        press(&mut h, x, thumb.y + 5.0);
        drag_to(&mut h, x, list.rect.y + (list.rect.h - thumb.h) / 2.0 + 5.0);
        let rows: Vec<usize> = h.names().iter().filter_map(|n| n.strip_prefix("Item ")?.parse().ok()).collect();
        assert!(rows.iter().any(|&i| (4900..5100).contains(&i)), "真ん中の行が見える: {rows:?}");
        // 一番上まで戻す
        drag_to(&mut h, x, list.rect.y - 300.0);
        release(&mut h, x, list.rect.y - 300.0);
        assert!(h.find("Item 0").is_some());
        // つかんで動かしている間に、ほかの部品は押されない
        assert_eq!(h.app().items.len(), 10_000);
        assert_eq!(done_count(&h), done);
    }

    #[test]
    fn clicking_the_scroll_track_moves_one_page() {
        // 100 件（中身 3200、一覧 約 500）なら、1 ページ動くとつまみも同じくらい動く
        let mut h = book(100);
        let list = h.rt.cx.prev.lists[0].clone();
        let thumb = list.thumb.unwrap();
        let x = thumb.x + thumb.w / 2.0;
        // つまみより下の空いた所を押すと、一覧の高さ 1 つ分だけ下へ動く
        press(&mut h, x, list.rect.bottom() - 10.0);
        release(&mut h, x, list.rect.bottom() - 10.0);
        assert_eq!(h.rt.cx.scrolls.get(&list.id).copied(), Some(list.rect.h));
        // つまみより上を押すと戻る
        press(&mut h, x, list.rect.y + 2.0);
        release(&mut h, x, list.rect.y + 2.0);
        assert_eq!(h.rt.cx.scrolls.get(&list.id).copied(), Some(0.0));
        assert_eq!(h.app().items.len(), 100);
    }

    #[test]
    fn buttons_next_to_the_scroll_bar_still_work() {
        // 削除ボタンの右の端は、棒の押せる範囲と重なる。ボタンの本来の範囲はボタンが勝つ
        let mut h = book(100);
        let r = h.find("Delete Item 0").unwrap();
        let bar = h.rt.cx.prev.lists[0].bar.unwrap();
        assert!(bar.x < r.right(), "この試験は重なりがある時のため: 棒 {bar:?} ボタン {r:?}");
        let (x, y) = (r.right() - 1.0, r.y + r.h / 2.0);
        press(&mut h, x, y);
        release(&mut h, x, y);
        assert_eq!(h.app().items.len(), 99);
    }

    #[test]
    fn clicks_just_outside_a_small_checkbox_still_hit_it() {
        // 作者の手の確かめ（3 回目）で、箱の左の端から 2 画素外と、文字の終わりから 10 画素右のクリックが当たらなかった
        struct Rows {
            done: Vec<bool>,
        }
        impl App for Rows {
            fn view(&mut self, ui: &mut Ui) {
                let done = &mut self.done;
                ui.list(done.len(), 32.0).grow().show(|ui, i| {
                    ui.checkbox(&mut done[i], format!("{:05} 企画書を作る", i + 1));
                    ui.fill();
                    ui.button("削除").small();
                });
            }
        }
        let mut h = Harness::new(Rows { done: vec![false; 3] });
        let r = h.find("00001 企画書を作る").unwrap();
        let mid = r.y + r.h / 2.0;
        for (x, y) in [(r.x - 2.0, mid), (r.right() + 10.0, mid), (r.x + 4.0, r.y - 5.0)] {
            let before = h.app().done[0];
            h.send(vec![Event::PointerDown { x, y, shift: false }, Event::PointerUp { x, y }]);
            assert_ne!(h.app().done[0], before, "({x}, {y}) が当たらない");
        }
        // 2 行目の本来の範囲は、1 行目の余白より強い
        let r2 = h.find("00002 企画書を作る").unwrap();
        let (x, y) = (r2.x + 4.0, r2.y + 1.0);
        h.send(vec![Event::PointerDown { x, y, shift: false }, Event::PointerUp { x, y }]);
        assert!(h.app().done[1]);
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

