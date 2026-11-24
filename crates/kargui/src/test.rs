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

use std::borrow::Cow;
use std::rc::Rc;

use crate::app::App;
use crate::geom::{IRect, Rect};
use crate::input::{Event, Key, Mods};
use crate::raster::Canvas;
use crate::runtime::Runtime;
use crate::text::Text;
use crate::ui::context::MemoryClipboard;
use crate::ui::{Ui, a11y};

/// Runs an [`App`] without a window. See the [module documentation](self).
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
pub struct Harness<A: App> {
    app: A,
    /// 元の窓
    rt: Runtime,
    /// ほかの窓（Ui::window のキーと、その窓）
    windows: Vec<(String, Runtime)>,
    /// 操作している窓（None なら元の窓）
    current: Option<String>,
    /// アプリが出すように頼んで、まだ答えていないファイルのダイアログ
    file_dialog: Option<(String, crate::FileDialog)>,
    /// アプリが閉じた（閉じるのを認めた、か Ui::quit）
    closed: bool,
    /// 押したままのポインターの位置（Harness::drag_over の後。Harness::release で離す）
    pointer_held: Option<(f32, f32)>,
    /// アプリが渡した書体のファイル（ほかの窓にも読み込む）
    font_files: Rc<Vec<Cow<'static, [u8]>>>,
}

impl<A: App> Harness<A> {
    /// A 900×640 screen in light mode at 100% scale, after the first frame.
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
    /// let h = Harness::new(Notes::default());
    /// assert!(h.find("New note").is_some());
    /// ```
    pub fn new(app: A) -> Self {
        Self::with_size(app, 900.0, 640.0, 1.0)
    }

    /// A screen of the given size (logical pixels) and scale, after the first frame.
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
    /// // A small phone-sized screen at 200% scale: 360 x 640 logical pixels is 720 x 1280 pixels.
    /// let mut h = Harness::with_size(Notes::default(), 360.0, 640.0, 2.0);
    /// assert_eq!(h.render().width, 720);
    /// ```
    pub fn with_size(app: A, width: f32, height: f32, scale: f32) -> Self {
        Self::make(app, width, height, scale, false, &crate::Font::Ui, Rc::new(Vec::new()))
    }

    /// A screen with the app's [`Options`](crate::Options): their size, font and font files
    /// ([`Options::font`](crate::Options::font), [`Options::fonts`](crate::Options::fonts)) and
    /// dark mode if `dark` is `Some(true)` (otherwise light), at 100% scale, after the first frame.
    ///
    /// ```
    /// use kargui::{App, Font, Options, Ui};
    /// use kargui::test::Harness;
    ///
    /// struct Receipt;
    ///
    /// impl App for Receipt {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.label(format!("{:?}", ui.font()));
    ///     }
    /// }
    ///
    /// let mut options = Options::new("Receipt");
    /// options.size = (320.0, 480.0);
    /// options.font = Font::Monospace;
    /// let h = Harness::with_options(Receipt, &options);
    /// assert!(h.find("Monospace").is_some());
    /// ```
    pub fn with_options(app: A, options: &crate::Options) -> Self {
        let (w, h) = options.size;
        Self::make(app, w, h, 1.0, options.dark == Some(true), &options.font, Rc::new(options.fonts.clone()))
    }

    fn make(app: A, width: f32, height: f32, scale: f32, dark: bool, font: &crate::Font, font_files: Rc<Vec<Cow<'static, [u8]>>>) -> Self {
        let text = Text::new().expect("the text engine starts");
        let mut rt = Runtime::new(text, dark, Box::new(MemoryClipboard::default()), "test".into());
        rt.resize((width * scale).round(), (height * scale).round(), scale);
        // 試験台の時計は止めておき、wait で進める（動く部品の写真と試験を毎回同じにする）
        rt.cx.frozen = Some(std::time::Duration::ZERO);
        // 今日も止める（暦の「今日」の印が日によって変わらないように）
        rt.cx.today = crate::date::Date::new(2026, 11, 22);
        // 動きは切っておく（押した後の画面が、時計を進めなくても終わりの形になる）。動きを試す時は reduce_motion(false)
        rt.cx.reduce_motion = true;
        rt.cx.text.load_fonts(&font_files);
        rt.cx.font = rt.cx.text.fonts.id(font);
        let mut h = Self { app, rt, windows: Vec::new(), current: None, file_dialog: None, closed: false, pointer_held: None, font_files };
        h.frame();
        h
    }

    /// The app.
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
    /// let h = Harness::new(Notes::default());
    /// assert!(h.app().notes.is_empty());
    /// ```
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The app, to change its state between frames.
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
    /// h.app_mut().notes.push("milk".to_string());
    /// h.frame();
    /// assert!(h.find("milk").is_some());
    /// ```
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Runs a frame without input.
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
    /// h.app_mut().notes.push("milk".to_string());
    /// assert!(h.find("milk").is_none()); // not drawn yet
    /// h.frame();
    /// assert!(h.find("milk").is_some());
    /// ```
    pub fn frame(&mut self) -> &mut Self {
        self.send(Vec::new())
    }

    fn send(&mut self, events: Vec<Event>) -> &mut Self {
        match self.current.clone() {
            None => {
                let out = self.rt.frame(&mut self.app, events);
                // 一覧の高さが決まった直後など、もう 1 フレーム要るときは続けて描く（アニメーションは除く）
                if out.want_frame && self.rt.frames < 1_000_000 {
                    self.rt.frame(&mut self.app, Vec::new());
                }
            }
            Some(key) => {
                // ほかの窓の入力。アプリは共有なので、元の窓も描き直す
                let app = &mut self.app;
                if let Some((_, rt)) = self.windows.iter_mut().find(|(k, _)| *k == key) {
                    let out = rt.frame_with(&mut |ui: &mut Ui| app.window(&key, ui), events);
                    if out.want_frame {
                        rt.frame_with(&mut |ui: &mut Ui| app.window(&key, ui), Vec::new());
                    }
                }
                self.rt.frame(&mut self.app, Vec::new());
            }
        }
        self.sync_windows();
        if let Some(d) = self.rt.cx.file_dialog_request.take() {
            self.file_dialog = Some(d);
        }
        for rt in std::iter::once(&mut self.rt).chain(self.windows.iter_mut().map(|(_, rt)| rt)) {
            if std::mem::take(&mut rt.cx.quit) {
                self.closed = true;
            }
        }
        self
    }

    /// The names a screen reader would read, in screen order.
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
    /// let h = Harness::new(Notes::default());
    /// assert!(h.names().contains(&"Add".to_string()));
    /// ```
    pub fn names(&self) -> Vec<String> {
        let rt = self.cur();
        (0..rt.tree.info.len()).filter_map(|i| a11y::name_of(&rt.tree, i)).collect()
    }

    /// Where the widget with this name is (physical pixels), if it is on screen.
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
    /// let h = Harness::new(Notes::default());
    /// let add = h.find("Add").expect("the Add button is on screen");
    /// assert!(add.w > 0.0 && add.h > 0.0);
    /// assert!(h.find("Remove").is_none());
    /// ```
    pub fn find(&self, name: &str) -> Option<Rect> {
        let rt = self.cur();
        (0..rt.tree.info.len()).find(|&i| a11y::name_of(&rt.tree, i).as_deref() == Some(name)).map(|i| rt.tree.layout[i].rect)
    }

    /// Where every widget with this name is (physical pixels), in screen-reader order. Use it when
    /// two widgets share a name, such as a table of contents entry and the heading it points to.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # struct Book;
    /// # impl App for Book {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         ui.button("Chapter 1");
    /// #         ui.heading("Chapter 1");
    /// #     }
    /// # }
    /// let h = kargui::test::Harness::new(Book);
    /// let both = h.find_all("Chapter 1");
    /// assert_eq!(both.len(), 2);
    /// assert!(both[0].y < both[1].y);
    /// ```
    pub fn find_all(&self, name: &str) -> Vec<Rect> {
        let rt = self.cur();
        (0..rt.tree.info.len()).filter(|&i| a11y::name_of(&rt.tree, i).as_deref() == Some(name)).map(|i| rt.tree.layout[i].rect)
            .collect()
    }

    /// Whether the widget with this name can be used (`Some(false)` inside [`Ui::enabled`]`(false, ..)`
    /// or after [`Button::enabled`](crate::ui::Button::enabled)`(false)`), or `None` if there is no such widget.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Form {
    ///     name: String,
    /// }
    ///
    /// impl App for Form {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.text_input(&mut self.name).hint("Name");
    ///         ui.button("Send").enabled(!self.name.is_empty());
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Form { name: String::new() });
    /// assert_eq!(h.enabled("Send"), Some(false));
    /// h.click("Name").type_text("Ada");
    /// assert_eq!(h.enabled("Send"), Some(true));
    /// ```
    pub fn enabled(&self, name: &str) -> Option<bool> {
        let rt = self.cur();
        (0..rt.tree.info.len()).find(|&i| a11y::name_of(&rt.tree, i).as_deref() == Some(name)).map(|i| !rt.tree.info[i].disabled)
    }

    /// Moves the pointer over the widget with this name and draws a frame (its hover look and, after
    /// half a second, its tooltip).
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// # use std::time::Duration;
    /// struct Bar;
    ///
    /// impl App for Bar {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.button("Save").tooltip("Saves the file");
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Bar);
    /// h.hover("Save").wait(Duration::from_millis(600));
    /// assert!(h.find("Saves the file").is_some());
    /// ```
    pub fn hover(&mut self, name: &str) -> &mut Self {
        let (x, y) = self.center(name);
        let rt = self.cur_mut();
        let id = rt.hover_at(x, y);
        rt.cx.set_hover(id);
        self.frame()
    }

    /// Lets `time` pass (for [`Ui::now`], [`Ui::request_frame_after`] and tooltips) and draws a
    /// frame. Nothing really sleeps, so tests stay fast.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// # use std::time::{Duration, Instant};
    /// struct Countdown {
    ///     end: Option<Instant>,
    /// }
    ///
    /// impl App for Countdown {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         let end = *self.end.get_or_insert(ui.now() + Duration::from_secs(3));
    ///         let left = end.saturating_duration_since(ui.now());
    ///         ui.label(format!("{} s", left.as_secs_f32().ceil()));
    ///         if !left.is_zero() {
    ///             ui.request_frame_after(Duration::from_secs(1).min(left));
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Countdown { end: None });
    /// assert!(h.find("3 s").is_some());
    /// h.wait(Duration::from_secs(2));
    /// assert!(h.find("1 s").is_some());
    /// ```
    pub fn wait(&mut self, time: std::time::Duration) -> &mut Self {
        // どの窓の時計も同じだけ進める
        for rt in std::iter::once(&mut self.rt).chain(self.windows.iter_mut().map(|(_, rt)| rt)) {
            match &mut rt.cx.frozen {
                Some(t) => *t += time,
                None => rt.cx.skew += time,
            }
        }
        self.frame()
    }

    /// 名前の部品の真ん中。同じ名前が並ぶ時は、利用者が押す物を選ぶ: 押せる部品で、真ん中を押すとその部品に当たる物
    /// （開いたメニューの項目など、いちばん上に見えている物）、次に押せる部品（見える名前と、その名前を付けた欄なら欄）
    fn center(&self, name: &str) -> (f32, f32) {
        let rt = self.cur();
        let named: Vec<usize> = (0..rt.tree.info.len()).filter(|&i| a11y::name_of(&rt.tree, i).as_deref() == Some(name)).collect();
        let usable = |i: usize| rt.tree.info[i].kind.interactive() && !rt.tree.info[i].disabled;
        let on_top = |i: usize| {
            let r = rt.tree.layout[i].rect;
            rt.cx.hit(r.x + r.w / 2.0, r.y + r.h / 2.0).is_some_and(|h| h.id == rt.tree.info[i].id)
        };
        // メニューやダイアログが開いている間は、利用者はその中を押す（外を押すと閉じる）
        let top_layer = rt.tree.overlays.iter().rev().find(|o| o.kind != crate::ui::OverlayKind::Tooltip).map(|o| o.idx);
        let in_layer = |i: usize| top_layer.is_some_and(|o| o == i || under(&rt.tree, o, i));
        let i = named
            .iter()
            .copied()
            .find(|&i| usable(i) && in_layer(i))
            .or(named.iter().copied().find(|&i| usable(i) && on_top(i)))
            .or(named.iter().copied().find(|&i| rt.tree.info[i].kind.interactive()))
            .or(named.first().copied())
            .unwrap_or_else(|| panic!("no widget named {name:?}; on screen: {:?}", self.names()));
        let r = rt.tree.layout[i].rect;
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// Clicks at a point (physical pixels from the window's top left), for widgets that share a
    /// name. When names differ, [`Harness::click`] is clearer.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// #[derive(Default)]
    /// struct Two {
    ///     second: bool,
    /// }
    ///
    /// impl App for Two {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.button("Remove");
    ///         if ui.button("Remove").clicked() {
    ///             self.second = true;
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Two::default());
    /// let r = h.find_all("Remove")[1];
    /// h.click_at(r.x + r.w / 2.0, r.y + r.h / 2.0);
    /// assert!(h.app().second);
    /// ```
    pub fn click_at(&mut self, x: f32, y: f32) -> &mut Self {
        self.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }])
    }

    /// Clicks the widget with this name (and focuses it if it can take focus).
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
    /// h.click("New note").type_text("tea").click("Add");
    /// assert_eq!(h.app().notes, ["tea"]);
    /// ```
    pub fn click(&mut self, name: &str) -> &mut Self {
        let (x, y) = self.center(name);
        self.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }])
    }

    /// Presses the widget with this name, moves the pointer by `dx`, `dy` (physical pixels) and
    /// releases it there: drags a slider, a divider, or a stroke on a canvas.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Panes {
    ///     left: f32,
    /// }
    ///
    /// impl App for Panes {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.split_left_right(&mut self.left, |ui, _| {
    ///             ui.label("Pane");
    ///         });
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Panes { left: 200.0 });
    /// h.drag("Resize", 40.0, 0.0);
    /// assert_eq!(h.app().left, 240.0);
    /// ```
    pub fn drag(&mut self, name: &str, dx: f32, dy: f32) -> &mut Self {
        let (x, y) = self.center(name);
        let (x2, y2) = (x + dx, y + dy);
        self.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerMove { x: x2, y: y2 }, Event::PointerUp { x: x2, y: y2 }])
    }

    /// Double-clicks the middle of the widget with this name. In a text field this selects the
    /// word there (the last word when the middle is past the end of the text). Separate
    /// [`Harness::click`]s never count as a double-click.
    ///
    /// ```
    /// # use kargui::{App, Ui, Key};
    /// # use kargui::test::Harness;
    /// struct Search {
    ///     query: String,
    /// }
    ///
    /// impl App for Search {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.text_input(&mut self.query).label("Search");
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Search { query: "red apples".into() });
    /// h.double_click("Search").type_text("pears");
    /// assert_eq!(h.app().query, "red pears");
    /// ```
    pub fn double_click(&mut self, name: &str) -> &mut Self {
        let (x, y) = self.center(name);
        self.send(vec![
            Event::PointerDown { x, y, shift: false, count: 1 },
            Event::PointerUp { x, y },
            Event::PointerDown { x, y, shift: false, count: 2 },
            Event::PointerUp { x, y },
        ])
    }

    /// Triple-clicks the middle of the widget with this name. In a one-line text field this
    /// selects all the text; in a multi-line field, the line there.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Search {
    ///     query: String,
    /// }
    ///
    /// impl App for Search {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.text_input(&mut self.query).label("Search");
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Search { query: "red apples".into() });
    /// h.triple_click("Search").type_text("pears");
    /// assert_eq!(h.app().query, "pears");
    /// ```
    pub fn triple_click(&mut self, name: &str) -> &mut Self {
        let (x, y) = self.center(name);
        let mut events = Vec::new();
        for count in 1..=3 {
            events.push(Event::PointerDown { x, y, shift: false, count });
            events.push(Event::PointerUp { x, y });
        }
        self.send(events)
    }

    /// Drags the widget named `name` onto the widget named `target` (drag and drop, see
    /// [`Handle::drag_source`](crate::ui::Handle::drag_source)): presses it, moves the pointer in a
    /// few steps, one frame each, and releases it over the middle of `target`.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Shelf {
    ///     books: Vec<&'static str>,
    ///     read: Vec<&'static str>,
    /// }
    ///
    /// impl App for Shelf {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         for (k, book) in self.books.iter().enumerate() {
    ///             ui.label(*book).drag_source(k);
    ///         }
    ///         let mut read = ui.column(|ui| {
    ///             ui.heading("Read");
    ///         });
    ///         if let Some(k) = read.dropped::<usize>() {
    ///             let book = self.books.remove(k);
    ///             self.read.push(book);
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Shelf { books: vec!["Dune", "Emma"], read: vec![] });
    /// h.drag_to("Emma", "Read");
    /// assert_eq!(h.app().read, ["Emma"]);
    /// ```
    pub fn drag_to(&mut self, name: &str, target: &str) -> &mut Self {
        self.drag_over(name, target).release()
    }

    /// Like [`Harness::drag_to`], but keeps the pointer button pressed over the middle of `target`:
    /// to look at what is drawn while dragging (the highlighted target, the box that follows the
    /// pointer). Finish with [`Harness::release`] (drops there) or Escape (cancels).
    ///
    /// ```
    /// # use kargui::{App, Key, Ui};
    /// # use kargui::test::Harness;
    /// struct Bin {
    ///     emptied: u32,
    /// }
    ///
    /// impl App for Bin {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.button("Old file").drag_source(());
    ///         let mut bin = ui.column(|ui| {
    ///             ui.heading("Bin");
    ///         });
    ///         if bin.dropped::<()>().is_some() {
    ///             self.emptied += 1;
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Bin { emptied: 0 });
    /// h.drag_over("Old file", "Bin").press(Key::Escape).release();
    /// assert_eq!(h.app().emptied, 0, "Escape cancelled the drag");
    /// h.drag_over("Old file", "Bin").release();
    /// assert_eq!(h.app().emptied, 1);
    /// ```
    pub fn drag_over(&mut self, name: &str, target: &str) -> &mut Self {
        let (x, y) = self.center(name);
        let (x2, y2) = self.center(target);
        self.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
        for k in 1..=4 {
            let f = k as f32 / 4.0;
            self.send(vec![Event::PointerMove { x: x + (x2 - x) * f, y: y + (y2 - y) * f }]);
        }
        self.pointer_held = Some((x2, y2));
        self
    }

    /// Drops files from another app (for example Explorer) on the widget named `target`: the files
    /// enter the window, move over the middle of `target` (one frame each) and are released there.
    /// The widget that takes `Vec<PathBuf>` with [`Handle::dropped`](crate::ui::Handle::dropped)
    /// receives them.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// use std::path::PathBuf;
    ///
    /// struct Viewer {
    ///     opened: Vec<PathBuf>,
    /// }
    ///
    /// impl App for Viewer {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         let mut area = ui.column(|ui| {
    ///             ui.heading("Drop pictures here");
    ///         });
    ///         if let Some(paths) = area.dropped::<Vec<PathBuf>>() {
    ///             self.opened.extend(paths);
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Viewer { opened: vec![] });
    /// h.drop_files("Drop pictures here", &["C:\\photos\\cat.png"]);
    /// assert_eq!(h.app().opened, [PathBuf::from("C:\\photos\\cat.png")]);
    /// ```
    pub fn drop_files(&mut self, target: &str, paths: &[&str]) -> &mut Self {
        let (x, y) = self.center(target);
        let paths = paths.iter().map(std::path::PathBuf::from).collect();
        // 窓の端から入って、先の部品の上まで動かす
        self.send(vec![Event::FileDragEnter { x: 1.0, y: 1.0, paths }]);
        self.send(vec![Event::FileDragOver { x, y }]);
        self.send(vec![Event::FileDrop { x, y }])
    }

    /// Acts as if the user turned animations off in the system (`true`, the default in tests) or
    /// on (`false`), and draws a frame. While motion is off, [`Ui::animate`](crate::Ui::animate)
    /// jumps to its target and [`Ui::reduce_motion`](crate::Ui::reduce_motion) returns `true`, so
    /// the screen after an action is already its end state. To test motion itself, turn it on and
    /// move the clock with [`Harness::wait`].
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Panel {
    ///     open: bool,
    ///     shown: f32,
    /// }
    ///
    /// impl App for Panel {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("Open").clicked() {
    ///             self.open = true;
    ///         }
    ///         self.shown = ui.animate_bool("panel", self.open);
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Panel { open: false, shown: 0.0 });
    /// h.click("Open");
    /// assert_eq!(h.app().shown, 1.0, "motion is off in tests: the panel is open at once");
    ///
    /// let mut h = Harness::new(Panel { open: false, shown: 0.0 });
    /// h.reduce_motion(false).click("Open");
    /// assert_eq!(h.app().shown, 0.0, "with motion on, it has only started");
    /// h.wait(std::time::Duration::from_millis(250));
    /// assert_eq!(h.app().shown, 1.0);
    /// ```
    pub fn reduce_motion(&mut self, on: bool) -> &mut Self {
        for rt in std::iter::once(&mut self.rt).chain(self.windows.iter_mut().map(|(_, rt)| rt)) {
            rt.cx.reduce_motion = on;
        }
        self.frame()
    }

    /// Releases the pointer button where [`Harness::drag_over`] left it. Does nothing if the button
    /// is not held.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// # struct Empty;
    /// # impl App for Empty {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         ui.label("Nothing to drag");
    /// #     }
    /// # }
    /// let mut h = Harness::new(Empty);
    /// h.release();
    /// ```
    pub fn release(&mut self) -> &mut Self {
        match self.pointer_held.take() {
            Some((x, y)) => self.send(vec![Event::PointerUp { x, y }]),
            None => self,
        }
    }

    /// Moves the test to the window opened with [`Ui::window`](crate::Ui::window) under `key`:
    /// the following finds, clicks and key presses go to it, until [`Harness::main_window`] (or
    /// until it closes). Panics if no such window is open.
    ///
    /// ```
    /// # use kargui::{App, Options, Ui};
    /// # use kargui::test::Harness;
    /// #[derive(Default)]
    /// struct Mail {
    ///     compose: bool,
    ///     sent: u32,
    /// }
    ///
    /// impl App for Mail {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("New message").clicked() {
    ///             self.compose = true;
    ///         }
    ///         ui.window("compose", &Options::new("New message"), &mut self.compose);
    ///     }
    ///
    ///     fn window(&mut self, _key: &str, ui: &mut Ui) {
    ///         if ui.button("Send").clicked() {
    ///             self.sent += 1;
    ///             self.compose = false;
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Mail::default());
    /// h.click("New message").window("compose").click("Send");
    /// assert_eq!(h.app().sent, 1);
    /// assert!(h.windows().is_empty());
    /// ```
    pub fn window(&mut self, key: &str) -> &mut Self {
        assert!(self.windows.iter().any(|(k, _)| k == key), "no window {key:?} is open; open windows: {:?}", self.windows());
        self.current = Some(key.to_string());
        self
    }

    /// Moves the test back to the main window. See [`Harness::window`].
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// # struct Hello;
    /// # impl App for Hello {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         ui.label("Hello");
    /// #     }
    /// # }
    /// let mut h = Harness::new(Hello);
    /// assert!(h.main_window().find("Hello").is_some());
    /// ```
    pub fn main_window(&mut self) -> &mut Self {
        self.current = None;
        self
    }

    /// The keys of the other windows that are open, in the order they were opened.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// # struct Hello;
    /// # impl App for Hello {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         ui.label("Hello");
    /// #     }
    /// # }
    /// let h = Harness::new(Hello);
    /// assert!(h.windows().is_empty());
    /// ```
    pub fn windows(&self) -> Vec<String> {
        self.windows.iter().map(|(k, _)| k.clone()).collect()
    }

    /// Closes the window `key` as if the user clicked its close button.
    ///
    /// ```
    /// # use kargui::{App, Options, Ui};
    /// # use kargui::test::Harness;
    /// #[derive(Default)]
    /// struct Viewer {
    ///     help: bool,
    /// }
    ///
    /// impl App for Viewer {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("Help").clicked() {
    ///             self.help = true;
    ///         }
    ///         ui.window("help", &Options::new("Help"), &mut self.help);
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Viewer::default());
    /// h.click("Help").close_window("help");
    /// assert!(!h.app().help);
    /// ```
    pub fn close_window(&mut self, key: &str) -> &mut Self {
        self.rt.cx.closed_windows.insert(key.to_string());
        self.current = None;
        self.send(Vec::new())
    }

    /// 試験で操作している窓（ほかの窓に移っていればその窓）
    fn cur(&self) -> &Runtime {
        match &self.current {
            Some(key) => self.windows.iter().find(|(k, _)| k == key).map_or(&self.rt, |(_, rt)| rt),
            None => &self.rt,
        }
    }

    fn cur_mut(&mut self) -> &mut Runtime {
        match self.current.clone() {
            Some(key) => match self.windows.iter_mut().find(|(k, _)| *k == key) {
                Some((_, rt)) => rt,
                None => &mut self.rt,
            },
            None => &mut self.rt,
        }
    }

    /// 元の窓が頼んだ窓に合わせて、ほかの窓を開け閉めし、どれも描き直す（窓はアプリを共有する）
    fn sync_windows(&mut self) {
        let requested = self.rt.cx.windows.clone();
        self.windows.retain(|(k, _)| requested.iter().any(|r| &r.key == k));
        if self.current.as_ref().is_some_and(|c| !self.windows.iter().any(|(k, _)| k == c)) {
            self.current = None;
        }
        for r in &requested {
            if self.windows.iter().any(|(k, _)| *k == r.key) {
                continue;
            }
            let text = Text::new().expect("the text engine starts");
            let mut rt = Runtime::new(text, self.rt.cx.dark, Box::new(MemoryClipboard::default()), r.title.clone());
            let scale = self.rt.cx.scale;
            rt.resize((r.size.0 * scale).round(), (r.size.1 * scale).round(), scale);
            rt.cx.frozen = self.rt.cx.frozen;
            rt.cx.today = self.rt.cx.today;
            rt.cx.reduce_motion = self.rt.cx.reduce_motion;
            // 書体は主の窓の今の書体と、同じ書体のファイル
            rt.cx.text.load_fonts(&self.font_files);
            let font = self.rt.cx.text.fonts.get(self.rt.cx.font).clone();
            rt.cx.font = rt.cx.text.fonts.id(&font);
            self.windows.push((r.key.clone(), rt));
        }
        let app = &mut self.app;
        for (key, rt) in &mut self.windows {
            let out = rt.frame_with(&mut |ui: &mut Ui| app.window(key, ui), Vec::new());
            if out.want_frame {
                rt.frame_with(&mut |ui: &mut Ui| app.window(key, ui), Vec::new());
            }
        }
    }

    /// The key of the file dialog the app asked for and that has not been answered yet.
    ///
    /// ```
    /// # use kargui::{App, FileDialog, Ui};
    /// # use kargui::test::Harness;
    /// struct Importer;
    ///
    /// impl App for Importer {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("Import").clicked() {
    ///             ui.file_dialog("import", FileDialog::open());
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Importer);
    /// h.click("Import");
    /// assert_eq!(h.file_dialog_key().as_deref(), Some("import"));
    /// ```
    pub fn file_dialog_key(&self) -> Option<String> {
        self.file_dialog.as_ref().map(|(k, _)| k.clone())
    }

    /// The file dialog the app asked for and that has not been answered yet.
    ///
    /// ```
    /// # use kargui::{App, FileDialog, Ui};
    /// # use kargui::test::Harness;
    /// # struct Exporter;
    /// # impl App for Exporter {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         if ui.button("Export").clicked() {
    /// #             ui.file_dialog("export", FileDialog::save());
    /// #         }
    /// #     }
    /// # }
    /// let mut h = Harness::new(Exporter);
    /// h.click("Export");
    /// assert!(h.file_dialog().unwrap().is_save());
    /// ```
    pub fn file_dialog(&self) -> Option<&crate::FileDialog> {
        self.file_dialog.as_ref().map(|(_, d)| d)
    }

    /// Answers the file dialog as if the user chose these files, and draws a frame.
    ///
    /// ```
    /// # use kargui::{App, FileDialog, Ui};
    /// # use kargui::test::Harness;
    /// #[derive(Default)]
    /// struct Viewer {
    ///     file: Option<std::path::PathBuf>,
    /// }
    ///
    /// impl App for Viewer {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("Open").clicked() {
    ///             ui.file_dialog("open", FileDialog::open());
    ///         }
    ///         if let Some(path) = ui.chosen_file("open") {
    ///             self.file = Some(path);
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Viewer::default());
    /// h.click("Open").choose_files(&["C:/photo.png"]);
    /// assert_eq!(h.app().file.as_deref(), Some(std::path::Path::new("C:/photo.png")));
    /// ```
    pub fn choose_files(&mut self, paths: &[&str]) -> &mut Self {
        let (key, _) = self.file_dialog.take().expect("no file dialog is waiting for an answer");
        self.rt.cx.file_results.insert(key, paths.iter().map(std::path::PathBuf::from).collect());
        self.main_window();
        self.send(Vec::new())
    }

    /// Answers the file dialog as if the user cancelled it, and draws a frame.
    ///
    /// ```
    /// # use kargui::{App, FileDialog, Ui};
    /// # use kargui::test::Harness;
    /// # struct Importer;
    /// # impl App for Importer {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         if ui.button("Import").clicked() {
    /// #             ui.file_dialog("import", FileDialog::open());
    /// #         }
    /// #     }
    /// # }
    /// let mut h = Harness::new(Importer);
    /// h.click("Import").cancel_file_dialog();
    /// assert_eq!(h.file_dialog_key(), None);
    /// ```
    pub fn cancel_file_dialog(&mut self) -> &mut Self {
        if let Some((key, _)) = self.file_dialog.take() {
            self.rt.cx.file_results.insert(key, Vec::new());
        }
        self.main_window();
        self.send(Vec::new())
    }

    /// Asks the app to close as if the user clicked the main window's close button: calls
    /// [`App::close_requested`], and draws a frame.
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Hello;
    ///
    /// impl App for Hello {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.label("Hello");
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Hello);
    /// h.request_close();
    /// assert!(h.is_closed());
    /// ```
    pub fn request_close(&mut self) -> &mut Self {
        if self.app.close_requested() {
            self.closed = true;
        }
        self.main_window();
        self.send(Vec::new())
    }

    /// Whether the app has closed (the close was allowed, or the app called
    /// [`Ui::quit`](crate::Ui::quit)).
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// struct Quitter;
    ///
    /// impl App for Quitter {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("Exit").clicked() {
    ///             ui.quit();
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Quitter);
    /// assert!(!h.is_closed());
    /// h.click("Exit");
    /// assert!(h.is_closed());
    /// ```
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Right-clicks the widget with this name (opens its
    /// [`Handle::context_menu`](crate::ui::Handle::context_menu)).
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// #[derive(Default)]
    /// struct Files {
    ///     deleted: bool,
    /// }
    ///
    /// impl App for Files {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.label("a.txt").context_menu(|m| {
    ///             if m.item("Delete").clicked() {
    ///                 self.deleted = true;
    ///             }
    ///         });
    ///     }
    /// }
    ///
    /// let mut h = Harness::new(Files::default());
    /// h.right_click("a.txt").click("Delete");
    /// assert!(h.app().deleted);
    /// ```
    pub fn right_click(&mut self, name: &str) -> &mut Self {
        let (x, y) = self.center(name);
        self.send(vec![Event::ContextMenu { x, y }])
    }

    /// Types text into the focused field, as if typed without an input method.
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
    /// h.click("New note").type_text("milk");
    /// assert_eq!(h.app().input, "milk");
    /// ```
    pub fn type_text(&mut self, text: &str) -> &mut Self {
        self.send(text.chars().map(Event::Char).collect())
    }

    /// Presses a key.
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
    pub fn press(&mut self, key: Key) -> &mut Self {
        self.press_with(key, Mods::default())
    }

    /// Presses a key with modifiers (for example Ctrl+Z).
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
    /// h.click("New note").type_text("milk");
    /// // Ctrl+A selects everything, so the next text replaces it.
    /// h.press_with(Key::A, Mods { ctrl: true, ..Mods::default() }).type_text("tea");
    /// assert_eq!(h.app().input, "tea");
    /// ```
    pub fn press_with(&mut self, key: Key, mods: Mods) -> &mut Self {
        self.send(vec![Event::Key { key, mods }])
    }

    /// The input method shows `text` as being composed (not yet converted or committed).
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
    /// h.click("New note").ime_compose("ぎゅうにゅう");
    /// assert_eq!(h.app().input, ""); // still composing
    /// ```
    pub fn ime_compose(&mut self, text: &str) -> &mut Self {
        self.send(vec![Event::ImePreedit(text.to_string())])
    }

    /// The input method commits `text` (the user pressed Enter to accept the conversion).
    /// This Enter is handled by the input method and does not reach the app.
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
    /// h.click("New note").ime_compose("ぎゅうにゅう").ime_commit("牛乳");
    /// assert_eq!(h.app().input, "牛乳");
    /// assert!(h.app().notes.is_empty()); // the committing Enter does not submit
    /// ```
    pub fn ime_commit(&mut self, text: &str) -> &mut Self {
        self.send(vec![Event::ImePreedit(String::new()), Event::ImeCommit(text.to_string())])
    }

    /// Scrolls the list under the widget with this name by `dy` logical pixels (down is positive).
    ///
    /// ```
    /// # use kargui::{App, Ui};
    /// # use kargui::test::Harness;
    /// # struct Log;
    /// # impl App for Log {
    /// #     fn view(&mut self, ui: &mut Ui) {
    /// #         ui.heading("Log");
    /// #         ui.list(1000, 28.0).grow().show(|ui, i| {
    /// #             ui.label(format!("Line {i}"));
    /// #         });
    /// #     }
    /// # }
    ///
    /// let mut h = Harness::new(Log);
    /// assert!(h.find("Line 0").is_some());
    /// h.scroll("Line 0", 2800.0);
    /// assert!(h.find("Line 0").is_none());
    /// ```
    pub fn scroll(&mut self, name: &str, dy: f32) -> &mut Self {
        let (x, y) = self.center(name);
        self.send(vec![Event::Wheel { x, y, dy }])
    }

    /// Draws the screen with the CPU renderer.
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
    /// let image = h.render();
    /// assert_eq!((image.width, image.height), (900, 640));
    /// assert_eq!(image.rgb(0, 0), 0xffffff); // the light background
    /// ```
    pub fn render(&mut self) -> Image {
        let rt = self.cur();
        let (w, h) = (rt.cx.width as i32, rt.cx.height as i32);
        let mut pixels = vec![0u32; (w * h) as usize];
        let bg = rt.cx.theme.background;
        let atlas = &rt.cx.text.atlas;
        Canvas { px: &mut pixels, width: w, height: h }.draw(&rt.draw, &atlas.pixels, atlas.size, IRect::new(0, 0, w, h), bg);
        Image { width: w as u32, height: h as u32, pixels }
    }

    /// 最後のフレームで、文字から位置の決まる四角（x, y, 幅, 高さ。画素）: 文字の形の四角と、書式付きの文字の段落の四角。
    /// xtask の写真の比べが、PC ごとに入っている書体の違いによる違いだけを見分けるのに使う。書式付きの文字は、書体が
    /// 変わると後ろの文字と一緒に下線・取り消し線・コードの背景も動く（文字の形の外の画素も変わる）ので、段落ごと入れる
    #[doc(hidden)]
    pub fn glyph_boxes(&mut self) -> Vec<[i32; 4]> {
        let rt = self.cur();
        let snap = |b: Rect| {
            let (x, y) = (b.x.floor() as i32, b.y.floor() as i32);
            [x, y, (b.right().ceil() as i32) - x, (b.bottom().ceil() as i32) - y]
        };
        let glyphs = rt.draw.quads.iter().filter(|q| q.is_glyph()).map(|q| snap(q.bounds()));
        let paragraphs = (0..rt.tree.info.len())
            .filter(|&i| matches!(rt.tree.info[i].kind, crate::ui::Kind::RichText { .. }))
            .map(|i| snap(rt.tree.layout[i].rect));
        glyphs.chain(paragraphs).collect()
    }
}

/// An image drawn by [`Harness::render`]: pixels as `0xAARRGGBB`, row by row from the top.
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
/// let image = h.render();
/// assert_eq!(image.pixels.len(), (image.width * image.height) as usize);
/// ```
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
    /// let image = h.render();
    /// assert_eq!(image.rgb(1, 1), 0xffffff);
    /// ```
    pub fn rgb(&self, x: u32, y: u32) -> u32 {
        self.pixels[(y * self.width + x) as usize] & 0x00ff_ffff
    }

    /// Saves as PNG (uncompressed, for screenshots in documentation and tests).
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
    /// let path = std::env::temp_dir().join("kargui-example.png");
    /// h.render().save_png(&path).unwrap();
    /// assert!(std::fs::metadata(&path).unwrap().len() > 0);
    /// std::fs::remove_file(&path).unwrap();
    /// ```
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
    let text = Text::new().expect("the text engine starts");
    let mut rt = Runtime::new(text, false, Box::new(MemoryClipboard::default()), "example".into());
    rt.resize(900.0, 640.0, 1.0);
    let mut tree = crate::ui::Tree::new(crate::runtime::ROOT, 1.0);
    f(&mut Ui::new(&mut rt.cx, &mut tree));
    crate::ui::draw::layout(&mut tree, &mut rt.cx);
    crate::ui::draw::paint(&tree, &mut rt.cx, &mut rt.draw);
}

/// 部品 a の下に部品 t があるか（a 自身を含まない）
fn under(tree: &crate::ui::Tree, a: usize, t: usize) -> bool {
    tree.layout[a].children.iter().any(|&c| c == t || under(tree, c, t))
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
                if (field.submitted() || ui.button("Add").primary().clicked())
                    && !self.input.trim().is_empty() {
                        self.items.insert(0, (std::mem::take(&mut self.input), false));
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
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
        assert!(h.app().done);
        // 文字の外（右の空き）を押しても切り替わらない
        let (x, y) = (r.right() + 40.0, r.y + r.h / 2.0);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
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
        h.send(vec![Event::PointerDown { x: tx, y: t.y + 3.0, shift: false, count: 1 }]);
        step(&mut h, "つまみをつかむ");
        for k in 1..=6 {
            h.send(vec![Event::PointerMove { x: tx, y: t.y + 3.0 + k as f32 * 7.0 }]);
            step(&mut h, &format!("つまみを動かす {k} 回目"));
        }
        h.send(vec![Event::PointerMove { x: tx, y: l.rect.bottom() + 50.0 }]);
        step(&mut h, "つまみを下の端へ");
        h.send(vec![Event::PointerUp { x: tx, y: l.rect.bottom() + 50.0 }]);
        step(&mut h, "つまみを離す");
        h.send(vec![Event::PointerDown { x: tx, y: l.rect.y + 3.0, shift: false, count: 1 }, Event::PointerUp { x: tx, y: l.rect.y + 3.0 }]);
        step(&mut h, "棒の空いた所を押す");
        h.rt.cx.bar_hover = None;
        h.frame();
        step(&mut h, "棒から離れる");
    }

    #[test]
    fn drawing_only_changes_matches_drawing_everything_for_a_list_inside_a_scroll_area() {
        // 作者の手の確かめ（0.3.0 の 2 回目）で、部品の見本のスクロールバーが二重に見えた。見本のページは
        // スクロールする入れ物の中にあり、その中の一覧をホイールで動かすと、古いつまみの絵が残る
        struct Page {
            row: Option<usize>,
            below: usize,
        }
        impl App for Page {
            fn view(&mut self, ui: &mut Ui) {
                let (row, below) = (&mut self.row, self.below);
                ui.scroll(|ui| {
                    ui.label("Above");
                    ui.list(200, 28.0).height(crate::Size::Fixed(240.0)).selection(row).show(|ui, i| {
                        ui.label(format!("{i} row"));
                    });
                    for k in 0..below {
                        ui.label(format!("Below {k}"));
                    }
                })
                .grow();
            }
        }
        // 入れ物が中身より高い時（見本の一覧のページ）と、入れ物もスクロールする時
        for below in [1, 40] {
            let mut h = Harness::new(Page { row: None, below });
            let (w, hh) = (h.rt.cx.width as i32, h.rt.cx.height as i32);
            let mut px = vec![0u32; (w * hh) as usize];
            let mut summary = crate::damage::Summary::default();
            let mut step = |h: &mut Harness<Page>, what: &str| {
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
                assert_eq!(diff, 0, "下に {below} 行: {what}: {diff} 画素が違う。範囲 {area:?}（{}）", out.mode);
                if what.starts_with("一覧を") {
                    assert!(out.mode.starts_with("blits=1"), "{what}: 絵をずらす道を通る（{}）", out.mode);
                }
            };
            step(&mut h, "最初");
            let list = h.rt.cx.prev.lists.iter().find(|l| l.rect.h < 300.0).unwrap().rect;
            let (x, y) = (list.x + list.w / 2.0, list.y + list.h / 2.0);
            for k in 0..12 {
                h.send(vec![Event::Wheel { x, y, dy: 96.0 }]);
                step(&mut h, &format!("一覧を下へ {} 回目", k + 1));
            }
            for k in 0..6 {
                h.send(vec![Event::Wheel { x, y, dy: -64.0 }]);
                step(&mut h, &format!("一覧を上へ {} 回目", k + 1));
            }
            // 入れ物の方を動かす（一覧の外でホイール）
            let (ox, oy) = (list.x + list.w / 2.0, list.bottom() + 40.0);
            for k in 0..4 {
                h.send(vec![Event::Wheel { x: ox, y: oy, dy: 48.0 }]);
                step(&mut h, &format!("入れ物を下へ {} 回目", k + 1));
            }
        }
    }

    fn press<A: App>(h: &mut Harness<A>, x: f32, y: f32) {
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
    }
    fn drag_to<A: App>(h: &mut Harness<A>, x: f32, y: f32) {
        h.send(vec![Event::PointerMove { x, y }]);
    }
    fn release<A: App>(h: &mut Harness<A>, x: f32, y: f32) {
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

    /// 一覧の行をアプリの id で区別する版と、上から何行目かで区別する版
    struct Shelf {
        items: Vec<(u32, String, bool)>,
        keyed: bool,
    }

    impl App for Shelf {
        fn view(&mut self, ui: &mut Ui) {
            let items = &mut self.items;
            let row = |ui: &mut Ui, i: usize, items: &mut Vec<(u32, String, bool)>| {
                let (_, name, done) = &mut items[i];
                ui.checkbox(done, name.clone());
            };
            let n = items.len();
            if self.keyed {
                let ids: Vec<u32> = items.iter().map(|t| t.0).collect();
                ui.list(n, 32.0).grow().show_keyed(|i| ids[i], |ui, i| row(ui, i, items));
            } else {
                ui.list(n, 32.0).grow().show(|ui, i| row(ui, i, items));
            }
        }
    }

    /// 見出しの下に、窓より高い設定の並びをスクロールの入れ物に入れた画面
    struct Settings {
        options: Vec<bool>,
    }

    impl App for Settings {
        fn view(&mut self, ui: &mut Ui) {
            ui.heading("Settings");
            ui.scroll(|ui| {
                for (i, on) in self.options.iter_mut().enumerate() {
                    ui.checkbox(on, format!("Option {i}"));
                }
                ui.label("The end");
            });
        }
    }

    /// 目次から章へ飛ぶ読み物
    struct Reader {
        jump: Option<usize>,
    }

    impl App for Reader {
        fn view(&mut self, ui: &mut Ui) {
            ui.scroll(|ui| {
                for ch in 0..8 {
                    let heading = ui.heading(format!("Chapter {ch}"));
                    if self.jump == Some(ch) {
                        heading.scroll_into_view();
                    }
                    for p in 0..6 {
                        ui.label(format!("Paragraph {ch}.{p}"));
                    }
                }
            });
            self.jump = None;
        }
    }

    #[test]
    fn scroll_into_view_puts_the_widget_at_the_top() {
        let mut h = Harness::with_size(Reader { jump: None }, 400.0, 300.0, 1.0);
        h.frame();
        let area = h.rt.cx.prev.lists[0].rect;
        h.app_mut().jump = Some(3);
        h.frame();
        assert_eq!(h.find("Chapter 3").unwrap().y, area.y, "3 章の見出しが入れ物の上端に来る");
        // 最後の章は中身の端で止まる（上端までは来ない）
        h.app_mut().jump = Some(7);
        h.frame();
        let end = h.find("Paragraph 7.5").unwrap();
        assert!((end.bottom() - area.bottom()).abs() <= 1.0, "最後の段落 {end:?} が下端 {area:?}");
        // 1 章に戻る
        h.app_mut().jump = Some(1);
        h.frame();
        assert_eq!(h.find("Chapter 1").unwrap().y, area.y);
    }

    struct Form {
        text: String,
    }

    impl App for Form {
        fn view(&mut self, ui: &mut Ui) {
            ui.scroll(|ui| {
                ui.text_input(&mut self.text).hint("Name");
                for i in 0..40 {
                    ui.label(format!("Line {i}"));
                }
            });
        }
    }

    #[test]
    fn the_scroll_bar_does_not_cover_the_content() {
        // 段階 3 の設定画面と読む画面の写真で、スクロールの棒が入力欄の右端と行の終わりの文字に重なっていた
        let mut h = Harness::with_size(Form { text: String::new() }, 400.0, 300.0, 1.0);
        h.frame();
        let field = h.find("Name").unwrap();
        let thumb = h.rt.cx.prev.lists[0].thumb.expect("中身が高いので棒がある");
        assert!(field.right() + 2.0 <= thumb.x, "入力欄 {field:?} と棒 {thumb:?} が重なる");
    }

    #[test]
    fn wrapped_text_in_a_column_does_not_push_the_next_widget_to_the_bottom() {
        let mut h = Harness::with_size(Wrapped, 400.0, 400.0, 1.0);
        h.frame();
        let text = h.find("A short paragraph that wraps onto two lines in a narrow window, maybe.").unwrap();
        let ok = h.find("OK").unwrap();
        // 文の高さは 2〜3 行分（残りの高さいっぱいにならない）で、そのすぐ下にボタンがある
        assert!(text.h < 80.0, "文の高さ {text:?}");
        assert!(ok.y - text.bottom() < 20.0, "文 {text:?} のすぐ下にボタン {ok:?}");
    }

    struct Wrapped;

    impl App for Wrapped {
        fn view(&mut self, ui: &mut Ui) {
            ui.column(|ui| {
                ui.label("A short paragraph that wraps onto two lines in a narrow window, maybe.").wrap();
                ui.button("OK");
            })
            .grow();
        }
    }

    /// 数の表示を先に描き、絞り込みの作り直しを後でする（AI の試験で AI が書いた形）
    struct Late {
        items: Vec<u32>,
        shown: Vec<u32>,
        dirty: bool,
    }

    impl App for Late {
        fn view(&mut self, ui: &mut Ui) {
            ui.label(format!("Showing {} of {}", self.shown.len(), self.items.len()));
            if self.dirty {
                self.shown = self.items.clone();
                self.dirty = false;
            }
            if ui.button("Clear all").clicked() {
                self.items.clear();
                self.dirty = true;
            }
        }
    }

    #[test]
    fn values_computed_later_in_view_show_up_in_the_same_frame() {
        // AI の試験で、Clear all の後に「Showing 3 of 0」が次の入力まで残った（描く回の途中で作り直していた）
        let mut h = Harness::new(Late { items: vec![1, 2, 3], shown: Vec::new(), dirty: true });
        h.frame();
        assert!(h.names().contains(&"Showing 3 of 3".to_string()), "{:?}", h.names());
        h.click("Clear all");
        assert!(h.names().contains(&"Showing 0 of 0".to_string()), "押した同じフレームで新しい数が出る: {:?}", h.names());
    }

    /// 一覧の行の中に、もう 1 つ行を入れて fill を使う（AI の試験で AI が書いた形）
    struct Nested {
        dark_seen: Option<bool>,
    }

    impl App for Nested {
        fn view(&mut self, ui: &mut Ui) {
            ui.dark_mode_switch("Dark");
            self.dark_seen = Some(ui.dark());
            ui.list(3, 32.0).grow().show(|ui, i| {
                ui.row(|ui| {
                    ui.label(format!("Task {i}"));
                    ui.fill();
                    ui.button("Delete").small().label(format!("Delete {i}"));
                });
            });
        }
    }

    #[test]
    fn fill_inside_a_nested_row_reaches_the_end_of_the_outer_row() {
        // AI の試験で、行の中の行に fill を入れても、Delete が行の右端ではなく文字のすぐ右に並んだ
        let mut h = Harness::with_size(Nested { dark_seen: None }, 400.0, 300.0, 1.0);
        h.frame();
        let list = h.rt.cx.prev.lists[0].rect;
        let delete = h.find("Delete 0").unwrap();
        assert!(list.right() - delete.right() <= 20.0, "Delete {delete:?} が一覧 {list:?} の右端にある");
    }

    #[test]
    fn the_dark_mode_switch_follows_and_changes_the_frameworks_look() {
        // AI の試験で、アプリが自分の bool（false）で始めたため、OS がダークだとスイッチが見た目と逆を示した
        let mut h = Harness::new(Nested { dark_seen: None });
        h.rt.cx.set_dark(true); // OS がダーク
        h.frame();
        let on = |h: &Harness<Nested>| h.rt.tree.info.iter().find_map(|n| match &n.kind {
            crate::ui::Kind::Toggle { on, text, .. } if text == "Dark" => Some(*on),
            _ => None,
        });
        assert_eq!(on(&h), Some(true), "スイッチは今の見た目（ダーク）を示す");
        h.click("Dark");
        assert_eq!(h.app().dark_seen, Some(false));
        assert_eq!(on(&h), Some(false));
    }

    fn settings() -> Harness<Settings> {
        let mut h = Harness::with_size(Settings { options: vec![false; 30] }, 400.0, 300.0, 1.0);
        h.frame();
        h
    }

    #[test]
    fn scroll_moves_its_content_with_the_wheel_and_stops_at_the_end() {
        let mut h = settings();
        let area = h.rt.cx.prev.lists[0].rect;
        let y0 = h.find("Option 1").unwrap().y;
        h.scroll("Option 1", 100.0);
        assert_eq!(h.find("Option 1").unwrap().y, y0 - 100.0);
        // ずっと下まで回すと、最後の行が入れ物の下端で止まる
        h.scroll("Option 5", 10_000.0);
        let end = h.find("The end").unwrap();
        assert!((end.bottom() - area.bottom()).abs() <= 1.0, "最後 {end:?} / 入れ物 {area:?}");
        // 入れ物の上に隠れた行は押せない（見出しの所を押すと、見出しの下の行ではなく何も当たらない）
        let heading = h.find("Settings").unwrap();
        h.send(vec![Event::PointerDown { x: heading.x + 4.0, y: heading.y + 4.0, shift: false, count: 1 }, Event::PointerUp { x: heading.x + 4.0, y: heading.y + 4.0 }]);
        assert!(h.app().options.iter().all(|o| !o), "隠れた行が押された: {:?}", h.app().options);
    }

    #[test]
    fn tab_reaches_widgets_below_the_scroll_and_shows_them() {
        let mut h = settings();
        let area = h.rt.cx.prev.lists[0].rect;
        for _ in 0..21 {
            h.press(Key::Tab);
        }
        let r = h.find("Option 20").unwrap();
        assert!(r.y >= area.y && r.bottom() <= area.bottom(), "Option 20 {r:?} が入れ物 {area:?} の中に見える");
        h.press(Key::Space);
        assert!(h.app().options[20], "焦点は Option 20 にある");
    }

    #[test]
    fn drawing_only_changes_matches_drawing_everything_in_a_scroll() {
        let mut h = settings();
        let (w, hh) = (h.rt.cx.width as i32, h.rt.cx.height as i32);
        let mut px = vec![0u32; (w * hh) as usize];
        let mut summary = crate::damage::Summary::default();
        let area = h.rt.cx.prev.lists[0].rect;
        let (x, y) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
        for k in 0..16 {
            let bg = h.rt.cx.theme.background;
            let out = crate::cpu_frame::draw(&mut px, w, hh, &h.rt.draw, &h.rt.cx.text.atlas, bg, &summary, false);
            summary = out.summary;
            let full = h.render();
            let diff = px.iter().zip(&full.pixels).filter(|(a, b)| a != b).count();
            assert_eq!(diff, 0, "{k} 回目: {diff} 画素が違う（{}）", out.mode);
            h.send(vec![Event::Wheel { x, y, dy: if k < 10 { 37.0 } else { -53.0 } }]);
        }
    }

    struct Prefs {
        size: i32,
        volume: f32,
    }

    impl App for Prefs {
        fn view(&mut self, ui: &mut Ui) {
            ui.row(|ui| {
                ui.label("Font size");
                ui.slider(&mut self.size, 10..=24).label("Text size");
            });
            ui.row(|ui| {
                ui.label("Volume");
                ui.slider(&mut self.volume, 0.0..=1.0).label("Volume level");
            });
        }
    }

    #[test]
    fn sliders_follow_the_keyboard_the_pointer_and_screen_readers() {
        let mut h = Harness::new(Prefs { size: 12, volume: 0.5 });
        h.frame();
        // キー: Tab で文字の大きさのスライダーへ。→ 3 回で 15、End で 24、Home で 10、端で止まる
        h.press(Key::Tab).press(Key::Right).press(Key::Right).press(Key::Right);
        assert_eq!(h.app().size, 15);
        h.press(Key::End);
        assert_eq!(h.app().size, 24);
        h.press(Key::Home).press(Key::Left);
        assert_eq!(h.app().size, 10);
        // ポインター: 音量のスライダーの左端を押し、真ん中、右の外まで動かす
        let r = h.find("Volume level").unwrap();
        let y = r.y + r.h / 2.0;
        h.send(vec![Event::PointerDown { x: r.x + 1.0, y, shift: false, count: 1 }]);
        assert!(h.app().volume < 0.01, "左端で 0: {}", h.app().volume);
        h.send(vec![Event::PointerMove { x: r.x + r.w / 2.0, y }]);
        assert!((h.app().volume - 0.5).abs() < 0.01, "真ん中で 0.5: {}", h.app().volume);
        h.send(vec![Event::PointerMove { x: r.right() + 50.0, y }]);
        assert_eq!(h.app().volume, 1.0, "外へ出ても端で止まる");
        h.send(vec![Event::PointerUp { x: r.right() + 50.0, y }]);
        h.send(vec![Event::PointerMove { x: r.x, y }]);
        assert_eq!(h.app().volume, 1.0, "離した後の動きは効かない");
        // 押してすぐ離す（同じフレームに届く）だけでも、その位置の値になる
        h.send(vec![Event::PointerDown { x: r.x + r.w / 2.0, y, shift: false, count: 1 }, Event::PointerUp { x: r.x + r.w / 2.0, y }]);
        assert!((h.app().volume - 0.5).abs() < 0.01, "クリックで 0.5: {}", h.app().volume);
        // 読み上げソフトの「増やす」
        let i = (0..h.rt.tree.info.len()).find(|&i| a11y::name_of(&h.rt.tree, i).as_deref() == Some("Text size")).unwrap();
        let target = h.rt.tree.info[i].id;
        h.send(vec![Event::A11y { target, action: crate::input::A11yAction::Increment }]);
        assert_eq!(h.app().size, 11);
    }

    struct Table {
        rows: Vec<(u32, String)>,
        sel: Option<usize>,
        query: String,
    }

    impl App for Table {
        fn view(&mut self, ui: &mut Ui) {
            ui.text_input(&mut self.query).hint("Search");
            let rows = &self.rows;
            ui.list(rows.len(), 30.0).grow().selection(&mut self.sel).show_keyed(
                |i| rows[i].0,
                |ui, i| {
                    ui.label(&rows[i].1);
                },
            );
        }
    }

    #[test]
    fn selectable_lists_move_with_arrows_and_are_one_tab_stop() {
        let rows = (0..100).map(|i| (1000 + i, format!("Row {i}"))).collect();
        let mut h = Harness::with_size(Table { rows, sel: None, query: String::new() }, 400.0, 300.0, 1.0);
        h.frame();
        h.click("Row 2");
        assert_eq!(h.app().sel, Some(2));
        h.press(Key::Down).press(Key::Down);
        assert_eq!(h.app().sel, Some(4));
        // End で最後の行へ。見えるようにスクロールする
        h.press(Key::End);
        assert_eq!(h.app().sel, Some(99));
        let list = h.rt.cx.prev.lists[0].rect;
        let r = h.find("Row 99").unwrap();
        assert!(r.y >= list.y && r.bottom() <= list.bottom() + 0.5, "Row 99 {r:?} が一覧 {list:?} の中に見える");
        h.press(Key::Home).press(Key::Up);
        assert_eq!(h.app().sel, Some(0), "上の端で止まる");
        // 選んだ行は読み上げでも「選ばれている」
        let i = (0..h.rt.tree.info.len()).find(|&i| a11y::name_of(&h.rt.tree, i).as_deref() == Some("Row 0")).unwrap();
        assert!(matches!(h.rt.tree.info[i].kind, crate::ui::Kind::Item { selected: true, .. }));
        // Tab: 検索の欄から 1 回で一覧（選んだ行）へ、もう 1 回で一覧の外（検索の欄に戻る）へ
        h.click("Search");
        h.press(Key::Tab).press(Key::Down);
        assert_eq!(h.app().sel, Some(1), "Tab で一覧に移った");
        h.press(Key::Tab).type_text("x");
        assert_eq!(h.app().query, "x", "次の Tab で一覧を出た");
    }

    fn id_of(h: &Harness<Shelf>, name: &str) -> crate::ui::Id {
        let i = (0..h.rt.tree.info.len()).find(|&i| a11y::name_of(&h.rt.tree, i).as_deref() == Some(name)).unwrap();
        h.rt.tree.info[i].id
    }

    #[test]
    fn keyed_rows_keep_their_ids_when_an_item_is_added_above() {
        // 段階 2 の手の確かめで、先頭にタスクを足すと見えている全部の行で「名前が変わった」出来事が出た
        for keyed in [true, false] {
            let items = vec![(7, "Milk".to_string(), false), (3, "Bread".to_string(), false)];
            let mut h = Harness::new(Shelf { items, keyed });
            h.frame();
            let (milk, bread) = (id_of(&h, "Milk"), id_of(&h, "Bread"));
            h.app_mut().items.insert(0, (9, "Eggs".to_string(), false));
            h.frame();
            let same = id_of(&h, "Milk") == milk && id_of(&h, "Bread") == bread;
            assert_eq!(same, keyed, "keyed={keyed}: 行の id が{}", if same { "変わらなかった" } else { "変わった" });
        }
        // 焦点も、行が下にずれても同じ品物についていく
        let items = vec![(7, "Milk".to_string(), false)];
        let mut h = Harness::new(Shelf { items, keyed: true });
        h.click("Milk");
        h.app_mut().items.insert(0, (9, "Eggs".to_string(), false));
        // 押した時に Milk は入になっている。Space は焦点のある Milk を切り替え、切に戻す（Eggs は変わらない）
        assert!(h.app().items[1].2);
        h.frame().press(Key::Space);
        assert!(!h.app().items[1].2, "Space で Milk（2 行目に移った）が切り替わる");
        assert!(!h.app().items[0].2, "Eggs は変わらない");
    }

    #[test]
    fn the_candidate_window_does_not_cover_the_thick_underline() {
        // 段階 2 の 2 回目の写真で、候補の窓の上の縁が太い下線の下の 1 画素を隠し、細い下線と見分けられなかった。
        // IME は候補の窓を、答えた位置（preedit_rect の上）から答えた行の高さ（切り捨て）だけ下に出す
        let mut h = book(0);
        h.click("New item");
        let clauses = Event::ImeClauses { bounds: vec![0, 3, 6, 8], target: Some((3, 6)), cursor: Some(6) };
        h.send(vec![Event::ImePreedit("明日は東京に行く".into()), clauses]);
        let f = h.rt.cx.ime_field.clone().unwrap();
        let p = h.rt.cx.preedit_rect.unwrap();
        let img = h.render();
        let window_top = p.y as u32 + p.h.max(1.0) as u32;
        let dark = |x: u32, y: u32| {
            let c = img.rgb(x, y);
            765 - (((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff))
        };
        // 文節の幅いっぱいにほぼ黒い行が下線（文字の線は幅いっぱいにはならない）
        let solid_rows = |a: usize, b: usize| -> Vec<u32> {
            let (x1, x2) = (f.preedit_xs[a] as u32 + 1, f.preedit_xs[b] as u32 - 3);
            (p.y as u32..window_top + 4).filter(|&y| (x1..x2).all(|x| dark(x, y) > 500)).collect()
        };
        let (thin, thick) = (solid_rows(0, 3), solid_rows(3, 6));
        assert_eq!(thin.len(), 1, "細い下線は 1 行: {thin:?}");
        assert_eq!(thick.len(), 2, "太い下線は 2 行: {thick:?}");
        assert!(thick.iter().all(|&y| y < window_top), "候補の窓（{window_top} 行目から）が下線 {thick:?} を隠す");
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
            h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
            assert_ne!(h.app().done[0], before, "({x}, {y}) が当たらない");
        }
        // 2 行目の本来の範囲は、1 行目の余白より強い
        let r2 = h.find("00002 企画書を作る").unwrap();
        let (x, y) = (r2.x + 4.0, r2.y + 1.0);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
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

    /// 押せない部品の見本: Ui::enabled の範囲の 6 つの部品と、範囲の外のボタン、Button::enabled を付けたボタン
    #[derive(Default)]
    struct Locked {
        locked: bool,
        clicks: u32,
        done: bool,
        on: bool,
        level: u32,
        name: String,
        pick: u8,
        others: u32,
        can_save: bool,
        saves: u32,
    }

    impl App for Locked {
        fn view(&mut self, ui: &mut Ui) {
            ui.enabled(!self.locked, |ui| {
                if ui.button("Go").clicked() {
                    self.clicks += 1;
                }
                ui.checkbox(&mut self.done, "Done");
                ui.toggle(&mut self.on, "On");
                ui.slider(&mut self.level, 0..=10).label("Level");
                ui.text_input(&mut self.name).hint("Name");
                ui.segmented(&mut self.pick, &[(0, "A"), (1, "B")]);
            });
            if ui.button("Other").clicked() {
                self.others += 1;
            }
            if ui.button("Save").enabled(self.can_save).clicked() {
                self.saves += 1;
            }
        }
    }

    use crate::input::A11yAction;

    fn focused_name<A: App>(h: &Harness<A>) -> Option<String> {
        let f = h.rt.cx.focus?;
        (0..h.rt.tree.info.len()).find(|&i| h.rt.tree.info[i].id == f).and_then(|i| a11y::name_of(&h.rt.tree, i))
    }

    fn id_named<A: App>(h: &Harness<A>, name: &str) -> crate::ui::Id {
        let i = (0..h.rt.tree.info.len()).find(|&i| a11y::name_of(&h.rt.tree, i).as_deref() == Some(name)).unwrap();
        h.rt.tree.info[i].id
    }

    #[test]
    fn disabled_widgets_ignore_pointer_and_keys() {
        let mut h = Harness::new(Locked { locked: true, ..Default::default() });
        h.click("Go").click("Done").click("On").click("Level").click("B");
        h.click("Name").type_text("x");
        let a = h.app();
        assert_eq!((a.clicks, a.done, a.on, a.level, a.pick, a.name.as_str()), (0, false, false, 0, 0, ""));
        // Tab は押せる部品（Other）だけに止まる。Save も押せない
        for _ in 0..3 {
            h.press(Key::Tab);
            assert_eq!(focused_name(&h).as_deref(), Some("Other"));
        }
        h.press(Key::Space);
        assert_eq!(h.app().others, 1);
        assert_eq!(h.enabled("Go"), Some(false));
        assert_eq!(h.enabled("Name"), Some(false));
        assert_eq!(h.enabled("Other"), Some(true));
        assert_eq!(h.enabled("Save"), Some(false));
        assert_eq!(h.enabled("Missing"), None);
    }

    #[test]
    fn enabling_again_restores_input() {
        let mut h = Harness::new(Locked { locked: true, ..Default::default() });
        h.app_mut().locked = false;
        h.app_mut().can_save = true;
        h.frame();
        h.click("Go").click("Done").click("Save");
        let a = h.app();
        assert_eq!((a.clicks, a.done, a.saves), (1, true, 1));
        assert_eq!(h.enabled("Go"), Some(true));
    }

    #[test]
    fn focused_widget_that_becomes_disabled_loses_focus() {
        let mut h = Harness::new(Locked { can_save: true, ..Default::default() });
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Go"));
        h.app_mut().locked = true;
        h.frame();
        assert_eq!(focused_name(&h), None);
        h.press(Key::Enter);
        assert_eq!(h.app().clicks, 0);
        // Button::enabled でも同じ
        h.click("Save");
        assert_eq!(h.app().saves, 1);
        h.app_mut().can_save = false;
        h.frame();
        assert_eq!(focused_name(&h), None);
        h.press(Key::Space);
        assert_eq!(h.app().saves, 1);
    }

    #[test]
    fn screen_readers_see_disabled_widgets_and_cannot_use_them() {
        let mut h = Harness::new(Locked { locked: true, ..Default::default() });
        let built = a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new());
        let node = |name: &str| {
            let id = id_named(&h, name);
            built.update.nodes.iter().find(|(n, _)| n.0 == id.0).map(|(_, n)| n.clone()).unwrap()
        };
        for name in ["Go", "Done", "On", "Level", "Name", "B"] {
            let n = node(name);
            assert!(n.is_disabled(), "{name} は押せないと伝える");
            assert!(!n.supports_action(accesskit::Action::Click) && !n.supports_action(accesskit::Action::Focus), "{name} は操作を出さない");
        }
        assert!(!node("Other").is_disabled());
        // 古い木を見た読み上げソフトから届いた操作も効かない
        for (name, action) in [("Go", A11yAction::Click), ("Done", A11yAction::Click), ("Level", A11yAction::Increment), ("Name", A11yAction::Focus)] {
            let target = id_named(&h, name);
            h.send(vec![Event::A11y { target, action }]);
        }
        let a = h.app();
        assert_eq!((a.clicks, a.done, a.level), (0, false, 0));
        assert_eq!(focused_name(&h), None);
    }

    #[test]
    fn disabled_widgets_are_drawn_dimmed() {
        // 文字の一番濃い画素の明るさ（明るい表示なので、押せない方が薄い）
        fn darkest(h: &mut Harness<Locked>, name: &str) -> u32 {
            let r = h.find(name).unwrap();
            let image = h.render();
            let mut min = u32::MAX;
            for y in r.y as u32..(r.y + r.h) as u32 {
                for x in r.x as u32..(r.x + r.w) as u32 {
                    let c = image.rgb(x, y);
                    min = min.min(((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff));
                }
            }
            min
        }
        let mut on = Harness::new(Locked::default());
        let mut off = Harness::new(Locked { locked: true, ..Default::default() });
        for name in ["Go", "Done", "Name"] {
            let (a, b) = (darkest(&mut on, name), darkest(&mut off, name));
            // 入力欄は空なので、もともと薄い案内の文字を比べる（差が小さめ）
            assert!(b >= a + 120, "{name}: 押せる {a}、押せない {b}");
        }
    }

    /// 重ねて出す層の見本: 「More」で開く浮き出し、「Open」で開くダイアログ、後ろの「Behind」、ツールチップ付きの「Save」
    #[derive(Default)]
    struct Layers {
        menu: bool,
        renamed: u32,
        dialog: bool,
        name: String,
        behind: u32,
        /// 部品の行を窓の下の端に置く（浮き出しが上に出るか確かめる）
        bottom: bool,
    }

    impl App for Layers {
        fn view(&mut self, ui: &mut Ui) {
            if self.bottom {
                ui.fill();
            }
            ui.row(|ui| {
                let more = ui.button("More");
                let anchor = more.id();
                if more.clicked() {
                    self.menu = !self.menu;
                }
                ui.popup(&mut self.menu, anchor, |ui| {
                    if ui.button("Rename").clicked() {
                        self.renamed += 1;
                    }
                    ui.button("Delete");
                });
                if ui.button("Open").clicked() {
                    self.dialog = true;
                }
                if ui.button("Behind").clicked() {
                    self.behind += 1;
                }
                ui.button("Save").tooltip("Saves the file");
            });
            ui.label("Body");
            let mut cancel = false;
            ui.dialog(&mut self.dialog, "Rename item", |ui| {
                ui.text_input(&mut self.name).hint("Name");
                ui.row(|ui| {
                    ui.button("OK");
                    cancel = ui.button("Cancel").clicked();
                });
            });
            if cancel {
                self.dialog = false;
            }
        }
    }

    #[test]
    fn popup_opens_below_its_anchor_takes_focus_and_closes_outside_or_on_escape() {
        let mut h = Harness::new(Layers::default());
        assert!(h.find("Rename").is_none());
        h.click("More");
        let (more, rename) = (h.find("More").unwrap(), h.find("Rename").unwrap());
        assert!(rename.y >= more.bottom(), "下に出る: {more:?} {rename:?}");
        assert_eq!(focused_name(&h).as_deref(), Some("Rename"), "開くと中へ焦点が移る");
        // 中の部品は押せる。押しても閉じない（閉じるかはアプリが決める）
        h.click("Rename");
        assert_eq!(h.app().renamed, 1);
        assert!(h.app().menu);
        // 外を押すと閉じ、焦点は開いた部品へ戻る。閉じるためのクリックは下の部品に届かない
        h.click("Behind");
        assert!(!h.app().menu);
        assert_eq!(h.app().behind, 0);
        assert!(h.find("Rename").is_none());
        assert_eq!(focused_name(&h).as_deref(), Some("More"));
        // Esc でも閉じる
        h.click("More");
        h.press(Key::Escape);
        assert!(!h.app().menu);
        assert_eq!(focused_name(&h).as_deref(), Some("More"));
        // 開いた部品をもう一度押すと閉じる（閉じてすぐ開き直さない）
        h.click("More").click("More");
        assert!(!h.app().menu);
    }

    #[test]
    fn popup_near_the_bottom_opens_above_and_stays_in_the_window() {
        let mut h = Harness::with_size(Layers { bottom: true, ..Default::default() }, 600.0, 300.0, 1.0);
        h.click("More");
        let (more, rename, delete) = (h.find("More").unwrap(), h.find("Rename").unwrap(), h.find("Delete").unwrap());
        assert!(delete.bottom() <= more.y, "上に出る: {more:?} {delete:?}");
        assert!(rename.y >= 0.0 && delete.bottom() <= 300.0);
    }

    #[test]
    fn dialog_blocks_what_is_behind_traps_focus_and_closes_on_escape() {
        let mut h = Harness::new(Layers::default());
        h.click("Open");
        assert!(h.app().dialog);
        assert_eq!(focused_name(&h).as_deref(), Some("Name"), "最初の部品へ焦点");
        // 後ろの部品は押せない（ダイアログは開いたまま、焦点も動かない）
        h.click("Behind");
        assert_eq!(h.app().behind, 0);
        assert!(h.app().dialog);
        assert_eq!(focused_name(&h).as_deref(), Some("Name"));
        // Tab はダイアログの中だけを回る
        let mut seen = Vec::new();
        for _ in 0..4 {
            h.press(Key::Tab);
            seen.push(focused_name(&h).unwrap());
        }
        assert_eq!(seen, ["OK", "Cancel", "Name", "OK"]);
        h.click("Name").type_text("x");
        assert_eq!(h.app().name, "x");
        // Esc で閉じ、焦点は開いた部品へ戻る
        h.press(Key::Escape);
        assert!(!h.app().dialog);
        assert_eq!(focused_name(&h).as_deref(), Some("Open"));
        // アプリが閉じた時も戻る
        h.click("Open").click("Cancel");
        assert!(!h.app().dialog);
        assert_eq!(focused_name(&h).as_deref(), Some("Open"));
    }

    #[test]
    fn tooltip_shows_after_the_pointer_rests_and_on_keyboard_focus() {
        let mut h = Harness::new(Layers::default());
        let save = h.find("Save").unwrap();
        h.hover("Save");
        assert!(h.find("Saves the file").is_none(), "すぐには出ない");
        h.wait(std::time::Duration::from_millis(600));
        let tip = h.find("Saves the file").expect("止まると出る");
        assert!(tip.y >= save.bottom(), "下に出る: {save:?} {tip:?}");
        h.press(Key::Escape);
        assert!(h.find("Saves the file").is_none(), "Esc で消える");
        // ポインターが離れたあと、キーボードで焦点を当てると出る
        h.rt.cx.set_hover(None);
        for _ in 0..4 {
            h.press(Key::Tab);
        }
        assert_eq!(focused_name(&h).as_deref(), Some("Save"));
        assert!(h.find("Saves the file").is_some(), "キーボードの焦点で出る");
        h.press(Key::Tab);
        assert!(h.find("Saves the file").is_none(), "焦点が離れると消える");
    }

    #[test]
    fn layers_are_in_the_screen_reader_tree() {
        let mut h = Harness::new(Layers::default());
        h.click("More");
        h.app_mut().dialog = true;
        h.frame();
        let built = a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new());
        // 根から全部の部品へたどれる
        let nodes: std::collections::HashMap<u64, &accesskit::Node> = built.update.nodes.iter().map(|(id, n)| (id.0, n)).collect();
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![crate::runtime::ROOT.0];
        while let Some(id) = stack.pop() {
            if seen.insert(id) {
                stack.extend(nodes[&id].children().iter().map(|c| c.0));
            }
        }
        assert_eq!(seen.len(), nodes.len(), "たどれない部品がある");
        let node = |name: &str| nodes[&id_named(&h, name).0];
        assert_eq!(node("Save").description(), Some("Saves the file"));
        let dialog = built.update.nodes.iter().find(|(_, n)| n.role() == accesskit::Role::Dialog).expect("ダイアログの役割").1.clone();
        assert!(dialog.is_modal());
        assert_eq!(dialog.label(), Some("Rename item"));
        assert!(nodes.contains_key(&id_named(&h, "Rename").0));
    }

    /// アプリのキー（ショートカット）の見本
    #[derive(Default)]
    struct Keys {
        note: String,
        saves: u32,
        refreshes: u32,
        plain_a: u32,
        all: u32,
        locked: u32,
    }

    impl App for Keys {
        fn view(&mut self, ui: &mut Ui) {
            if ui.shortcut(Key::S, Mods::CTRL) {
                self.saves += 1;
            }
            if ui.shortcut(Key::F5, Mods::NONE) {
                self.refreshes += 1;
            }
            if ui.shortcut(Key::A, Mods::NONE) {
                self.plain_a += 1;
            }
            if ui.shortcut(Key::A, Mods::CTRL) {
                self.all += 1;
            }
            ui.enabled(false, |ui| {
                if ui.shortcut(Key::S, Mods::CTRL) {
                    self.locked += 1;
                }
            });
            ui.text_input(&mut self.note).hint("Note");
            ui.button("Other");
        }
    }

    #[test]
    fn shortcuts_reach_the_app_unless_a_widget_uses_the_key() {
        let mut h = Harness::new(Keys::default());
        // 焦点が無い時は全部アプリへ
        h.press_with(Key::S, Mods::CTRL).press(Key::A).press_with(Key::A, Mods::CTRL).press(Key::F5);
        let a = h.app();
        assert_eq!((a.saves, a.plain_a, a.all, a.refreshes), (1, 1, 1, 1));
        // 押せない範囲の中では受け取らない
        assert_eq!(h.app().locked, 0);
        // 入力欄に焦点がある時: 入力欄が使うキー（A、Ctrl+A）は入力欄へ、使わないキー（Ctrl+S、F5）はアプリへ
        h.click("Note").type_text("abc");
        h.press(Key::A).press_with(Key::A, Mods::CTRL).press_with(Key::S, Mods::CTRL).press(Key::F5);
        let a = h.app();
        assert_eq!((a.saves, a.plain_a, a.all, a.refreshes), (2, 1, 1, 2));
        assert_eq!(a.note, "abc", "Ctrl+A は入力欄の全部を選ぶだけ");
        // ボタンに焦点がある時: Space はボタンへ、Ctrl+S はアプリへ
        h.press(Key::Tab);
        h.press(Key::Space).press_with(Key::S, Mods::CTRL | Mods::SHIFT).press_with(Key::S, Mods::CTRL);
        assert_eq!(h.app().saves, 3, "Ctrl+Shift+S は Ctrl+S と別");
    }

    #[test]
    fn mods_combine() {
        let m = Mods::CTRL | Mods::SHIFT;
        assert!(m.ctrl && m.shift && !m.alt);
        assert_eq!(Mods::NONE, Mods::default());
    }

    /// 1 秒たったら「Done」を出す見本（タイマー）
    #[derive(Default)]
    struct Timer {
        start: Option<std::time::Instant>,
    }

    impl App for Timer {
        fn view(&mut self, ui: &mut Ui) {
            let start = *self.start.get_or_insert(ui.now());
            let wait = std::time::Duration::from_secs(1);
            if ui.now() >= start + wait {
                ui.label("Done");
            } else {
                ui.label("Waiting");
                ui.request_frame_after(start + wait - ui.now());
            }
        }
    }

    #[test]
    fn request_frame_after_wakes_the_app_later() {
        let mut h = Harness::new(Timer::default());
        assert!(h.find("Waiting").is_some());
        // 描き直しを続けずに、1 秒後に起こしてもらう
        let out = h.rt.frame(&mut h.app, Vec::new());
        assert!(!out.want_frame);
        let after = out.wake_after.expect("起こす時間");
        assert!(after > std::time::Duration::from_millis(900) && after <= std::time::Duration::from_secs(1), "{after:?}");
        h.wait(std::time::Duration::from_millis(500));
        assert!(h.find("Waiting").is_some());
        h.wait(std::time::Duration::from_millis(600));
        assert!(h.find("Done").is_some());
        let out = h.rt.frame(&mut h.app, Vec::new());
        assert_eq!(out.wake_after, None, "もう起こさない");
    }

    #[test]
    fn tooltip_waits_with_a_timer_instead_of_drawing_every_frame() {
        let mut h = Harness::new(Layers::default());
        h.hover("Save");
        let out = h.rt.frame(&mut h.app, Vec::new());
        assert!(!out.want_frame, "待つ間に描き直しを続けない");
        let after = out.wake_after.expect("出す時刻に起こす");
        assert!(after <= crate::ui::context::TOOLTIP_DELAY, "{after:?}");
        h.wait(crate::ui::context::TOOLTIP_DELAY);
        assert!(h.find("Saves the file").is_some());
    }

    /// ほかのスレッドから届いた物を出す見本（起こし役）
    struct Inbox {
        rx: std::sync::mpsc::Receiver<String>,
        waker: Option<crate::Waker>,
        got: Vec<String>,
    }

    impl App for Inbox {
        fn view(&mut self, ui: &mut Ui) {
            if self.waker.is_none() {
                self.waker = Some(ui.waker());
            }
            self.got.extend(self.rx.try_iter());
            for g in &self.got {
                ui.label(g);
            }
        }
    }

    #[test]
    fn waker_wakes_the_window_from_another_thread() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Harness::new(Inbox { rx, waker: None, got: Vec::new() });
        let waker = h.app().waker.clone().unwrap();
        assert!(!h.rt.cx.waker.take_woken());
        std::thread::spawn(move || {
            tx.send("hello from a thread".to_string()).unwrap();
            waker.wake();
        })
        .join()
        .unwrap();
        assert!(h.rt.cx.waker.take_woken(), "起こされた印");
        assert!(!h.rt.cx.waker.take_woken(), "印は 1 回で消える");
        h.frame();
        assert!(h.find("hello from a thread").is_some());
    }

    #[test]
    fn waker_merges_wakes_until_the_window_reads_them() {
        let count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c = count.clone();
        let w = crate::Waker::new(move || {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let w2 = w.clone();
        std::thread::spawn(move || w2.wake()).join().unwrap();
        w.wake();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1, "窓が読む前の起こしは 1 回にまとめる");
        assert!(w.take_woken());
        w.wake();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2, "読んだ後はまた知らせる");
    }

    /// アイコンの見本
    #[derive(Default)]
    struct Icons {
        closed: u32,
        done: bool,
    }

    impl App for Icons {
        fn view(&mut self, ui: &mut Ui) {
            ui.row(|ui| {
                ui.icon(crate::Icon::Info);
                ui.icon(crate::Icon::Warning).size(24.0).label("Warning");
                if ui.icon_button(crate::Icon::Close, "Close").clicked() {
                    self.closed += 1;
                }
                ui.button("Add").icon(crate::Icon::Plus);
                ui.button("Plain");
                ui.checkbox(&mut self.done, "Done");
            });
        }
    }

    #[test]
    fn icon_buttons_have_names_and_decorative_icons_are_hidden() {
        let mut h = Harness::new(Icons::default());
        h.click("Close");
        assert_eq!(h.app().closed, 1);
        let close = h.find("Close").unwrap();
        assert_eq!(close.w, close.h, "アイコンだけのボタンは正方形");
        // 文字の前にアイコンがあるボタンは、同じ文字の長さのボタンより広い
        assert!(h.find("Add").unwrap().w >= h.find("Plain").unwrap().w);
        let built = a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new());
        let images: Vec<&accesskit::Node> = built.update.nodes.iter().filter(|(_, n)| n.role() == accesskit::Role::Image).map(|(_, n)| n).collect();
        assert_eq!(images.len(), 2);
        assert_eq!(images.iter().filter(|n| n.is_hidden()).count(), 1, "名前の無いアイコンは読み上げない");
        assert!(images.iter().any(|n| n.label() == Some("Warning") && !n.is_hidden()));
        assert_eq!(built.unlabeled, 0, "アイコンだけのボタンにも名前がある");
    }

    #[test]
    fn icons_and_the_check_mark_are_drawn() {
        fn ink(h: &mut Harness<Icons>, name: Option<&str>, r: crate::geom::Rect) -> u32 {
            let _ = name;
            let image = h.render();
            let bg = image.rgb(r.x as u32, r.y as u32);
            let mut n = 0;
            for y in r.y as u32..(r.y + r.h) as u32 {
                for x in r.x as u32..(r.x + r.w) as u32 {
                    if image.rgb(x, y) != bg {
                        n += 1;
                    }
                }
            }
            n
        }
        let mut h = Harness::new(Icons::default());
        let warning = h.find("Warning").unwrap();
        assert!(ink(&mut h, None, warning) > 40, "アイコンが描かれる");
        let done = h.find("Done").unwrap();
        let box_ = crate::geom::Rect::new(done.x, done.y, done.h, done.h);
        let before = ink(&mut h, None, box_);
        h.click("Done");
        let after = ink(&mut h, None, box_);
        assert!(after > before + 20, "チェックの印が描かれる: {before} → {after}");
    }

    /// 画像の見本: 2×1（赤と、半分すけた青）を等倍と 10 倍で出す
    struct Pictures {
        img: crate::Image,
    }

    impl App for Pictures {
        fn view(&mut self, ui: &mut Ui) {
            ui.image(&self.img);
            ui.image(&self.img).size(20.0, 10.0).label("Flag");
        }
    }

    fn pictures() -> Harness<Pictures> {
        let img = crate::Image::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        Harness::new(Pictures { img })
    }

    #[test]
    fn images_are_drawn_at_their_size_scaled_and_blended() {
        let mut h = pictures();
        let image = h.render();
        let bg = image.rgb(1, 1);
        assert_eq!(bg, 0xffffff);
        // 等倍: 左上の余白（16, 12）から 2 画素
        let at = |x: u32, y: u32| image.rgb(x, y);
        let small = h.rt.tree.layout.iter().zip(&h.rt.tree.info).filter(|(_, n)| matches!(n.kind, crate::ui::Kind::Image { .. })).map(|(l, _)| l.rect).collect::<Vec<_>>();
        assert_eq!((small[0].w, small[0].h), (2.0, 1.0));
        assert_eq!((small[1].w, small[1].h), (20.0, 10.0));
        let (x0, y0) = (small[0].x as u32, small[0].y as u32);
        assert_eq!(at(x0, y0), 0xff0000, "赤はそのまま");
        // 半分すけた青は、白の上で薄い青
        let c = at(x0 + 1, y0);
        assert!((c & 0xff) > 200 && ((c >> 16) & 0xff) > 100 && ((c >> 16) & 0xff) < 160, "{c:06x}");
        // 10 倍: 左の端は赤、右の端は薄い青
        let r = small[1];
        assert_eq!(at(r.x as u32 + 1, r.y as u32 + 5), 0xff0000);
        let c = at((r.x + r.w) as u32 - 2, r.y as u32 + 5);
        assert!((c & 0xff) > 200 && ((c >> 16) & 0xff) < 160, "{c:06x}");
    }

    #[test]
    fn images_without_a_description_are_hidden_from_screen_readers() {
        let h = pictures();
        let built = a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new());
        let images: Vec<&accesskit::Node> = built.update.nodes.iter().filter(|(_, n)| n.role() == accesskit::Role::Image).map(|(_, n)| n).collect();
        assert_eq!(images.len(), 2);
        assert_eq!(images.iter().filter(|n| n.is_hidden()).count(), 1);
        assert!(images.iter().any(|n| n.label() == Some("Flag")));
    }

    /// 2×1 の PNG（赤と、半分すけた青）
    const TINY_PNG: [u8; 71] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0, 244, 34, 127, 138, 0, 0, 0, 14,
        73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 66, 13, 0, 15, 122, 3, 126, 119, 233, 127, 151, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
        96, 130,
    ];

    #[cfg(windows)]
    #[test]
    fn image_files_are_decoded_on_windows() {
        let img = crate::Image::from_bytes(&TINY_PNG).unwrap();
        assert_eq!((img.width(), img.height()), (2, 1));
        assert_eq!(&img.0.pixels[..4], &[255, 0, 0, 255]);
        // 乗算済み: 青 255 × 不透明度 128
        assert_eq!(img.0.pixels[7], 128);
        assert!((img.0.pixels[6] as i32 - 128).abs() <= 1, "{:?}", &img.0.pixels[4..]);
        assert!(crate::Image::from_bytes(b"not an image").is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn image_files_need_rgba_outside_windows_for_now() {
        assert!(crate::Image::from_bytes(&TINY_PNG).is_err());
    }

    /// 複数行の入力欄の見本（3 行の高さ。Ctrl+Enter で送る）
    #[derive(Default)]
    struct Notes {
        body: String,
        sent: u32,
        changes: u32,
    }

    impl App for Notes {
        fn view(&mut self, ui: &mut Ui) {
            if ui.text_area(&mut self.body).hint("Body").rows(3).changed() {
                self.changes += 1;
            }
            if ui.shortcut(Key::Enter, Mods::CTRL) {
                self.sent += 1;
            }
            ui.button("After");
        }
    }

    #[test]
    fn text_area_takes_newlines_and_moves_by_lines() {
        let mut h = Harness::new(Notes::default());
        h.click("Body").type_text("abc").press(Key::Enter).type_text("de").press(Key::Enter).type_text("fghij");
        assert_eq!(h.app().body, "abc\nde\nfghij");
        assert!(h.app().changes > 0);
        // 上へ: 2 行目（"fghij" の終わりは "de" より右なので、2 行目の終わり）
        h.press(Key::Up).type_text("X");
        assert_eq!(h.app().body, "abc\ndeX\nfghij");
        // Home は行の始め、Ctrl+Home は全体の始め、Ctrl+End は全体の終わり
        h.press(Key::Home).type_text("1");
        assert_eq!(h.app().body, "abc\n1deX\nfghij");
        h.press_with(Key::Home, Mods::CTRL).type_text("0");
        h.press_with(Key::End, Mods::CTRL).type_text("!");
        assert_eq!(h.app().body, "0abc\n1deX\nfghij!");
        // 下へ: 同じ横の位置（左の端）の次の行
        h.press_with(Key::Home, Mods::CTRL).press(Key::Down).type_text("2");
        assert_eq!(h.app().body, "0abc\n21deX\nfghij!");
        // End は行の終わり
        h.press(Key::End).type_text("Y");
        assert_eq!(h.app().body, "0abc\n21deXY\nfghij!");
        // PageDown は見える高さ分下へ（ここでは最後の行）
        h.press_with(Key::Home, Mods::CTRL).press(Key::PageDown).press(Key::End).type_text("Z");
        assert_eq!(h.app().body, "0abc\n21deXY\nfghij!Z");
        // Ctrl+Enter は改行を入れずにアプリへ
        h.press_with(Key::Enter, Mods::CTRL);
        assert_eq!(h.app().sent, 1);
        assert!(!h.app().body.ends_with('\n'));
        // Windows の改行を貼ると LF になる
        h.rt.cx.clipboard.set("p\r\nq");
        h.press_with(Key::V, Mods::CTRL);
        assert!(h.app().body.ends_with("Zp\nq"), "{:?}", h.app().body);
    }

    #[test]
    fn text_area_scrolls_to_the_caret_and_with_the_wheel() {
        let mut h = Harness::new(Notes::default());
        let long: String = (0..30).map(|i| format!("line {i}\n")).collect();
        h.app_mut().body = long.clone();
        h.click("Body").press_with(Key::End, Mods::CTRL).type_text("end");
        let id = id_named(&h, "Body");
        let scrolled = h.rt.cx.scrolls.get(&id).copied().unwrap_or(0.0);
        assert!(scrolled > 100.0, "キャレットが見えるように下までスクロールする: {scrolled}");
        // ホイールで上へ戻せる（キャレットの位置へ戻されない）
        h.scroll("Body", -1000.0);
        assert_eq!(h.rt.cx.scrolls.get(&id).copied().unwrap_or(0.0), 0.0);
        h.frame();
        assert_eq!(h.rt.cx.scrolls.get(&id).copied().unwrap_or(0.0), 0.0, "描き直しても戻らない");
    }

    #[test]
    fn text_area_puts_the_caret_where_it_is_clicked_and_is_multiline_for_screen_readers() {
        let mut h = Harness::new(Notes::default());
        h.app_mut().body = "first\nsecond\nthird".into();
        h.frame();
        let r = h.find("Body").unwrap();
        // 入力欄の下の端の近く（3 行目）を押す
        h.send(vec![Event::PointerDown { x: r.x + 12.0, y: r.bottom() - 8.0, shift: false, count: 1 }, Event::PointerUp { x: r.x + 12.0, y: r.bottom() - 8.0 }]);
        h.type_text("#");
        let body = h.app().body.clone();
        assert!(body.starts_with("first\nsecond\n") && body.contains('#') && body.find('#').unwrap() >= "first\nsecond\n".len(), "{body:?}");
        let built = a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new());
        let n = built.update.nodes.iter().find(|(_, n)| n.role() == accesskit::Role::MultilineTextInput).expect("複数行の入力欄の役割").1.clone();
        assert_eq!(n.label(), Some("Body"));
        assert_eq!(n.value(), Some(body.as_str()));
    }

    #[test]
    fn text_area_works_with_the_input_method() {
        let mut h = Harness::new(Notes::default());
        h.click("Body").type_text("メモ").press(Key::Enter);
        h.ime_compose("ぎゅうにゅう");
        assert_eq!(h.app().body, "メモ\n", "変換中は値を変えない");
        h.ime_commit("牛乳");
        assert_eq!(h.app().body, "メモ\n牛乳");
        // Tab は焦点を次の部品へ移す（タブの文字は入れない）
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("After"));
    }

    #[derive(Default)]
    struct TextFields {
        name: String,
        body: String,
        secret: String,
    }

    impl App for TextFields {
        fn view(&mut self, ui: &mut Ui) {
            ui.text_input(&mut self.name).hint("Name");
            ui.text_area(&mut self.body).hint("Body");
            ui.password(&mut self.secret).hint("Password");
        }
    }

    /// 部品の子（読み上げの木の中の物）
    fn a11y_children<A: App>(h: &Harness<A>, name: &str) -> Vec<accesskit::Node> {
        let parent = a11y_node(h, name);
        let built = a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new());
        parent.children().iter().map(|id| built.update.nodes.iter().find(|(n, _)| n == id).expect("子の部品").1.clone()).collect()
    }

    #[test]
    fn text_fields_give_screen_readers_text_runs_and_the_caret() {
        use accesskit::{Role, TextPosition};
        let mut h = Harness::new(TextFields::default());
        h.click("Name").type_text("ab cd").press(Key::Left);
        // 1 行の欄: 子に文字の並びが 1 つ。Orca（AT-SPI）はこれで中身とキャレットを読む
        let name = a11y_node(&h, "Name");
        let runs = a11y_children(&h, "Name");
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].role(), runs[0].value()), (Role::TextRun, Some("ab cd")));
        assert_eq!((runs[0].character_lengths(), runs[0].word_starts()), (&[1u8, 1, 1, 1, 1][..], &[0u8, 3][..]));
        let caret = TextPosition { node: name.children()[0], character_index: 4 };
        let sel = *name.text_selection().expect("キャレットの位置");
        assert_eq!((sel.anchor, sel.focus), (caret, caret));
        // Shift+← で選ぶと、動かない端（anchor）と動く端（focus）が分かれる
        h.press_with(Key::Left, Mods::SHIFT);
        let sel = *a11y_node(&h, "Name").text_selection().unwrap();
        assert_eq!((sel.anchor.character_index, sel.focus.character_index), (4, 3));
        // 複数行の欄: 行ごとの並び。改行は行の終わりの 1 字
        h.click("Body").type_text("x").press(Key::Enter).type_text("y");
        let lines: Vec<String> = a11y_children(&h, "Body").iter().map(|r| r.value().unwrap().to_string()).collect();
        assert_eq!(lines, vec!["x\n", "y"]);
        let body = a11y_node(&h, "Body");
        assert_eq!(body.text_selection().unwrap().focus, TextPosition { node: body.children()[1], character_index: 1 });
        // パスワードは中身を出さない
        h.click("Password").type_text("pw");
        assert!(a11y_node(&h, "Password").children().is_empty());
        assert!(a11y_nodes(&h).iter().all(|n| n.value() != Some("pw")));
    }

    #[test]
    fn password_fields_turn_the_input_method_off() {
        let mut h = Harness::new(TextFields::default());
        // ふつうの入力欄は IME を使う
        h.click("Name");
        let name = h.rt.cx.focus;
        assert!(name.is_some() && h.rt.cx.ime_target() == name);
        // パスワードの欄では IME を切る（打った字を IME の変換や学習に渡さない。Windows の ES_PASSWORD と同じ）
        h.click("Password");
        assert!(h.rt.cx.focus.is_some());
        assert_eq!(h.rt.cx.ime_target(), None);
        // 複数行の欄は使う
        h.click("Body");
        assert_eq!(h.rt.cx.ime_target(), h.rt.cx.focus);
    }

    #[test]
    fn shift_and_arrows_on_a_column_heading_change_its_width() {
        let mut h = Harness::with_size(Narrow, 600.0, 200.0, 1.0);
        let before = h.find("Size").unwrap().w;
        // 見出しに焦点を置く（Tab で表の最初の見出し、右矢印で次の見出し）
        h.press(Key::Tab).press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("Size"));
        h.press_with(Key::Right, Mods::SHIFT).press_with(Key::Right, Mods::SHIFT);
        assert_eq!(h.find("Size").unwrap().w, before + 20.0, "Shift+→ で 10 ずつ広げる");
        h.press_with(Key::Left, Mods::SHIFT);
        assert_eq!(h.find("Size").unwrap().w, before + 10.0);
        // Shift の無い矢印は今までどおり見出しを移る（幅は変えない）
        h.press(Key::Left);
        assert_eq!(h.find("Size").unwrap().w, before + 10.0);
        // 残りの幅の列（幅を決めていない）の見出しでは、右の端（隣の Size との境）を動かす。焦点は動かない
        assert_eq!(focused_name(&h).as_deref(), Some("Name"));
        let name = h.find("Name").unwrap().w;
        h.press_with(Key::Right, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("Name"), "Shift+→ は見出しを移らない");
        assert_eq!(h.find("Size").unwrap().w, before, "Name の右の端が 10 右へ（Size が 10 狭く）");
        assert_eq!(h.find("Name").unwrap().w, name + 10.0);
        h.press_with(Key::Left, Mods::SHIFT).press_with(Key::Left, Mods::SHIFT);
        assert_eq!(h.find("Size").unwrap().w, before + 20.0);
        // 読み上げも、残りの幅の列の見出しで幅を変えられることを伝える
        assert!(a11y_node(&h, "Name").description().is_some_and(|d| d.contains("Shift+Left")));
    }

    /// 残りの幅を使う列が左端にある表（ファイルの閲覧と部品の見本と同じ並び）
    #[derive(Default)]
    struct CityTable;

    impl App for CityTable {
        fn view(&mut self, ui: &mut Ui) {
            let rows = [("Tokyo", "13,960,000", "Kanto"), ("Osaka", "8,780,000", "Kinki")];
            let columns = [crate::ui::TableColumn::new("City").fill(), crate::ui::TableColumn::new("People").width(120.0).right(), crate::ui::TableColumn::new("Region").width(100.0)];
            ui.table(&columns, rows.len(), 30.0).label("Cities").show(|row, i| {
                row.text(rows[i].0);
                row.text(rows[i].1);
                row.text(rows[i].2);
            });
        }
    }

    #[test]
    fn grips_right_of_a_fill_column_sit_on_the_left_edge_and_follow_the_pointer() {
        let mut h = Harness::new(CityTable);
        let people = h.find("People").unwrap();
        let grip = h.find("Resize column").unwrap();
        // 残りの幅を使う列（City）より右の列は、右の端が動かないので、左の端で幅を変える
        assert!((grip.right() - people.x).abs() <= 1.0, "People のつまみは People の左: {grip:?} {people:?}");
        let (x, y) = (grip.x + grip.w / 2.0, grip.y + grip.h / 2.0);
        // 押して、2 回に分けて左へ 40 動かし、何もないフレームも挟んでから離す（幅はフレームの数によらない）
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
        h.send(vec![Event::PointerMove { x: x - 20.0, y }]);
        h.frame();
        h.send(vec![Event::PointerMove { x: x - 40.0, y }]);
        h.frame();
        h.send(vec![Event::PointerUp { x: x - 40.0, y }]);
        let after = h.find("People").unwrap();
        assert!((after.w - (people.w + 40.0)).abs() <= 1.0, "People が 40 広がる: {people:?} → {after:?}");
        assert!((after.right() - people.right()).abs() <= 1.0, "右の端は動かない");
        let g = h.find("Resize column").unwrap();
        assert!((g.x + g.w / 2.0 - (x - 40.0)).abs() <= 1.0, "つまみはマウスについて来る: {g:?}");
        // 右寄せの升の文字は、見出しの右の 8 内側で終わる（見出しと升がそろう）
        let cell = h.find("13,960,000").unwrap();
        assert!((cell.right() - (after.right() - 8.0)).abs() <= 1.0, "{cell:?} {after:?}");
        let tokyo = h.find("Tokyo").unwrap();
        assert!((tokyo.x - (h.find("City").unwrap().x + 8.0)).abs() <= 1.0, "1 列目の升も見出しにそろう");
    }

    /// 文字で飛べる一覧と表（行は見えている所しか作らないので、行の文字はアプリが渡す）
    struct Fruits {
        list: Option<usize>,
        table: Option<usize>,
        names: Vec<String>,
    }

    impl App for Fruits {
        fn view(&mut self, ui: &mut Ui) {
            let names = &self.names;
            ui.list(names.len(), 28.0).height(crate::Size::Fixed(200.0)).selection(&mut self.list).type_ahead(|i| names[i].clone()).show(|ui, i| {
                ui.label(&names[i]);
            });
            let columns = [crate::ui::TableColumn::new("Name").fill()];
            ui.table(&columns, names.len(), 28.0).selection(&mut self.table).type_ahead(|i| names[i].clone()).label("Table").show(|row, i| {
                row.text(format!("{} (t)", names[i]));
            });
        }
    }

    #[test]
    fn lists_jump_by_typed_digits() {
        // 部品の見本の一覧と同じく、行の文字が数字で始まる（「0 row」〜「59 row」）
        let names: Vec<String> = (0..60).map(|i| format!("{i} row")).collect();
        let mut h = Harness::new(Fruits { list: None, table: None, names });
        h.frame();
        h.click("2 row");
        assert_eq!(h.app().list, Some(2));
        h.press(Key::Num5);
        assert_eq!(h.app().list, Some(5), "5 で始まる次の行");
        h.press(Key::Num5).press(Key::Num5);
        assert_eq!(h.app().list, Some(51), "続けて 50、51");
    }

    #[test]
    fn clicking_where_nothing_is_keeps_the_focus() {
        // 作者の手の確かめ（0.3.0 の 3 回目）で、一覧の右の何も無い所を押すと焦点が外れ、数字で飛べなくなった。
        // Windows・GTK・Qt の部品と同じく、焦点を持てる部品を押した時だけ焦点を移す
        struct Form {
            name: String,
            row: Option<usize>,
        }
        impl App for Form {
            fn view(&mut self, ui: &mut Ui) {
                ui.text_input(&mut self.name).hint("Name");
                ui.label("Just text");
                ui.list(60, 28.0).height(Size::Fixed(200.0)).selection(&mut self.row).type_ahead(|i| format!("{i} row")).show(|ui, i| {
                    ui.label(format!("{i} row"));
                });
            }
        }
        let mut h = Harness::new(Form { name: String::new(), row: None });
        h.frame();
        let blank = (h.rt.cx.width - 10.0, h.rt.cx.height - 10.0);
        // 一覧の行を選び、窓の右下の何も無い所と、文字（焦点を持たない部品）を押してから数字を打つ
        h.click("2 row");
        h.click_at(blank.0, blank.1);
        h.click("Just text");
        h.press(Key::Num5);
        assert_eq!(h.app().row, Some(5), "焦点は一覧に残る");
        // 入力欄も同じ。何も無い所を押しても、続けて打てる
        h.click("Name").type_text("ab");
        h.click_at(blank.0, blank.1);
        h.type_text("c");
        assert_eq!(h.app().name, "abc", "焦点は入力欄に残る");
        // 焦点を持てる部品を押せば、焦点はそちらへ移る
        h.click("3 row");
        h.type_text("d");
        assert_eq!(h.app().name, "abc", "一覧を押したので入力欄には入らない");
    }

    #[test]
    fn lists_keep_the_focus_after_the_wheel_scrolls_the_selected_row_away() {
        // 作者の手の確かめ（0.3.0 の 2 回目）で、行を選んでからホイールで遠くへ動かすと、数字を打っても飛ばなかった
        let names: Vec<String> = (0..60).map(|i| format!("{i} row")).collect();
        let mut h = Harness::new(Fruits { list: None, table: None, names });
        h.frame();
        // 一覧の見えている範囲に、その名前の部品があるか（見えない所に作った行は数えない）
        let shown = |h: &Harness<Fruits>, name: &str, table: bool| {
            let lists = &h.rt.cx.prev.lists;
            let view = if table { lists.iter().max_by(|a, b| a.rect.y.total_cmp(&b.rect.y)).unwrap().rect } else { lists[0].rect };
            h.find(name).is_some_and(|r| r.y < view.bottom() && r.bottom() > view.y)
        };
        for (pick, near, table) in [("2 row", "3 row", false), ("2 row (t)", "3 row (t)", true)] {
            h.click(pick);
            h.scroll(near, 800.0);
            assert!(!shown(&h, pick, table), "{pick}: 選んだ行は見えない所にある");
            h.press(Key::Num5);
            h.press(Key::Down);
            let app = h.app();
            assert_eq!(if table { app.table } else { app.list }, Some(6), "{pick}: 5 で 5 行目、↓ で 6 行目");
            assert!(shown(&h, &pick.replace('2', "6"), table), "{pick}: 選んだ行が見えるまで戻る");
        }
    }

    #[test]
    fn the_scroll_bar_of_a_text_area_can_be_pressed_and_dragged() {
        // 入力欄そのものの当たりが、中のスクロールの棒を覆っていた（一覧の行と同じ）
        struct Notes {
            text: String,
        }
        impl App for Notes {
            fn view(&mut self, ui: &mut Ui) {
                ui.text_area(&mut self.text).hint("Notes").rows(4);
            }
        }
        let text = (0..40).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let mut h = Harness::new(Notes { text });
        h.frame();
        let l = h.rt.cx.prev.lists[0].clone();
        let thumb = l.thumb.expect("40 行ならつまみがある");
        let x = thumb.x + thumb.w / 2.0;
        assert_eq!(h.rt.hover_at(x, l.rect.bottom() - 4.0), None, "棒の上では入力欄に乗ったことにしない");
        // 作者の手の確かめ（0.3.0 の 3 回目）で、入力欄の棒の上でもポインターが I の形のままだった
        use crate::ui::context::PointerShape;
        assert_eq!(h.rt.cx.pointer_shape(x, l.rect.bottom() - 4.0), PointerShape::Default, "棒の上は矢印");
        assert_eq!(h.rt.cx.pointer_shape(l.rect.x + 20.0, l.rect.y + 8.0), PointerShape::Text, "文字の上は I の形");
        press(&mut h, x, l.rect.bottom() - 4.0);
        release(&mut h, x, l.rect.bottom() - 4.0);
        assert_eq!(h.rt.cx.scrolls.get(&l.id).copied(), Some(l.rect.h), "空いた所を押すと 1 ページ動く");
        let thumb = h.rt.cx.prev.lists[0].thumb.unwrap();
        press(&mut h, x, thumb.y + 3.0);
        drag_to(&mut h, x, l.rect.y - 100.0);
        release(&mut h, x, l.rect.y - 100.0);
        assert_eq!(h.rt.cx.scrolls.get(&l.id).copied(), Some(0.0), "つまみで一番上へ戻る");
        assert!(h.app().text.starts_with("line 0\nline 1"), "文字は変わらない");
    }

    #[test]
    fn the_scroll_bar_of_a_selectable_list_is_not_covered_by_its_rows() {
        // 作者の手の確かめ（0.3.0 の 2 回目）で、部品の見本の一覧のつまみを押すと、行が選ばれてスクロールしなかった。
        // 選べる一覧の行は一覧の幅いっぱいの当たりなので、右端の棒を覆っていた
        let names: Vec<String> = (0..60).map(|i| format!("{i} row")).collect();
        let mut h = Harness::new(Fruits { list: None, table: None, names });
        h.frame();
        for which in ["list", "table"] {
            let lists = h.rt.cx.prev.lists.clone();
            let l = if which == "list" { lists[0].clone() } else { lists.iter().max_by(|a, b| a.rect.y.total_cmp(&b.rect.y)).unwrap().clone() };
            let thumb = l.thumb.expect("60 行ならつまみがある");
            let x = thumb.x + thumb.w / 2.0;
            assert_eq!(h.rt.hover_at(x, l.rect.bottom() - 6.0), None, "{which}: 棒の上では行に乗ったことにしない");
            // つまみより下の空いた所を押すと、行を選ばずに一覧の高さ 1 つ分動く
            press(&mut h, x, l.rect.bottom() - 6.0);
            release(&mut h, x, l.rect.bottom() - 6.0);
            assert_eq!((h.app().list, h.app().table), (None, None), "{which}: 行は選ばれない");
            assert_eq!(h.rt.cx.scrolls.get(&l.id).copied(), Some(l.rect.h), "{which}: 1 ページ動く");
            // つまみをつかんで一番上まで戻す
            let thumb = h.rt.cx.prev.lists.iter().find(|m| m.id == l.id).unwrap().thumb.unwrap();
            press(&mut h, x, thumb.y + 5.0);
            drag_to(&mut h, x, l.rect.y - 100.0);
            release(&mut h, x, l.rect.y - 100.0);
            assert_eq!(h.rt.cx.scrolls.get(&l.id).copied(), Some(0.0), "{which}: つまみで戻る");
            // 作者の手の確かめ（0.3.0 の 3 回目）で、つまみを狙って一覧の右の端から 1 画素外を押し、
            // 何も無い所を押したことになって焦点が外れた。棒の押せる範囲は、つまみの左と同じく右にも余白を持つ
            press(&mut h, l.rect.right() + 1.0, l.rect.bottom() - 6.0);
            release(&mut h, l.rect.right() + 1.0, l.rect.bottom() - 6.0);
            assert_eq!(h.rt.cx.scrolls.get(&l.id).copied(), Some(l.rect.h), "{which}: 右の端の外でも 1 ページ動く");
            assert_eq!((h.app().list, h.app().table), (None, None), "{which}: 引いても行は選ばれない");
        }
    }

    #[test]
    fn lists_and_tables_jump_to_the_next_row_that_starts_with_the_typed_letter() {
        let mut names: Vec<String> = (0..200).map(|i| format!("apple {i}")).collect();
        names[150] = "Cherry".into();
        names[180] = "cranberry".into();
        let mut h = Harness::new(Fruits { list: None, table: None, names });
        h.frame();
        h.click("apple 0");
        assert_eq!(h.app().list, Some(0));
        // 見えていない行（150 行目）へ飛び、見えるようにスクロールする。大文字と小文字は区別しない
        h.press(Key::C);
        assert_eq!(h.app().list, Some(150));
        assert!(h.find("Cherry").is_some());
        // もう一度同じ文字で、次に c で始まる行。最後まで行ったら先頭から探す
        h.press(Key::C);
        assert_eq!(h.app().list, Some(180));
        h.press(Key::C);
        assert_eq!(h.app().list, Some(150));
        // 当てはまる行が無い文字では動かない
        h.press(Key::Z);
        assert_eq!(h.app().list, Some(150));
        // 表も同じ
        h.click("apple 1 (t)");
        assert_eq!(h.app().table, Some(1));
        h.press(Key::C);
        assert_eq!(h.app().table, Some(150));
    }

    #[derive(Default)]
    struct Copies {
        n: i32,
    }

    impl App for Copies {
        fn view(&mut self, ui: &mut Ui) {
            ui.number(&mut self.n, 0..=1000).label("Copies");
        }
    }

    #[test]
    fn holding_a_spin_button_keeps_stepping() {
        use std::time::Duration;
        let mut h = Harness::new(Copies::default());
        let r = h.find("Increase").unwrap();
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        // 押した時に 1 つ進む
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
        assert_eq!(h.app().n, 1);
        // 押したまま 400 ms までは進まない。その後は 60 ms ごと（フレームの数によらず、たった時間で数える）
        h.wait(Duration::from_millis(390));
        assert_eq!(h.app().n, 1);
        h.wait(Duration::from_millis(250));
        assert_eq!(h.app().n, 1 + 1 + 4, "400 ms で 1 つ、その後 60 ms ごとに 4 つ（640 ms まで）");
        // 離しても増えず、その後も増えない
        h.send(vec![Event::PointerUp { x, y }]);
        h.wait(Duration::from_millis(500));
        assert_eq!(h.app().n, 6);
        // すぐ離すクリックは 1 つだけ
        h.click("Increase");
        assert_eq!(h.app().n, 7);
        h.click("Decrease");
        assert_eq!(h.app().n, 6);
    }

    /// 名前の長い行がある表（名前の列は残りの幅、大きさの列は固定で右寄せ）
    struct Narrow;

    impl App for Narrow {
        fn view(&mut self, ui: &mut Ui) {
            let columns = [crate::ui::TableColumn::new("Name").fill(), crate::ui::TableColumn::new("Size").width(80.0).right()];
            let rows = [("a.txt", "1 KB"), ("a-file-name-much-wider-than-its-column.md", "25 KB")];
            ui.table(&columns, rows.len(), 28.0).show(|row, i| {
                row.text(rows[i].0);
                row.text(rows[i].1);
            });
        }
    }

    #[test]
    fn table_columns_stay_aligned_when_a_name_is_wider_than_its_column() {
        let h = Harness::with_size(Narrow, 260.0, 200.0, 1.0);
        // 右寄せの大きさの升は、どの行でも同じ所で終わる（名前の長い行だけ右へずれない）
        let (short, long, head) = (h.find("1 KB").unwrap(), h.find("25 KB").unwrap(), h.find("Size").unwrap());
        assert_eq!(short.x + short.w, long.x + long.w, "{short:?} {long:?}");
        // 見出しの列とも並ぶ（升の右の端は窓の中）
        assert!(long.x + long.w <= 260.0 && (long.x + long.w - (head.x + head.w)).abs() <= 12.0, "{long:?} {head:?}");
    }

    #[derive(Default)]
    struct Titled {
        clicks: u32,
        about: bool,
    }

    impl App for Titled {
        fn view(&mut self, ui: &mut Ui) {
            if ui.button("Count").clicked() {
                self.clicks += 1;
            }
            ui.dialog(&mut self.about, "About", |ui| {
                ui.label("Hello");
            });
        }
    }

    #[test]
    fn client_side_title_bar_puts_the_app_below_it_and_asks_the_window() {
        use crate::ui::context::{Csd, CsdButton as B, CsdRequest as R};
        let mut h = Harness::new(Titled::default());
        // 合成が帯を描く時（Windows・Hyprland）は帯が無い
        assert!(h.find("Close").is_none());
        let plain = h.find("Count").unwrap();
        h.rt.cx.csd = Some(Csd { title: "Notes".into(), focused: true, maximized: false, tiled: false, left: vec![], right: vec![B::Minimize, B::Maximize, B::Close] });
        h.frame();
        // 浮いた窓は縁に線を引く（合成が影も縁も描かないので、重なった窓の境が分かるように）
        let edge = |h: &mut Harness<Titled>| {
            let image = h.render();
            (image.rgb(0, 300), image.rgb(899, 300), image.rgb(450, 639), image.rgb(6, 300))
        };
        let (left, right, bottom, inside) = edge(&mut h);
        assert!(left != inside && left == right && left == bottom, "{left:06x} {right:06x} {bottom:06x} {inside:06x}");
        let (min, max, close, title, count) = (h.find("Minimize").unwrap(), h.find("Maximize").unwrap(), h.find("Close").unwrap(), h.find("Notes").unwrap(), h.find("Count").unwrap());
        // ボタンは右端に設定の順で並ぶ。題は窓の真ん中。帯は上端から 40 画素で、本文はその下にいつもの余白で置く
        assert!(min.x < max.x && max.x < close.x, "{min:?} {max:?} {close:?}");
        assert!(close.x + close.w > 900.0 - 12.0, "{close:?}");
        assert!(close.y > 0.0 && close.y + close.h < 40.0, "{close:?}");
        assert!((title.x + title.w / 2.0 - 450.0).abs() <= 1.0, "{title:?}");
        assert_eq!((count.x, count.y), (plain.x, plain.y + 40.0));
        assert!(a11y_nodes(&h).iter().any(|n| n.role() == accesskit::Role::TitleBar));
        // ボタンは窓の処理に頼む
        for (name, want) in [("Maximize", R::ToggleMaximize), ("Minimize", R::Minimize), ("Close", R::Close)] {
            h.click(name);
            assert_eq!(h.rt.cx.csd_request.take(), Some(want), "{name}");
        }
        // 帯の何も無い所を押して動かすと、窓を動かす（少し動いただけでは動かさない）
        let (x, y) = (300.0, 20.0);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerMove { x: x + 2.0, y }]);
        assert_eq!(h.rt.cx.csd_request.take(), None);
        h.send(vec![Event::PointerMove { x: x + 20.0, y }]);
        assert_eq!(h.rt.cx.csd_request.take(), Some(R::Move));
        h.send(vec![Event::PointerUp { x: x + 20.0, y }]);
        // 2 回押すと最大化を切り替え、右クリックは窓のメニュー
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
        assert_eq!(h.rt.cx.csd_request.take(), None);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 2 }, Event::PointerUp { x, y }]);
        assert_eq!(h.rt.cx.csd_request.take(), Some(R::ToggleMaximize));
        h.send(vec![Event::ContextMenu { x, y }]);
        assert_eq!(h.rt.cx.csd_request.take(), Some(R::Menu { x, y }));
        // 本文の部品は今までどおり。本文の何も無い所を押して動かしても窓は動かない
        h.click("Count");
        assert_eq!(h.app().clicks, 1);
        h.send(vec![Event::PointerDown { x, y: 300.0, shift: false, count: 1 }, Event::PointerMove { x: x + 40.0, y: 300.0 }, Event::PointerUp { x: x + 40.0, y: 300.0 }]);
        assert_eq!(h.rt.cx.csd_request.take(), None);
        // 最大化した窓では「元に戻す」。焦点の無い窓でも題は出す
        let c = h.rt.cx.csd.as_mut().unwrap();
        c.maximized = true;
        c.tiled = true;
        c.focused = false;
        h.frame();
        assert!(h.find("Restore").is_some() && h.find("Maximize").is_none() && h.find("Notes").is_some());
        // 最大化・敷き詰めの窓は画面の端に付くので、縁の線を引かない
        let (left, _, _, inside) = edge(&mut h);
        assert_eq!(left, inside);
        // ダイアログを出している間も、題の帯は暗くせず押せる（Windows で OS の題の帯が使えるのと同じ）。
        // ダイアログは帯の下の真ん中に出す
        let c = h.rt.cx.csd.as_mut().unwrap();
        c.maximized = false;
        c.tiled = false;
        h.frame();
        let bar = h.render().rgb(300, 20);
        h.app_mut().about = true;
        h.frame();
        assert_eq!(h.render().rgb(300, 20), bar);
        let about = h.find("About").unwrap();
        let csd = h.rt.cx.csd.take();
        h.frame();
        let plain = h.find("About").unwrap();
        h.rt.cx.csd = csd;
        h.frame();
        assert_eq!((about.x, about.y), (plain.x, plain.y + 20.0), "帯の 40 の下の真ん中");
        h.click("Maximize");
        assert_eq!(h.rt.cx.csd_request.take(), Some(R::ToggleMaximize));
        assert!(h.app().about, "題の帯を押してもダイアログは閉じない");
        h.app_mut().about = false;
        // 左にも並べられる（GNOME の button-layout が「close:」の時）
        let c = h.rt.cx.csd.as_mut().unwrap();
        c.left = vec![B::Close];
        c.right = vec![];
        h.frame();
        assert!(h.find("Close").unwrap().x < 12.0);
    }

    /// 1 群の A 組の見本: ラジオ・区切り線・進み具合・枠・開閉・リンク・押したままのボタン・パスワード
    #[derive(Default)]
    struct Parts {
        size: u8,
        sound: bool,
        advanced: bool,
        bold: bool,
        secret: String,
        clicks: u32,
    }

    impl App for Parts {
        fn view(&mut self, ui: &mut Ui) {
            ui.radio_group(&mut self.size, &[(0, "Small"), (1, "Medium"), (2, "Large")]);
            ui.separator();
            ui.progress(0.4).label("Upload");
            ui.spinner().label("Loading");
            let mixed = !self.sound && self.bold;
            ui.group("Sound", |ui| {
                ui.checkbox(&mut self.sound, "Play sounds").mixed(mixed);
            });
            ui.expander(&mut self.advanced, "Advanced", |ui| {
                ui.label("Secret settings");
            });
            if ui.link("Learn more").clicked() {
                self.clicks += 1;
            }
            ui.toggle_button(&mut self.bold, "Bold");
            ui.password(&mut self.secret).hint("Password");
        }
    }

    fn a11y_nodes<A: App>(h: &Harness<A>) -> Vec<accesskit::Node> {
        a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new()).update.nodes.into_iter().map(|(_, n)| n).collect()
    }

    fn a11y_node<A: App>(h: &Harness<A>, name: &str) -> accesskit::Node {
        let id = id_named(h, name);
        a11y::build(&h.rt.tree, &h.rt.cx, "t", &mut std::collections::HashSet::new()).update.nodes.into_iter().find(|(n, _)| n.0 == id.0).unwrap().1
    }

    #[test]
    fn radio_groups_select_with_clicks_and_arrows_and_stop_tab_once() {
        let mut h = Harness::new(Parts::default());
        h.click("Large");
        assert_eq!(h.app().size, 2);
        // Tab は選んだ 1 つにだけ止まる
        h.rt.cx.focus = None;
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Large"));
        // 矢印は選びながら移り、端で折り返す
        h.press(Key::Down);
        assert_eq!((h.app().size, focused_name(&h).as_deref()), (0, Some("Small")));
        h.press(Key::Up);
        assert_eq!((h.app().size, focused_name(&h).as_deref()), (2, Some("Large")));
        h.press(Key::Left);
        assert_eq!(h.app().size, 1);
        // 次の Tab はグループの外へ
        h.press(Key::Tab);
        assert_ne!(focused_name(&h).as_deref(), Some("Small"));
        assert_ne!(focused_name(&h).as_deref(), Some("Large"));
        let nodes = a11y_nodes(&h);
        assert_eq!(nodes.iter().filter(|n| n.role() == accesskit::Role::RadioButton).count(), 3);
        assert!(nodes.iter().any(|n| n.role() == accesskit::Role::RadioGroup));
        assert_eq!(a11y_node(&h, "Medium").toggled(), Some(accesskit::Toggled::True));
    }

    #[test]
    fn expanders_show_their_content_when_open() {
        let mut h = Harness::new(Parts::default());
        assert!(h.find("Secret settings").is_none());
        assert_eq!(a11y_node(&h, "Advanced").is_expanded(), Some(false));
        h.click("Advanced");
        assert!(h.app().advanced);
        assert!(h.find("Secret settings").is_some());
        assert_eq!(a11y_node(&h, "Advanced").is_expanded(), Some(true));
        // 焦点がある時は Enter でも閉じる
        h.press(Key::Enter);
        assert!(!h.app().advanced);
    }

    #[test]
    fn links_toggle_buttons_groups_and_mixed_check_boxes() {
        let mut h = Harness::new(Parts::default());
        h.click("Learn more");
        assert_eq!(h.app().clicks, 1);
        assert_eq!(a11y_node(&h, "Learn more").role(), accesskit::Role::Link);
        h.click("Bold");
        assert!(h.app().bold);
        assert_eq!(a11y_node(&h, "Bold").toggled(), Some(accesskit::Toggled::True));
        // 音が無くて太字なら、チェックは「一部」の状態（見本の決まり）
        assert_eq!(a11y_node(&h, "Play sounds").toggled(), Some(accesskit::Toggled::Mixed));
        let group = a11y_nodes(&h).into_iter().find(|n| n.role() == accesskit::Role::Group).expect("枠");
        assert_eq!(group.label(), Some("Sound"));
    }

    #[test]
    fn password_fields_hide_the_text_and_do_not_copy_it() {
        let mut h = Harness::new(Parts::default());
        h.click("Password").type_text("hunter2");
        assert_eq!(h.app().secret, "hunter2");
        let n = a11y_node(&h, "Password");
        assert_eq!(n.role(), accesskit::Role::PasswordInput);
        assert_ne!(n.value(), Some("hunter2"), "読み上げに値を出さない");
        h.rt.cx.clipboard.set("before");
        h.press_with(Key::A, Mods::CTRL).press_with(Key::C, Mods::CTRL).press_with(Key::X, Mods::CTRL);
        assert_eq!(h.rt.cx.clipboard.get().as_deref(), Some("before"), "写さない");
        assert_eq!(h.app().secret, "hunter2", "切り取らない");
        h.press_with(Key::V, Mods::CTRL);
        assert_eq!(h.app().secret, "before", "貼り付けはできる（全部を選んでいたので置き換わる）");
        // 画面には ● だけを描く: 文字の数だけの ● の幅で、元の文字を並べた幅とは違う
        let field = h.find("Password").unwrap();
        assert!(field.w > 0.0);
    }

    #[test]
    fn progress_reports_its_value_and_the_spinner_animates_with_a_timer() {
        let mut h = Harness::new(Parts::default());
        let p = a11y_node(&h, "Upload");
        assert_eq!(p.role(), accesskit::Role::ProgressIndicator);
        assert_eq!(p.numeric_value(), Some(40.0));
        assert_eq!(a11y_node(&h, "Loading").numeric_value(), None, "終わりが分からない物は値を持たない");
        let out = h.rt.frame(&mut h.app, Vec::new());
        assert!(!out.want_frame, "描き直しを続けない");
        assert!(out.wake_after.is_some_and(|d| d <= std::time::Duration::from_millis(40)), "{:?}", out.wake_after);
    }

    /// ドロップダウンの見本
    #[derive(Default)]
    struct Pick {
        size: u8,
        other: u32,
    }

    impl App for Pick {
        fn view(&mut self, ui: &mut Ui) {
            ui.row(|ui| {
                ui.dropdown(&mut self.size, &[(0, "Small"), (1, "Medium"), (2, "Large")]).label("Size");
                if ui.button("Other").clicked() {
                    self.other += 1;
                }
            });
        }
    }

    #[test]
    fn dropdowns_open_below_and_pick_with_the_pointer() {
        let mut h = Harness::new(Pick::default());
        assert!(h.find("Large").is_none());
        h.click("Size");
        let (field, large) = (h.find("Size").unwrap(), h.find("Large").expect("開くと選択肢が出る"));
        assert!(large.y > field.bottom());
        assert_eq!(focused_name(&h).as_deref(), Some("Small"), "選んでいる項目に焦点");
        assert_eq!(a11y_node(&h, "Size").is_expanded(), Some(true));
        h.click("Large");
        assert_eq!(h.app().size, 2);
        assert!(h.find("Large").is_none(), "選ぶと閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("Size"), "焦点は戻る");
        let n = a11y_node(&h, "Size");
        assert_eq!((n.role(), n.value(), n.is_expanded()), (accesskit::Role::ComboBox, Some("Large"), Some(false)));
    }

    #[test]
    fn dropdowns_work_with_the_keyboard() {
        let mut h = Harness::new(Pick::default());
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Size"));
        // 閉じたまま下の矢印で選び直す
        h.press(Key::Down);
        assert_eq!(h.app().size, 1);
        assert!(h.find("Small").is_none(), "開かない");
        // Alt+下で開き、下で次の項目へ（まだ選ばない）、Enter で選んで閉じる
        h.press_with(Key::Down, Mods::ALT);
        assert_eq!(focused_name(&h).as_deref(), Some("Medium"));
        h.press(Key::Down);
        assert_eq!((h.app().size, focused_name(&h).as_deref()), (1, Some("Large")));
        h.press(Key::Enter);
        assert_eq!(h.app().size, 2);
        assert_eq!(focused_name(&h).as_deref(), Some("Size"));
        // Esc は選び直さずに閉じる
        h.press(Key::Space).press(Key::Up).press(Key::Escape);
        assert_eq!(h.app().size, 2);
        assert!(h.find("Small").is_none());
        // 外を押すと閉じ、そのクリックは下に届かない
        h.click("Size").click("Other");
        assert_eq!(h.app().other, 0);
        assert!(h.find("Small").is_none());
        let nodes = a11y_nodes(&h);
        assert!(!nodes.iter().any(|n| n.role() == accesskit::Role::ListBoxOption));
        h.click("Size");
        let nodes = a11y_nodes(&h);
        assert_eq!(nodes.iter().filter(|n| n.role() == accesskit::Role::ListBoxOption).count(), 3);
        assert!(nodes.iter().any(|n| n.role() == accesskit::Role::ListBox));
    }

    #[test]
    fn dropdowns_pick_by_typing_a_letter() {
        let mut h = Harness::new(Pick::default());
        h.press(Key::Tab).press(Key::L);
        assert_eq!(h.app().size, 2, "閉じたまま、L で始まる Large を選ぶ");
        h.press(Key::Space).press(Key::M);
        assert_eq!(focused_name(&h).as_deref(), Some("Medium"), "開いた一覧では焦点だけが移る");
        assert_eq!(h.app().size, 2);
    }

    /// メニューの見本: メニューバー、メニューボタン、右クリックのメニュー
    #[derive(Default)]
    struct Doc {
        saved: u32,
        opened: Vec<&'static str>,
        wrap: bool,
        sort: u8,
        renamed: u32,
        deleted: u32,
        customized: u32,
        other: u32,
    }

    impl App for Doc {
        fn view(&mut self, ui: &mut Ui) {
            ui.menu_bar(|bar| {
                bar.menu("File", |m| {
                    if m.item("Save").shortcut(Key::S, Mods::CTRL).clicked() {
                        self.saved += 1;
                    }
                    m.submenu("Recent", |m| {
                        for name in ["a.txt", "b.txt"] {
                            if m.item(name).clicked() {
                                self.opened.push(name);
                            }
                        }
                    });
                    m.separator();
                    m.item("Exit").enabled(false);
                });
                bar.menu("View", |m| {
                    m.check(&mut self.wrap, "Word wrap").shortcut(Key::W, Mods::ALT);
                    m.separator();
                    m.radio(&mut self.sort, 0, "By name");
                    m.radio(&mut self.sort, 1, "By date");
                });
            });
            ui.row(|ui| {
                ui.menu_button("More", |m| {
                    if m.item("Rename").clicked() {
                        self.renamed += 1;
                    }
                    m.item("Duplicate");
                    m.item("Delete");
                });
                let other = ui.button("Other").context_menu(|m| {
                    if m.item("Customize").shortcut(Key::F2, Mods::NONE).clicked() {
                        self.customized += 1;
                    }
                });
                if other.clicked() {
                    self.other += 1;
                }
            });
            ui.label("report.pdf").context_menu(|m| {
                if m.item("Delete file").clicked() {
                    self.deleted += 1;
                }
                m.item("Properties");
            });
        }
    }

    #[test]
    fn menu_buttons_open_below_and_close_after_a_pick() {
        let mut h = Harness::new(Doc::default());
        assert!(h.find("Rename").is_none());
        let n = a11y_node(&h, "More");
        assert_eq!((n.role(), n.has_popup(), n.is_expanded()), (accesskit::Role::Button, Some(accesskit::HasPopup::Menu), Some(false)));
        h.click("More");
        let (button, rename) = (h.find("More").unwrap(), h.find("Rename").expect("押すと開く"));
        assert!(rename.y > button.bottom());
        assert_eq!(a11y_node(&h, "More").is_expanded(), Some(true));
        assert_eq!(a11y_node(&h, "Rename").role(), accesskit::Role::MenuItem);
        h.click("Rename");
        assert_eq!(h.app().renamed, 1);
        assert!(h.find("Rename").is_none(), "選ぶと閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("More"), "焦点はボタンへ戻る");
        // 外を押すと閉じ、そのクリックは下に届かない
        h.click("More").click("Other");
        assert_eq!(h.app().other, 0);
        assert!(h.find("Rename").is_none());
        // 開いている時にもう一度押すと閉じる
        h.click("More").click("More");
        assert!(h.find("Rename").is_none());
    }

    #[test]
    fn opening_a_layer_asks_for_a_frame_to_draw_the_moved_focus() {
        let mut h = Harness::new(Doc::default());
        h.press(Key::Tab).press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("More"));
        let out = h.rt.frame(&mut h.app, vec![Event::Key { key: Key::Enter, mods: Mods::NONE }]);
        assert_eq!(focused_name(&h).as_deref(), Some("Rename"));
        assert!(out.want_frame, "描いた後で焦点が開いた物の中へ移ったので、もう 1 フレーム描く");
    }

    #[test]
    fn menu_buttons_work_with_the_keyboard() {
        let mut h = Harness::new(Doc::default());
        // メニューバーは Tab で 1 回だけ止まる
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("File"));
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("More"));
        h.press(Key::Enter);
        assert_eq!(focused_name(&h).as_deref(), Some("Rename"), "開くと最初の項目に焦点");
        h.press(Key::Up);
        assert_eq!(focused_name(&h).as_deref(), Some("Delete"), "端で折り返す");
        h.press(Key::Home);
        assert_eq!(focused_name(&h).as_deref(), Some("Rename"));
        h.press(Key::D);
        assert_eq!(focused_name(&h).as_deref(), Some("Duplicate"), "打った文字で始まる項目へ");
        h.press(Key::D);
        assert_eq!(focused_name(&h).as_deref(), Some("Delete"), "同じ文字でその次へ");
        h.press(Key::Escape);
        assert!(h.find("Rename").is_none());
        assert_eq!(focused_name(&h).as_deref(), Some("More"));
        h.press(Key::Up);
        assert_eq!(focused_name(&h).as_deref(), Some("Delete"), "上の矢印で開くと最後の項目");
        h.press(Key::Tab);
        assert!(h.find("Rename").is_none(), "Tab で閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("More"));
        h.press(Key::Down).press(Key::Enter);
        assert_eq!(h.app().renamed, 1);
    }

    #[test]
    fn menu_bars_and_submenus_work_with_the_keyboard() {
        let mut h = Harness::new(Doc::default());
        h.click("Other");
        // F10 でメニューバーの最初の見出しへ。左右で見出しを移る
        h.press(Key::F10);
        assert_eq!(focused_name(&h).as_deref(), Some("File"));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("View"));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("File"), "端で折り返す");
        h.press(Key::Down);
        assert_eq!(focused_name(&h).as_deref(), Some("Save"));
        h.press(Key::Down);
        assert_eq!(focused_name(&h).as_deref(), Some("Recent"));
        assert_eq!(a11y_node(&h, "Recent").has_popup(), Some(accesskit::HasPopup::Menu));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("a.txt"), "右で入れ子のメニューを開く");
        let (recent, a) = (h.find("Recent").unwrap(), h.find("a.txt").unwrap());
        assert!(a.x >= recent.right(), "入れ子のメニューは横に出る");
        h.press(Key::Left);
        assert!(h.find("a.txt").is_none(), "左で閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("Recent"));
        h.press(Key::Right).press(Key::Down).press(Key::Enter);
        assert_eq!(h.app().opened, vec!["b.txt"]);
        assert!(h.find("Save").is_none(), "選ぶと全部閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("Other"), "焦点はメニューバーへ入る前の場所へ戻る");
        // メニューの中の左右で、隣の見出しのメニューへ
        h.press(Key::F10).press(Key::Enter);
        assert!(h.find("Save").is_some());
        h.press(Key::Right);
        assert!(h.find("Save").is_none());
        assert_eq!(focused_name(&h).as_deref(), Some("Word wrap"));
        h.press(Key::Left);
        assert_eq!(focused_name(&h).as_deref(), Some("Save"));
        // 使えない項目（Exit）は飛ばす
        h.press(Key::Up);
        assert_eq!(focused_name(&h).as_deref(), Some("Recent"));
        // Esc の 1 回目でメニューを閉じて見出しへ、2 回目でメニューバーへ入る前の場所へ
        h.press(Key::Escape);
        assert_eq!(focused_name(&h).as_deref(), Some("File"));
        h.press(Key::Escape);
        assert_eq!(focused_name(&h).as_deref(), Some("Other"));
    }

    #[test]
    fn menu_bars_switch_on_hover_and_submenus_open_after_a_pause() {
        let mut h = Harness::new(Doc::default());
        h.click("File");
        assert!(h.find("Save").is_some());
        h.hover("View");
        assert!(h.find("Save").is_none(), "開いている時は、乗せるだけで切り替わる");
        assert!(h.find("Word wrap").is_some());
        h.hover("File").hover("Recent");
        assert!(h.find("a.txt").is_none(), "すぐには開かない");
        h.wait(std::time::Duration::from_millis(500));
        assert!(h.find("a.txt").is_some(), "止まると開く");
        h.hover("Save").wait(std::time::Duration::from_millis(500));
        assert!(h.find("a.txt").is_none(), "別の項目に止まると閉じる");
        h.click("Recent");
        assert!(h.find("a.txt").is_some(), "押すとすぐ開く");
        h.click("a.txt");
        assert_eq!(h.app().opened, vec!["a.txt"]);
        assert!(h.find("Save").is_none());
    }

    #[test]
    fn menu_items_check_pick_and_answer_to_shortcuts() {
        let mut h = Harness::new(Doc::default());
        h.press_with(Key::S, Mods::CTRL);
        assert_eq!(h.app().saved, 1, "閉じたメニューの項目もショートカットで動く");
        h.press_with(Key::W, Mods::ALT);
        assert!(h.app().wrap);
        h.click("View");
        let n = a11y_node(&h, "Word wrap");
        assert_eq!((n.role(), n.toggled()), (accesskit::Role::MenuItemCheckBox, Some(accesskit::Toggled::True)));
        assert_eq!(n.keyboard_shortcut(), Some("Alt+W"));
        assert_eq!(a11y_node(&h, "By date").role(), accesskit::Role::MenuItemRadio);
        h.click("Word wrap");
        assert!(!h.app().wrap);
        h.click("View").click("By date");
        assert_eq!(h.app().sort, 1);
        h.click("View");
        assert_eq!(a11y_node(&h, "By date").toggled(), Some(accesskit::Toggled::True));
        assert_eq!(a11y_node(&h, "By name").toggled(), Some(accesskit::Toggled::False));
        h.press(Key::Escape);
        assert_eq!(h.enabled("Exit"), None, "File のメニューは閉じている");
        h.click("File");
        assert_eq!(h.enabled("Exit"), Some(false));
        let nodes = a11y_nodes(&h);
        assert!(nodes.iter().any(|n| n.role() == accesskit::Role::MenuBar));
        assert!(nodes.iter().any(|n| n.role() == accesskit::Role::Menu));
        assert_eq!(a11y_node(&h, "Save").keyboard_shortcut(), Some("Ctrl+S"));
    }

    #[test]
    fn context_menus_open_at_the_pointer_and_from_the_keyboard() {
        let mut h = Harness::new(Doc::default());
        h.right_click("report.pdf");
        let (label, item) = (h.find("report.pdf").unwrap(), h.find("Delete file").expect("右クリックで開く"));
        let (x, y) = (label.x + label.w / 2.0, label.y + label.h / 2.0);
        assert!((item.x - x).abs() < 12.0 && item.y >= y, "ポインターの所に出る: {item:?} {x} {y}");
        h.click("Delete file");
        assert_eq!(h.app().deleted, 1);
        assert!(h.find("Properties").is_none());
        // ほかのメニューが開いている時に右クリックすると、そちらを閉じて開く
        h.click("More").right_click("report.pdf");
        assert!(h.find("Rename").is_none() && h.find("Properties").is_some());
        h.press(Key::Escape);
        // 焦点のある部品で Shift+F10 かアプリケーションキー
        h.click("Other");
        h.press_with(Key::F10, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("Customize"));
        h.press(Key::Escape);
        assert_eq!(focused_name(&h).as_deref(), Some("Other"));
        h.press(Key::ContextMenu);
        assert!(h.find("Customize").is_some());
        h.press(Key::Escape);
        // 右クリックのメニューの項目のショートカットは、焦点がその部品の中にある時だけ動く
        h.press(Key::F2);
        assert_eq!(h.app().customized, 1);
        h.press(Key::Tab);
        assert_ne!(focused_name(&h).as_deref(), Some("Other"));
        h.press(Key::F2);
        assert_eq!(h.app().customized, 1);
    }

    /// タブの見本
    #[derive(Clone, Copy, PartialEq, Debug, Default)]
    enum Page {
        #[default]
        General,
        Advanced,
        About,
    }

    #[derive(Default)]
    struct TabbedSettings {
        page: Page,
        sound: bool,
        proxy: bool,
    }

    impl App for TabbedSettings {
        fn view(&mut self, ui: &mut Ui) {
            let pages = [(Page::General, "General"), (Page::Advanced, "Advanced"), (Page::About, "About")];
            ui.tabs(&mut self.page, &pages, |ui, page| match page {
                Page::General => {
                    ui.checkbox(&mut self.sound, "Sound");
                }
                Page::Advanced => {
                    ui.checkbox(&mut self.proxy, "Use a proxy");
                }
                Page::About => {
                    ui.label("Version 1");
                }
            });
            ui.button("Done");
        }
    }

    #[test]
    fn tabs_show_the_selected_panel_and_switch_on_click() {
        let mut h = Harness::new(TabbedSettings::default());
        assert!(h.find("Sound").is_some());
        assert!(h.find("Use a proxy").is_none());
        h.click("Advanced");
        assert_eq!(h.app().page, Page::Advanced);
        assert!(h.find("Sound").is_none());
        assert!(h.find("Use a proxy").is_some());
        let (tab, panel) = (h.find("Advanced").unwrap(), h.find("Use a proxy").unwrap());
        assert!(panel.y > tab.bottom(), "中身はタブの下");
        let n = a11y_node(&h, "Advanced");
        assert_eq!((n.role(), n.is_selected()), (accesskit::Role::Tab, Some(true)));
        assert_eq!(a11y_node(&h, "General").is_selected(), Some(false));
        let nodes = a11y_nodes(&h);
        assert!(nodes.iter().any(|n| n.role() == accesskit::Role::TabList));
        let panel = nodes.iter().find(|n| n.role() == accesskit::Role::TabPanel).expect("中身の入れ物");
        assert_eq!(panel.label(), Some("Advanced"), "中身は選んでいるタブの名前で読む");
    }

    #[test]
    fn tabs_work_with_the_keyboard() {
        let mut h = Harness::new(TabbedSettings::default());
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("General"), "Tab は選んでいるタブに 1 回だけ止まる");
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Sound"), "次は中身");
        h.press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("General"));
        h.press(Key::Right);
        assert_eq!((h.app().page, focused_name(&h).as_deref()), (Page::Advanced, Some("Advanced")), "矢印で移って選ぶ");
        h.press(Key::End);
        assert_eq!(h.app().page, Page::About);
        h.press(Key::Right);
        assert_eq!(h.app().page, Page::General, "端で回る");
        h.press(Key::Left);
        assert_eq!(h.app().page, Page::About);
        h.press(Key::Home);
        assert_eq!(h.app().page, Page::General);
        // 中身に焦点がある時の Ctrl+Tab・Ctrl+Shift+Tab・Ctrl+PageDown・Ctrl+PageUp
        h.click("Sound");
        h.press_with(Key::Tab, Mods::CTRL);
        assert_eq!(h.app().page, Page::Advanced);
        assert_eq!(focused_name(&h).as_deref(), Some("Advanced"), "中身が消えたので、焦点はタブへ");
        h.press_with(Key::Tab, Mods::CTRL | Mods::SHIFT);
        assert_eq!(h.app().page, Page::General);
        h.press_with(Key::PageDown, Mods::CTRL);
        assert_eq!(h.app().page, Page::Advanced);
        h.press_with(Key::PageUp, Mods::CTRL);
        assert_eq!(h.app().page, Page::General);
        // タブの外に焦点がある時は動かない
        h.click("Done");
        h.press_with(Key::Tab, Mods::CTRL);
        assert_eq!(h.app().page, Page::General);
    }

    /// 数の入力の見本
    struct Order {
        copies: i32,
        scale: f64,
    }

    impl App for Order {
        fn view(&mut self, ui: &mut Ui) {
            ui.number(&mut self.copies, 1..=10).label("Copies");
            ui.number(&mut self.scale, 0.5..=2.0).step(0.25).label("Scale");
            ui.button("Next");
        }
    }

    fn order() -> Harness<Order> {
        Harness::new(Order { copies: 1, scale: 1.0 })
    }

    #[test]
    fn number_inputs_take_typed_numbers_on_enter_or_when_leaving() {
        let mut h = order();
        h.click("Copies").press_with(Key::A, Mods::CTRL).type_text("7").press(Key::Enter);
        assert_eq!(h.app().copies, 7);
        // 数でなければ元に戻す
        h.press_with(Key::A, Mods::CTRL).type_text("abc").press(Key::Enter);
        assert_eq!(h.app().copies, 7);
        assert_eq!(a11y_node(&h, "Copies").numeric_value(), Some(7.0));
        // 外へ出る時にも決める。範囲の外は端に寄せる
        h.press_with(Key::A, Mods::CTRL).type_text("50").press(Key::Tab);
        assert_eq!(h.app().copies, 10);
        // 打っている途中はまだ変えない
        h.click("Copies").press_with(Key::A, Mods::CTRL).type_text("3");
        assert_eq!(h.app().copies, 10);
        h.click("Next");
        assert_eq!(h.app().copies, 3);
        // 小数
        h.click("Scale").press_with(Key::A, Mods::CTRL).type_text("1.5").press(Key::Enter);
        assert_eq!(h.app().scale, 1.5);
    }

    #[test]
    fn number_inputs_step_with_keys_buttons_and_screen_readers() {
        let mut h = order();
        h.click("Copies");
        h.press(Key::Up);
        assert_eq!(h.app().copies, 2);
        h.press(Key::Down).press(Key::Down);
        assert_eq!(h.app().copies, 1, "最小より下には行かない");
        h.press(Key::PageUp);
        assert_eq!(h.app().copies, 10, "PageUp は 10 段（最大で止まる）");
        h.press(Key::PageDown);
        assert_eq!(h.app().copies, 1);
        // 小さいボタン（読み上げの木には出さない）
        h.click("Increase");
        assert_eq!(h.app().copies, 2);
        h.click("Decrease");
        assert_eq!(h.app().copies, 1);
        h.click("Scale").press(Key::Up);
        assert_eq!(h.app().scale, 1.25, "step(0.25)");
        let n = a11y_node(&h, "Scale");
        assert_eq!(n.role(), accesskit::Role::SpinButton);
        assert_eq!((n.min_numeric_value(), n.max_numeric_value(), n.numeric_value_step()), (Some(0.5), Some(2.0), Some(0.25)));
        assert!(!a11y_nodes(&h).iter().any(|n| n.label() == Some("Increase")), "小さいボタンは読み上げに出さない");
        let id = id_named(&h, "Copies");
        h.send(vec![Event::A11y { target: id, action: crate::input::A11yAction::Increment }]);
        assert_eq!(h.app().copies, 2, "読み上げソフトの増やす操作");
    }

    /// ツールバーの見本
    #[derive(Default)]
    struct Editor {
        added: u32,
        bold: bool,
        settings: u32,
    }

    impl App for Editor {
        fn view(&mut self, ui: &mut Ui) {
            ui.toolbar(|tb| {
                if tb.button(crate::Icon::Plus, "New").clicked() {
                    self.added += 1;
                }
                tb.toggle(&mut self.bold, crate::Icon::Check, "Bold");
                tb.separator();
                tb.button(crate::Icon::Search, "Find").enabled(false);
                tb.more(|m| {
                    if m.item("Settings").clicked() {
                        self.settings += 1;
                    }
                });
            })
            .label("Editing");
            ui.button("After");
        }
    }

    #[test]
    fn toolbars_are_one_tab_stop_and_move_with_arrows() {
        let mut h = Harness::new(Editor::default());
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("New"));
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("After"), "ツールバーは Tab で 1 回だけ止まる");
        h.press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("New"));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("Bold"));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("More options"), "使えない Find は飛ばす");
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("New"), "端で回る");
        h.press(Key::End);
        assert_eq!(focused_name(&h).as_deref(), Some("More options"));
        h.press(Key::Home).press(Key::Right).press(Key::Space);
        assert!(h.app().bold);
        // 次に Tab で入る時は、最後にいたボタンに止まる
        h.press(Key::Tab).press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("Bold"));
        h.press(Key::End).press(Key::Down);
        assert_eq!(focused_name(&h).as_deref(), Some("Settings"), "「ほか」はメニュー");
        h.press(Key::Enter);
        assert_eq!(h.app().settings, 1);
    }

    #[test]
    fn toolbar_buttons_show_their_names_as_tooltips_and_to_screen_readers() {
        let mut h = Harness::new(Editor::default());
        h.click("New");
        assert_eq!(h.app().added, 1);
        let nodes = a11y_nodes(&h);
        let bar = nodes.iter().find(|n| n.role() == accesskit::Role::Toolbar).expect("ツールバー");
        assert_eq!(bar.label(), Some("Editing"));
        let bold = a11y_node(&h, "Bold");
        assert_eq!((bold.role(), bold.toggled()), (accesskit::Role::Button, Some(accesskit::Toggled::False)));
        assert_eq!(h.enabled("Find"), Some(false));
        h.hover("New").wait(std::time::Duration::from_millis(600));
        assert!(h.find_all("New").len() >= 2, "名前をツールチップにも出す");
    }

    /// 格子の見本: 名前の列はいちばん長い名前の幅、欄の列は残り
    #[derive(Default)]
    struct GridForm {
        name: String,
        email: String,
    }

    impl App for GridForm {
        fn view(&mut self, ui: &mut Ui) {
            ui.grid(&[Size::Auto, Size::Fill(1.0)], |ui| {
                ui.label("Name");
                ui.text_input(&mut self.name).label("Name field");
                ui.label("E-mail address");
                ui.text_input(&mut self.email).label("E-mail field");
            });
            ui.label("Below");
        }
    }

    #[test]
    fn grids_line_up_cells_in_columns_and_rows() {
        let h = Harness::new(GridForm::default());
        let (name, mail) = (h.find("Name").unwrap(), h.find("E-mail address").unwrap());
        let (f1, f2) = (h.find("Name field").unwrap(), h.find("E-mail field").unwrap());
        assert_eq!(name.x, mail.x);
        assert_eq!(f1.x, f2.x, "2 列目は同じ x から");
        assert!(f1.x > mail.right(), "1 列目はいちばん長い名前の幅");
        assert_eq!(f1.w, f2.w);
        assert!((f1.right() - (h.rt.cx.width - 16.0)).abs() < 1.0, "Fill の列は残りの幅を取る: {f1:?}");
        assert!(f2.y >= f1.bottom(), "2 行目は 1 行目の下");
        assert!((name.y + name.h / 2.0 - (f1.y + f1.h / 2.0)).abs() <= 1.0, "行の中は縦の真ん中にそろう");
        assert!(h.find("Below").unwrap().y >= f2.bottom());
    }

    /// 仕切りの見本
    struct Panes {
        left: f32,
    }

    impl App for Panes {
        fn view(&mut self, ui: &mut Ui) {
            ui.split_left_right(&mut self.left, |ui, pane| {
                if pane == 0 {
                    ui.label("Folders");
                } else {
                    ui.label("Files");
                }
            })
            .label("Resize folders");
        }
    }

    #[test]
    fn splitters_resize_with_the_pointer_and_the_keyboard() {
        let mut h = Harness::new(Panes { left: 200.0 });
        let (folders, files, bar) = (h.find("Folders").unwrap(), h.find("Files").unwrap(), h.find("Resize folders").unwrap());
        assert_eq!(folders.x, 16.0);
        assert!((bar.x - (16.0 + 200.0)).abs() <= 1.0, "仕切りは左の幅の右: {bar:?}");
        assert!(files.x >= bar.right());
        h.drag("Resize folders", 50.0, 0.0);
        assert_eq!(h.app().left, 250.0);
        let bar = h.find("Resize folders").unwrap();
        assert!((bar.x - (16.0 + 250.0)).abs() <= 1.0);
        // 狭すぎる・広すぎる幅には寄せない
        h.drag("Resize folders", -1000.0, 0.0);
        assert_eq!(h.app().left, 48.0);
        // キーボード: Tab で止まり、左右で 10 ずつ、Home と End で端へ
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Resize folders"));
        h.press(Key::Right).press(Key::Right);
        assert_eq!(h.app().left, 68.0);
        h.press(Key::End);
        let max = h.app().left;
        assert!(max > 600.0 && max < h.rt.cx.width - 32.0, "{max}");
        h.press(Key::Home);
        assert_eq!(h.app().left, 48.0);
        let n = a11y_node(&h, "Resize folders");
        assert_eq!((n.role(), n.numeric_value()), (accesskit::Role::Splitter, Some(48.0)));
    }

    /// 自由に描く面の見本: 押した所に点を打つ
    #[derive(Default)]
    struct Sketch {
        dots: Vec<(f32, f32)>,
        held: Option<(f32, f32)>,
    }

    impl App for Sketch {
        fn view(&mut self, ui: &mut Ui) {
            let dots = self.dots.clone();
            let c = ui
                .canvas(200.0, 120.0, |d| {
                    d.rect(crate::Rect::new(0.0, 0.0, 200.0, 120.0), crate::rgb(0xf0f0f0));
                    d.line((0.0, 0.0), (200.0, 120.0), 2.0, crate::rgb(0x808080));
                    for &(x, y) in &dots {
                        d.circle((x, y), 4.0, crate::rgb(0x2060c0));
                    }
                    d.text((8.0, 8.0), "Sketch", crate::rgb(0x000000));
                })
                .label("Drawing");
            self.held = c.pointer();
            if let Some(p) = c.clicked_at() {
                self.dots.push(p);
            }
        }
    }

    #[test]
    fn canvases_draw_shapes_and_report_the_pointer_in_their_own_coordinates() {
        let mut h = Harness::new(Sketch::default());
        let area = h.find("Drawing").unwrap();
        assert_eq!((area.w, area.h), (200.0, 120.0));
        let quads = h.rt.draw.quads.len();
        h.click("Drawing");
        assert_eq!(h.app().dots, vec![(100.0, 60.0)], "面の左上からの位置（論理ピクセル）");
        assert!(h.rt.draw.quads.len() > quads, "点の円が増える");
        // 押している間の位置
        let (x, y) = (area.x + 10.0, area.y + 20.0);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
        assert_eq!(h.app().held, Some((10.0, 20.0)));
        h.send(vec![Event::PointerMove { x: x + 30.0, y }]);
        assert_eq!(h.app().held, Some((40.0, 20.0)));
        h.send(vec![Event::PointerUp { x: x + 30.0, y }]);
        assert_eq!(h.app().dots.last(), Some(&(40.0, 20.0)), "離した所で clicked_at");
        let n = a11y_node(&h, "Drawing");
        assert_eq!(n.role(), accesskit::Role::Canvas);
        // 斜めの線と文字は CPU で描いた絵にも出る（線の真ん中の画素が背景と違う）
        let img = h.render();
        // (50, 30) は線の上（点から離れた所）、(150, 20) は何も描いていない所
        let on_line = img.rgb((area.x + 50.0) as u32, (area.y + 30.0) as u32);
        let bg = img.rgb((area.x + 150.0) as u32, (area.y + 20.0) as u32);
        assert_ne!(on_line, bg, "線が描かれている");
    }

    /// 木の表示の見本
    #[derive(Default)]
    struct Folders {
        sel: Option<u32>,
    }

    impl App for Folders {
        fn view(&mut self, ui: &mut Ui) {
            ui.tree(&mut self.sel, |t| {
                t.branch(1, "Documents", |t| {
                    t.leaf(2, "report.pdf");
                    t.branch(3, "Photos", |t| {
                        t.leaf(4, "cat.jpg");
                    });
                });
                t.leaf(5, "notes.txt");
            })
            .label("Files");
            ui.button("After");
        }
    }

    #[test]
    fn trees_start_collapsed_and_open_with_the_pointer() {
        let mut h = Harness::new(Folders::default());
        assert!(h.find("report.pdf").is_none(), "最初は閉じている");
        h.click("Documents");
        assert_eq!(h.app().sel, Some(1), "行を押すと選ぶ");
        assert!(h.find("report.pdf").is_none(), "行の真ん中を押しても開かない");
        // 左の印（シェブロン）を押すと開く
        let row = h.find("Documents").unwrap();
        let (x, y) = (row.x + 12.0, row.y + row.h / 2.0);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
        let child = h.find("report.pdf").expect("開く");
        assert!(child.y > row.y);
        let n = a11y_node(&h, "Documents");
        assert_eq!((n.role(), n.level(), n.is_expanded(), n.is_selected()), (accesskit::Role::TreeItem, Some(1), Some(true), Some(true)));
        let n = a11y_node(&h, "report.pdf");
        assert_eq!((n.level(), n.is_expanded()), (Some(2), None), "葉は開閉を持たない");
        assert!(a11y_nodes(&h).iter().any(|n| n.role() == accesskit::Role::Tree && n.label() == Some("Files")));
        // 子は字下げされる（行の中の文字の位置を、描いた絵で比べる代わりに、行の左の余白の印の位置で見る）
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
        assert!(h.find("report.pdf").is_none(), "もう一度押すと閉じる");
    }

    #[test]
    fn trees_work_with_the_keyboard() {
        let mut h = Harness::new(Folders::default());
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Documents"), "選んでいなければ最初の行");
        h.press(Key::Down);
        assert_eq!((focused_name(&h).as_deref(), h.app().sel), (Some("notes.txt"), Some(5)), "矢印で移って選ぶ");
        h.press(Key::Up);
        assert_eq!(h.app().sel, Some(1));
        h.press(Key::Right);
        assert!(h.find("report.pdf").is_some(), "右で開く");
        assert_eq!(focused_name(&h).as_deref(), Some("Documents"));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("report.pdf"), "開いていれば最初の子へ");
        h.press(Key::Down).press(Key::Right).press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("cat.jpg"));
        h.press(Key::Left);
        assert_eq!(focused_name(&h).as_deref(), Some("Photos"), "左で親へ");
        h.press(Key::Left);
        assert!(h.find("cat.jpg").is_none(), "開いている枝は左で閉じる");
        h.press(Key::Left);
        assert_eq!(focused_name(&h).as_deref(), Some("Documents"));
        h.press(Key::Enter);
        assert!(h.find("report.pdf").is_none(), "Enter で開閉");
        h.press(Key::End);
        assert_eq!(h.app().sel, Some(5));
        h.press(Key::Home).press(Key::N);
        assert_eq!(focused_name(&h).as_deref(), Some("notes.txt"), "文字で移る");
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("After"), "木は Tab で 1 回だけ止まる");
        h.press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("notes.txt"), "戻ると選んでいる行");
    }

    /// 表の見本: アプリが並べ替える
    #[derive(Default)]
    struct FileTable {
        sort: Option<(usize, bool)>,
        sel: Option<usize>,
    }

    impl App for FileTable {
        fn view(&mut self, ui: &mut Ui) {
            let mut rows = [("b.txt", 20, "Text"), ("a.png", 300, "Image"), ("c.md", 5, "Text")];
            match self.sort {
                Some((0, asc)) => rows.sort_by(|x, y| if asc { x.0.cmp(y.0) } else { y.0.cmp(x.0) }),
                Some((1, asc)) => rows.sort_by(|x, y| if asc { x.1.cmp(&y.1) } else { y.1.cmp(&x.1) }),
                _ => {}
            }
            let cols = [crate::ui::TableColumn::new("Name").width(200.0), crate::ui::TableColumn::new("Size").width(90.0).right(), crate::ui::TableColumn::new("Kind").fill()];
            ui.table(&cols, rows.len(), 30.0).sort(&mut self.sort).selection(&mut self.sel).label("Files").grow().show(|row, i| {
                row.text(rows[i].0);
                row.text(format!("{} KB", rows[i].1));
                row.text(rows[i].2);
            });
        }
    }

    #[test]
    fn tables_line_up_headers_and_cells() {
        let h = Harness::new(FileTable::default());
        let (name, size, kind) = (h.find("Name").unwrap(), h.find("Size").unwrap(), h.find("Kind").unwrap());
        let (b, b_size, b_kind) = (h.find("b.txt").unwrap(), h.find("20 KB").unwrap(), h.find_all("Text")[0]);
        // 升の文字は、見出しの文字と同じく升の 8 内側
        assert!((b.x - name.x - 8.0).abs() <= 1.0, "1 列目の見出しと升の左がそろう: {name:?} {b:?}");
        assert!(size.x > name.x + 200.0 - 1.0, "2 列目は 1 列目の幅の後");
        assert!((size.right() - 8.0 - b_size.right()).abs() <= 1.0, "右寄せの列: 見出しと升の右がそろう {size:?} {b_size:?}");
        assert!(b_size.x > size.x, "右寄せの升の文字は右へ");
        assert!((b_kind.x - kind.x - 8.0).abs() <= 1.0);
        assert!(b.y > name.bottom() - 1.0, "升は見出しの下");
    }

    #[test]
    fn tables_sort_by_clicking_headers_and_resize_columns() {
        let mut h = Harness::new(FileTable::default());
        h.click("Size");
        assert_eq!(h.app().sort, Some((1, true)));
        let first = |h: &Harness<FileTable>| ["a.png", "b.txt", "c.md"].into_iter().min_by(|x, y| h.find(x).unwrap().y.total_cmp(&h.find(y).unwrap().y)).unwrap();
        assert_eq!(first(&h), "c.md", "小さい順");
        assert_eq!(a11y_node(&h, "Size").sort_direction(), Some(accesskit::SortDirection::Ascending));
        h.click("Size");
        assert_eq!(h.app().sort, Some((1, false)));
        assert_eq!(first(&h), "a.png", "大きい順");
        h.click("Name");
        assert_eq!(h.app().sort, Some((0, true)));
        assert_eq!(a11y_node(&h, "Size").sort_direction(), None);
        // 見出しの右のつまみで列の幅を変える
        let size_x = h.find("Size").unwrap().x;
        h.drag("Resize column", 40.0, 0.0);
        assert!((h.find("Size").unwrap().x - (size_x + 40.0)).abs() <= 1.0, "1 列目が 40 広がる");
        h.drag("Resize column", -1000.0, 0.0);
        assert!(h.find("Size").unwrap().x >= h.find("Name").unwrap().x + 40.0, "狭くても 40");
    }

    #[test]
    fn tables_tell_screen_readers_rows_cells_and_headers() {
        let mut h = Harness::new(FileTable::default());
        let nodes = a11y_nodes(&h);
        let table = nodes.iter().find(|n| n.role() == accesskit::Role::Table).expect("表");
        assert_eq!((table.label(), table.row_count(), table.column_count()), (Some("Files"), Some(4), Some(3)));
        assert_eq!(nodes.iter().filter(|n| n.role() == accesskit::Role::ColumnHeader).count(), 3);
        let rows: Vec<_> = nodes.iter().filter(|n| n.role() == accesskit::Role::Row).collect();
        assert_eq!(rows.len(), 4, "見出しの行と 3 行");
        assert_eq!(rows.iter().filter_map(|r| r.row_index()).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        let cells: Vec<_> = nodes.iter().filter(|n| n.role() == accesskit::Role::Cell).collect();
        assert_eq!(cells.len(), 9);
        assert_eq!(cells.iter().take(3).filter_map(|c| c.column_index()).collect::<Vec<_>>(), vec![0, 1, 2]);
        // 行を選ぶ
        h.click("a.png");
        assert_eq!(h.app().sel, Some(1));
        h.press(Key::Down);
        assert_eq!(h.app().sel, Some(2));
        // 見出しは Tab で 1 回、左右で移り、Enter で並べ替える
        h.press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("Name"));
        h.press(Key::Right).press(Key::Enter);
        assert_eq!(h.app().sort, Some((1, true)));
    }

    /// 日付の選択の見本
    #[derive(Default)]
    struct Due {
        date: Option<crate::Date>,
        japanese: bool,
    }

    impl App for Due {
        fn view(&mut self, ui: &mut Ui) {
            if self.japanese {
                ui.set_date_text(crate::DateText::japanese());
            }
            ui.date_picker(&mut self.date).label("Due date");
            ui.button("Other");
        }
    }

    fn date(y: i32, m: u32, d: u32) -> crate::Date {
        crate::Date::new(y, m, d).unwrap()
    }

    #[test]
    fn date_pickers_open_a_month_and_pick_with_the_pointer() {
        let mut h = Harness::new(Due::default());
        assert_eq!(h.find("Pick a date"), None, "名前は label");
        assert_eq!(a11y_node(&h, "Due date").value(), Some("Pick a date"));
        h.click("Due date");
        let (field, title) = (h.find("Due date").unwrap(), h.find("November 2026").expect("今日の月が開く"));
        assert!(title.y > field.bottom());
        assert_eq!(focused_name(&h).as_deref(), Some("November 22, 2026"), "焦点は今日");
        let today = a11y_node(&h, "November 22, 2026");
        assert_eq!(today.role(), accesskit::Role::GridCell);
        h.click("November 25, 2026");
        assert_eq!(h.app().date, Some(date(2026, 11, 25)));
        assert!(h.find("November 2026").is_none(), "選ぶと閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("Due date"));
        let n = a11y_node(&h, "Due date");
        assert_eq!((n.role(), n.value()), (accesskit::Role::DateInput, Some("2026-11-25")));
        // 開き直すと選んだ日に焦点。次の月へ
        h.click("Due date");
        assert_eq!(focused_name(&h).as_deref(), Some("November 25, 2026"));
        assert_eq!(a11y_node(&h, "November 25, 2026").is_selected(), Some(true));
        h.click("Next month");
        assert!(h.find("December 2026").is_some());
        h.click("December 1, 2026");
        assert_eq!(h.app().date, Some(date(2026, 12, 1)));
        // 外を押すと選ばずに閉じる
        h.click("Due date").click("Other");
        assert!(h.find("December 2026").is_none());
        assert_eq!(h.app().date, Some(date(2026, 12, 1)));
    }

    #[test]
    fn date_pickers_work_with_the_keyboard() {
        let mut h = Harness::new(Due::default());
        h.press(Key::Tab).press(Key::Enter);
        assert_eq!(focused_name(&h).as_deref(), Some("November 22, 2026"));
        h.press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("November 23, 2026"));
        h.press(Key::Down);
        assert_eq!(focused_name(&h).as_deref(), Some("November 30, 2026"));
        h.press(Key::Down);
        assert_eq!(focused_name(&h).as_deref(), Some("December 7, 2026"), "月をまたぐと次の月を出す");
        assert!(h.find("December 2026").is_some());
        h.press(Key::PageUp);
        assert_eq!(focused_name(&h).as_deref(), Some("November 7, 2026"));
        h.press_with(Key::PageDown, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("November 7, 2027"), "Shift で 1 年");
        h.press(Key::Home);
        assert_eq!(focused_name(&h).as_deref(), Some("November 7, 2027"), "2027-11-07 は日曜日（週の初め）");
        h.press(Key::End);
        assert_eq!(focused_name(&h).as_deref(), Some("November 13, 2027"));
        h.press(Key::Enter);
        assert_eq!(h.app().date, Some(date(2027, 11, 13)));
        assert_eq!(focused_name(&h).as_deref(), Some("Due date"));
        // Esc は選ばずに閉じる
        h.press_with(Key::Down, Mods::ALT).press(Key::Left).press(Key::Escape);
        assert_eq!(h.app().date, Some(date(2027, 11, 13)));
        assert!(h.find("November 2027").is_none());
    }

    #[test]
    fn date_pickers_speak_japanese_when_asked() {
        let mut h = Harness::new(Due { date: None, japanese: true });
        assert_eq!(a11y_node(&h, "Due date").value(), Some("日付を選ぶ"));
        h.click("Due date");
        assert!(h.find("2026年11月").is_some());
        assert!(h.find("日").is_some() && h.find("土").is_some());
        assert_eq!(focused_name(&h).as_deref(), Some("2026年11月22日"));
        assert!(h.find("次の月").is_some());
    }

    /// 複数の窓の見本: 設定の窓で変えた値を、元の窓にも出す
    #[derive(Default)]
    struct TwoWindows {
        open: bool,
        volume: i32,
    }

    impl App for TwoWindows {
        fn view(&mut self, ui: &mut Ui) {
            if ui.button("Settings").clicked() {
                self.open = true;
            }
            ui.label(format!("Volume {}", self.volume));
            let options = crate::Options { size: (320.0, 200.0), ..crate::Options::new("Settings") };
            ui.window("settings", &options, &mut self.open);
        }

        fn window(&mut self, key: &str, ui: &mut Ui) {
            if key == "settings" {
                ui.slider(&mut self.volume, 0..=10).label("Volume");
                if ui.button("Close").clicked() {
                    self.open = false;
                }
            }
        }
    }

    #[test]
    fn apps_open_more_windows_that_share_the_app() {
        let mut h = Harness::new(TwoWindows::default());
        assert!(h.windows().is_empty());
        h.click("Settings");
        assert_eq!(h.windows(), vec!["settings".to_string()]);
        assert!(h.find("Volume 0").is_some(), "操作するのは、まだ元の窓");
        h.window("settings");
        assert!(h.find("Settings").is_none() && h.find("Volume").is_some(), "設定の窓の中");
        assert!(h.find("Close").unwrap().right() <= 320.0, "設定の窓の大きさ");
        h.press(Key::Tab).press(Key::Right).press(Key::Right);
        assert_eq!(h.app().volume, 2);
        h.main_window();
        assert!(h.find("Volume 2").is_some(), "元の窓も描き直す");
        h.window("settings").click("Close");
        assert!(h.windows().is_empty(), "アプリが閉じた");
        assert!(!h.app().open);
        assert!(h.find("Settings").is_some(), "閉じた窓を操作していたら、元の窓へ戻る");
        // 利用者が窓の閉じるボタンで閉じた
        h.click("Settings");
        assert_eq!(h.windows().len(), 1);
        h.close_window("settings");
        assert!(!h.app().open && h.windows().is_empty());
    }

    /// ファイルのダイアログの見本
    #[derive(Default)]
    struct FileApp {
        opened: Vec<std::path::PathBuf>,
        saved: Option<std::path::PathBuf>,
    }

    impl App for FileApp {
        fn view(&mut self, ui: &mut Ui) {
            if ui.button("Open").clicked() {
                ui.file_dialog("open", crate::FileDialog::open().title("Open notes").filter("Text", &["txt", "md"]).multiple());
            }
            if let Some(files) = ui.chosen_files("open") {
                self.opened.extend(files);
            }
            if ui.button("Save as").clicked() {
                ui.file_dialog("save", crate::FileDialog::save().file_name("notes.txt"));
            }
            if let Some(path) = ui.chosen_file("save") {
                self.saved = Some(path);
            }
        }
    }

    #[test]
    fn file_dialogs_are_shown_after_the_frame_and_answer_in_a_later_one() {
        let mut h = Harness::new(FileApp::default());
        assert_eq!(h.file_dialog_key(), None);
        h.click("Open");
        assert_eq!(h.file_dialog_key().as_deref(), Some("open"));
        let d = h.file_dialog().unwrap();
        assert_eq!((d.is_save(), d.allows_multiple()), (false, true));
        h.choose_files(&["C:/notes/a.txt", "C:/notes/b.md"]);
        assert_eq!(h.app().opened, vec![std::path::PathBuf::from("C:/notes/a.txt"), std::path::PathBuf::from("C:/notes/b.md")]);
        assert_eq!(h.file_dialog_key(), None, "答えたら無くなる");
        // 受け取るのは 1 回だけ
        h.frame();
        assert_eq!(h.app().opened.len(), 2);
        // 取り消し
        h.click("Save as");
        assert!(h.file_dialog().unwrap().is_save());
        h.cancel_file_dialog();
        assert_eq!(h.app().saved, None);
        h.click("Save as").choose_files(&["D:/out.txt"]);
        assert_eq!(h.app().saved, Some(std::path::PathBuf::from("D:/out.txt")));
    }

    /// 閉じる前の確認の見本: 保存していない変更があれば、窓の中のダイアログで聞く
    #[derive(Default)]
    struct Editor2 {
        dirty: bool,
        asking: bool,
    }

    impl App for Editor2 {
        fn view(&mut self, ui: &mut Ui) {
            if ui.button("Type").clicked() {
                self.dirty = true;
            }
            let mut quit = false;
            ui.dialog(&mut self.asking, "Unsaved changes", |ui| {
                if ui.button("Discard").clicked() {
                    quit = true;
                }
            });
            if quit {
                ui.quit();
            }
        }

        fn close_requested(&mut self) -> bool {
            if self.dirty {
                self.asking = true;
            }
            !self.dirty
        }
    }

    #[test]
    fn apps_can_ask_before_the_window_closes() {
        let mut h = Harness::new(Editor2::default());
        h.request_close();
        assert!(h.is_closed(), "変更が無ければすぐ閉じる");
        let mut h = Harness::new(Editor2::default());
        h.click("Type").request_close();
        assert!(!h.is_closed(), "変更があれば閉じない");
        assert!(h.find("Unsaved changes").is_some(), "アプリが聞く");
        h.click("Discard");
        assert!(h.is_closed(), "ui.quit で閉じる");
    }

    /// 伸ばしたタブの中身は残りの高さを取る（家計簿を書いて見つけた: 中の表の高さが 0 で行が無かった）
    struct GrownTabs;

    impl App for GrownTabs {
        fn view(&mut self, ui: &mut Ui) {
            let mut page = 0;
            ui.tabs(&mut page, &[(0, "Rows")], |ui, _| {
                ui.list(50, 20.0).grow().show(|ui, i| {
                    ui.label(format!("Row {i}"));
                });
            })
            .grow();
        }
    }

    #[test]
    fn grown_tabs_give_their_panel_the_remaining_height() {
        let h = Harness::new(GrownTabs);
        assert!(h.find("Row 10").is_some(), "中の一覧が残りの高さを取る");
    }

    /// 同じ名前のボタンとメニューの項目（ファイルの閲覧を書いて見つけた: 試験台が開いたメニューの外のボタンを押していた）
    #[derive(Default)]
    struct SameName {
        from_menu: u32,
        from_button: u32,
    }

    impl App for SameName {
        fn view(&mut self, ui: &mut Ui) {
            if ui.button("Delete").clicked() {
                self.from_button += 1;
            }
            ui.menu_button("More", |m| {
                if m.item("Delete").clicked() {
                    self.from_menu += 1;
                }
            });
        }
    }

    #[test]
    fn the_harness_clicks_what_a_user_would_see_on_top() {
        let mut h = Harness::new(SameName::default());
        h.click("More").click("Delete");
        assert_eq!((h.app().from_menu, h.app().from_button), (1, 0), "メニューが開いている間はメニューの項目");
        h.click("Delete");
        assert_eq!((h.app().from_menu, h.app().from_button), (1, 1), "閉じていればボタン");
    }

    /// ドラッグ＆ドロップの見本: 2 つの列の間でカードを移す。「Bin」は別の型の荷物（Tool）だけを受け取る
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct CardAt(usize, usize);
    struct Tool;

    struct Board {
        cols: [Vec<&'static str>; 2],
        taps: u32,
        binned: u32,
    }

    impl App for Board {
        fn view(&mut self, ui: &mut Ui) {
            let mut moved = None;
            ui.row(|ui| {
                for (c, cards) in self.cols.iter().enumerate() {
                    let mut col = ui
                        .column(|ui| {
                            ui.heading(if c == 0 { "To do" } else { "Done" });
                            for (k, name) in cards.iter().enumerate() {
                                ui.label(*name).drag_source(CardAt(c, k));
                            }
                        })
                        .width(Size::Fixed(160.0))
                        .height(Size::Fixed(200.0));
                    if let Some(card) = col.dropped::<CardAt>() {
                        moved = Some((card, c));
                    }
                }
                let mut bin = ui
                    .column(|ui| {
                        ui.label("Bin");
                    })
                    .width(Size::Fixed(100.0))
                    .height(Size::Fixed(200.0));
                if bin.dropped::<Tool>().is_some() {
                    self.binned += 1;
                }
                ui.row(|ui| {
                    if ui.button("Tap").clicked() {
                        self.taps += 1;
                    }
                })
                .drag_source(Tool);
            });
            if let Some((CardAt(from, k), to)) = moved {
                let card = self.cols[from].remove(k);
                self.cols[to].push(card);
            }
        }
    }

    fn board() -> Harness<Board> {
        Harness::with_size(Board { cols: [vec!["Write", "Read"], vec![]], taps: 0, binned: 0 }, 640.0, 300.0, 1.0)
    }

    fn mid(h: &Harness<Board>, name: &str) -> (f32, f32) {
        let r = h.find(name).unwrap_or_else(|| panic!("{name}"));
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn dragging_a_card_to_another_column_drops_it_there() {
        let mut h = board();
        h.drag_to("Write", "Done");
        assert_eq!(h.app().cols, [vec!["Read"], vec!["Write"]]);
        assert!(h.cur().cx.dnd.is_none() && h.cur().cx.drop.is_none(), "終わったら何も残らない");
    }

    #[test]
    fn a_drag_that_arrives_in_one_go_still_drops() {
        // 押す・動かす・離すが 1 回に届いても（素早く動かした時）、荷物は離した後のビューで受け取る
        let mut h = board();
        let ((x, y), (x2, y2)) = (mid(&h, "Read"), mid(&h, "Done"));
        h.drag("Read", x2 - x, y2 - y);
        assert_eq!(h.app().cols, [vec!["Write"], vec!["Read"]]);
    }

    #[test]
    fn escape_and_releasing_outside_any_target_cancel() {
        let mut h = board();
        let ((x, y), (x2, y2)) = (mid(&h, "Write"), mid(&h, "Done"));
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
        h.send(vec![Event::PointerMove { x: x2, y: y2 }]);
        assert!(h.cur().cx.dnd.is_some(), "4 ピクセルより動いたら引っ張り始める");
        h.press(Key::Escape);
        assert!(h.cur().cx.dnd.is_none(), "Esc で取り消す");
        h.send(vec![Event::PointerUp { x: x2, y: y2 }]);
        assert_eq!(h.app().cols[1], Vec::<&str>::new(), "取り消した後に離しても落とさない");
        // 受け取れる所の外（窓の下の端）で離す
        h.drag("Write", 0.0, 290.0 - y);
        assert_eq!(h.app().cols, [vec!["Write", "Read"], vec![]]);
    }

    #[test]
    fn a_target_takes_only_its_own_kind_of_payload() {
        let mut h = board();
        h.drag_to("Write", "Bin");
        assert_eq!((h.app().cols[0].len(), h.app().binned), (2, 0), "カードは Bin に落とせない");
        h.drag_to("Tap", "Bin");
        assert_eq!(h.app().binned, 1);
        assert_eq!(h.app().taps, 0, "引っ張ったボタンは押したことにならない");
    }

    #[test]
    fn a_short_move_is_still_a_click() {
        let mut h = board();
        h.drag("Tap", 3.0, 0.0);
        assert_eq!(h.app().taps, 1, "4 ピクセル未満はクリック");
        assert!(h.cur().cx.dnd.is_none());
        // 作者が本物の窓で「移す」を押した時、横 2・縦 4 ずれて引っ張りになった（2026-11-22 の記録）。Windows の
        // SM_CXDRAG・SM_CYDRAG と同じく、横か縦のどちらかが 4 を超えるまではクリック
        h.drag("Tap", 2.0, 4.0);
        assert_eq!(h.app().taps, 2, "横 2・縦 4 はクリック");
        h.drag("Tap", 0.0, 5.0);
        assert_eq!(h.app().taps, 2, "縦 5 は引っ張り");
    }

    #[test]
    fn the_target_under_the_pointer_is_known_while_dragging() {
        let mut h = board();
        let ((x, y), (x2, y2), (bx, by)) = (mid(&h, "Write"), mid(&h, "Done"), mid(&h, "Bin"));
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }]);
        h.send(vec![Event::PointerMove { x: x + 10.0, y }]);
        h.send(vec![Event::PointerMove { x: x2, y: y2 }]);
        let over = h.cur().cx.dnd.as_ref().and_then(|d| d.over);
        assert!(over.is_some(), "Done の列の上");
        assert_eq!(h.cur().cx.drag_cursor(x2, y2), Some(true), "受け取れる所の上はふつうの矢印");
        h.send(vec![Event::PointerMove { x: bx, y: by }]);
        assert_eq!(h.cur().cx.dnd.as_ref().and_then(|d| d.over), None, "Bin はカードを受け取らない");
        assert_eq!(h.cur().cx.drag_cursor(bx, by), Some(false), "受け取れない所は禁止の形");
        h.send(vec![Event::PointerUp { x: bx, y: by }]);
        assert_eq!(h.cur().cx.drag_cursor(bx, by), None);
    }

    /// ほかのアプリ（エクスプローラー）から落とすファイル: Vec<PathBuf> として届く
    struct FileShelf {
        opened: Vec<std::path::PathBuf>,
        accept: bool,
    }

    impl App for FileShelf {
        fn view(&mut self, ui: &mut Ui) {
            ui.row(|ui| {
                let mut shelf = ui
                    .column(|ui| {
                        ui.heading("Drop files here");
                        for p in &self.opened {
                            ui.label(p.display().to_string());
                        }
                    })
                    .width(Size::Fixed(300.0))
                    .height(Size::Fixed(200.0));
                if self.accept
                    && let Some(paths) = shelf.dropped::<Vec<std::path::PathBuf>>() {
                        self.opened.extend(paths);
                    }
                ui.label("Not here");
                // 窓の中の引っ張り（ファイルの行）も、同じ型なら同じ部品が受け取る
                ui.label("notes.txt").drag_source(vec![std::path::PathBuf::from("notes.txt")]);
            });
        }
    }

    fn shelf(accept: bool) -> Harness<FileShelf> {
        Harness::with_size(FileShelf { opened: vec![], accept }, 640.0, 240.0, 1.0)
    }

    #[test]
    fn files_dropped_from_another_app_arrive_as_paths() {
        let mut h = shelf(true);
        assert!(h.cur().cx.wants_files, "ファイルを受け取る部品があれば、窓が OS に届け出る印を立てる");
        h.drop_files("Drop files here", &["C:\\docs\\a.txt", "C:\\docs\\b.md"]);
        assert_eq!(h.app().opened, [std::path::PathBuf::from("C:\\docs\\a.txt"), std::path::PathBuf::from("C:\\docs\\b.md")]);
        assert!(h.cur().cx.dnd.is_none() && h.cur().cx.drop.is_none());
        assert!(h.find("C:\\docs\\a.txt").is_some(), "落とした後のフレームで描いている");
    }

    #[test]
    fn files_over_a_place_that_does_not_take_them_show_not_allowed_and_leaving_cancels() {
        let mut h = shelf(true);
        let (nx, ny) = mid_of(&h, "Not here");
        let (sx, sy) = mid_of(&h, "Drop files here");
        h.send(vec![Event::FileDragEnter { x: nx, y: ny, paths: vec!["C:\\x.txt".into()] }]);
        assert!(h.cur().cx.dnd.as_ref().is_some_and(|d| d.external), "ほかのアプリからの引っ張り（影は描かない）");
        assert_eq!(h.cur().cx.drag_cursor(nx, ny), Some(false));
        assert!(!h.cur().cx.accepts_files_at(nx, ny));
        assert!(h.cur().cx.accepts_files_at(sx, sy), "窓の処理が OS に答える（その場で前のフレームの配置から）");
        h.send(vec![Event::FileDragOver { x: sx, y: sy }]);
        assert!(h.cur().cx.dnd.as_ref().and_then(|d| d.over).is_some(), "受け取る部品を囲む");
        h.send(vec![Event::FileDragLeave]);
        assert!(h.cur().cx.dnd.is_none(), "窓の外へ出たら取り消す");
        h.send(vec![Event::FileDragEnter { x: nx, y: ny, paths: vec!["C:\\x.txt".into()] }, Event::FileDrop { x: nx, y: ny }]);
        assert!(h.app().opened.is_empty(), "受け取らない所に落としても何も起きない");
    }

    #[test]
    fn a_lost_pointer_does_not_cancel_files_from_another_app() {
        // 作者の記録（2026-11-22 06:48:29）: エクスプローラーから落とす直前に、ボタンの離れたマウスの動きが窓に届き、
        // 「つかみが外れた」として取り消していた。マウスをつかんでいるのはエクスプローラーなので、取り消さない
        let mut h = shelf(true);
        let (sx, sy) = mid_of(&h, "Drop files here");
        h.send(vec![Event::FileDragEnter { x: sx, y: sy, paths: vec!["C:\\a.txt".into()] }]);
        h.send(vec![Event::PointerCancel, Event::FileDrop { x: sx, y: sy }]);
        assert_eq!(h.app().opened, [std::path::PathBuf::from("C:\\a.txt")]);
    }

    #[test]
    fn a_window_without_file_targets_does_not_ask_for_files() {
        let h = shelf(false);
        assert!(!h.cur().cx.wants_files, "使わないアプリは OS に届け出ない（起動を遅くしない）");
    }

    #[test]
    fn an_in_window_drag_of_paths_lands_on_the_same_target() {
        let mut h = shelf(true);
        h.drag_to("notes.txt", "Drop files here");
        assert_eq!(h.app().opened, [std::path::PathBuf::from("notes.txt")]);
    }

    fn mid_of<A: App>(h: &Harness<A>, name: &str) -> (f32, f32) {
        let r = h.find(name).unwrap_or_else(|| panic!("{name}"));
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// 動き: animate_bool の値を毎フレーム覚える
    struct Fader {
        on: bool,
        value: f32,
        wifi: bool,
    }

    impl App for Fader {
        fn view(&mut self, ui: &mut Ui) {
            if ui.button("Flip").clicked() {
                self.on = !self.on;
            }
            self.value = ui.animate_bool("fade", self.on);
            ui.toggle(&mut self.wifi, "Wi-Fi");
        }
    }

    fn fader() -> Harness<Fader> {
        let mut h = Harness::new(Fader { on: false, value: -1.0, wifi: false });
        h.reduce_motion(false);
        h
    }

    #[test]
    fn animate_bool_moves_over_250_ms_slowing_down_and_then_stops() {
        use std::time::Duration;
        let mut h = fader();
        assert_eq!(h.app().value, 0.0, "初めて現れた時は動かさない");
        h.click("Flip");
        assert_eq!(h.app().value, 0.0, "押したフレームはまだ動き始め");
        h.wait(Duration::from_millis(125));
        let mid = h.app().value;
        assert!(mid > 0.6 && mid < 1.0, "減速の曲線なので、半分の時間で半分より先: {mid}");
        h.wait(Duration::from_millis(125));
        assert_eq!(h.app().value, 1.0, "250 ms で目標に着く");
        h.wait(Duration::from_millis(500));
        assert_eq!(h.app().value, 1.0, "着いたら止まる");
        assert!(h.cur().cx.anims.values().all(|a| a.done(h.cur().cx.now)), "動いている物は残らない");
    }

    #[test]
    fn a_value_seen_for_the_first_time_does_not_ask_for_frames() {
        use std::time::Duration;
        // 初めて呼んだ値は動かないので、フレームを頼まない（頼むと、切り替えのある画面が起動のたびに 167 ms 描き続ける）
        let mut h = fader();
        h.rt.cx.want_frame = false;
        let v = h.rt.cx.animate(crate::ui::Id(42), 1.0, Duration::from_millis(250));
        assert_eq!(v, 1.0);
        assert!(!h.rt.cx.want_frame, "動かない値でフレームを頼んだ");
        let now = h.rt.cx.now;
        assert!(h.rt.cx.anims.values().all(|a| a.done(now)), "初めて現れた値は、もう止まっている");
    }

    #[test]
    fn turning_back_midway_starts_from_where_it_is() {
        use std::time::Duration;
        let mut h = fader();
        h.click("Flip");
        h.wait(Duration::from_millis(60));
        let at = h.app().value;
        assert!(at > 0.2 && at < 0.9, "途中: {at}");
        h.click("Flip");
        assert!((h.app().value - at).abs() < 1e-4, "向きを変えたフレームは、今いる所から（飛ばない）");
        h.wait(Duration::from_millis(250));
        assert_eq!(h.app().value, 0.0);
    }

    #[test]
    fn with_reduced_motion_values_jump_to_the_target() {
        let mut h = fader();
        h.reduce_motion(true);
        assert!(h.cur().cx.reduce_motion);
        h.click("Flip");
        assert_eq!(h.app().value, 1.0, "動きを減らす設定では、すぐ目標");
    }

    struct Drawer {
        open: bool,
        clicks: u32,
    }

    impl App for Drawer {
        fn view(&mut self, ui: &mut Ui) {
            ui.expander(&mut self.open, "More", |ui| {
                ui.label("Line one");
                if ui.button("Deep button").clicked() {
                    self.clicks += 1;
                }
            });
            ui.label("Below");
        }
    }

    #[test]
    fn expanders_slide_open_and_closed_and_cut_off_what_is_not_shown() {
        use std::time::Duration;
        let mut h = Harness::new(Drawer { open: false, clicks: 0 });
        h.reduce_motion(false);
        let closed = h.find("Below").unwrap().y;
        h.click("More");
        assert!(h.app().open);
        h.wait(Duration::from_millis(15));
        // 開いている途中: 下の部品は少し下がるだけで、まだ見えていないボタンは押せない
        let (mid, button) = (h.find("Below").unwrap().y, h.find("Deep button").unwrap());
        assert!(mid > closed && mid < button.y, "途中: {closed} < {mid} < {}", button.y);
        h.click_at(button.x + button.w / 2.0, button.y + button.h / 2.0);
        assert_eq!(h.app().clicks, 0, "切り取られた所は押せない");
        h.press(Key::Tab).press(Key::Tab);
        assert_ne!(focused_name(&h).as_deref(), Some("Deep button"), "全部隠れた部品へは Tab で移らない");
        h.wait(Duration::from_millis(250));
        h.click("Deep button");
        assert_eq!(h.app().clicks, 1, "開き終われば押せる");
        // 閉じる時も、中身を描きながら縮み、終わったら消える
        h.click("More");
        assert!(!h.app().open);
        h.wait(Duration::from_millis(60));
        assert!(h.find("Line one").is_some(), "閉じている途中は、まだ描く");
        h.wait(Duration::from_millis(250));
        assert!(h.find("Line one").is_none());
        assert_eq!(h.find("Below").unwrap().y, closed);
    }

    #[test]
    fn the_switch_thumb_slides_in_167_ms() {
        use std::time::Duration;
        let pos = |h: &Harness<Fader>| {
            h.cur().tree.info.iter().find_map(|n| match &n.kind {
                crate::ui::Kind::Toggle { pos, .. } => Some(*pos),
                _ => None,
            })
        };
        let mut h = fader();
        assert_eq!(pos(&h), Some(0.0));
        h.click("Wi-Fi");
        assert!(h.app().wifi);
        h.wait(Duration::from_millis(80));
        let p = pos(&h).unwrap();
        assert!(p > 0.3 && p < 1.0, "途中: {p}");
        h.wait(Duration::from_millis(100));
        assert_eq!(pos(&h), Some(1.0));
    }

    struct Typeset {
        code: String,
        mono_window: bool,
    }

    impl App for Typeset {
        fn view(&mut self, ui: &mut Ui) {
            if self.mono_window {
                ui.set_font(crate::Font::Monospace);
            }
            // 行の中のラベルは中身の幅になる（縦の並びでは幅いっぱいに伸びる）
            ui.row(|ui| {
                ui.label("iiiiiiii");
            });
            ui.row(|ui| {
                ui.label("MMMMMMMM");
            });
            ui.row(|ui| {
                ui.label("iiii mono").monospace();
            });
            ui.row(|ui| {
                ui.label("MMMM mono").monospace();
            });
            ui.text_area(&mut self.code).hint("Code").monospace();
        }
    }

    #[test]
    fn labels_take_their_own_font_and_set_font_changes_the_rest() {
        let mut h = Harness::new(Typeset { code: String::new(), mono_window: false });
        let w = |h: &Harness<Typeset>, name: &str| h.find(name).unwrap().w;
        assert!((w(&h, "iiii mono") - w(&h, "MMMM mono")).abs() < 1.0, "等幅のラベル");
        assert!(w(&h, "MMMMMMMM") > w(&h, "iiiiiiii") * 1.5, "窓の書体（UI）のラベル");
        h.app_mut().mono_window = true;
        h.frame();
        assert!((w(&h, "MMMMMMMM") - w(&h, "iiiiiiii")).abs() < 1.0, "set_font の後は窓の書体が等幅");
    }

    #[test]
    fn moving_the_caret_between_lines_measures_in_the_fields_font() {
        let mut h = Harness::new(Typeset { code: "MMMM\naaaaaaaa".into(), mono_window: false });
        h.click("Code").press_with(Key::Home, Mods::CTRL).press(Key::End);
        let id = id_named(&h, "Code");
        let cursor = |h: &Harness<Typeset>| h.cur().cx.edits.get(&id).unwrap().cursor;
        assert_eq!(cursor(&h), 4, "1 行目の終わり");
        // 等幅なので、M 4 つ分の位置は a 4 つ分（UI の書体なら M の方が広いので、もっと右になる）
        h.press(Key::Down);
        assert_eq!(cursor(&h), 9);
    }

    struct Clock {
        start: Option<crate::Time>,
        step: u32,
    }

    impl App for Clock {
        fn view(&mut self, ui: &mut Ui) {
            ui.time_picker(&mut self.start).label("Start").minute_step(self.step);
            ui.button("Other");
        }
    }

    fn clock(step: u32) -> Harness<Clock> {
        Harness::new(Clock { start: None, step })
    }

    fn time(h: u32, m: u32) -> Option<crate::Time> {
        crate::Time::new(h, m)
    }

    #[test]
    fn picking_an_hour_then_a_minute_sets_the_time_and_closes() {
        let mut h = clock(5);
        assert!(h.find("09 Hours").is_none(), "閉じている");
        h.click("Start");
        assert!(h.find("09 Hours").is_some() && h.find("55 Minutes").is_some(), "時と分のます目が出る");
        h.click("09 Hours");
        assert_eq!(h.app().start, time(9, 0), "時を選ぶと、分は今の時刻（試験台では 12:00）の分");
        assert_eq!(focused_name(&h).as_deref(), Some("00 Minutes"), "時を選ぶと分へ移る");
        assert!(h.find("30 Minutes").is_some(), "まだ開いている");
        h.click("30 Minutes");
        assert_eq!(h.app().start, time(9, 30));
        assert!(h.find("30 Minutes").is_none(), "分を選ぶと閉じる");
        assert_eq!(focused_name(&h).as_deref(), Some("Start"), "閉じると欄に戻る");
        assert_eq!(a11y_node(&h, "Start").value(), Some("09:30"));
        assert_eq!(a11y_node(&h, "Start").role(), accesskit::Role::TimeInput);
    }

    #[test]
    fn the_time_grids_follow_the_arrow_keys_and_escape_keeps_the_time() {
        let mut h = clock(5);
        // キーボードだけで: Other から Shift+Tab で欄へ
        h.click("Other").press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("Start"));
        h.press(Key::Enter);
        assert_eq!(focused_name(&h).as_deref(), Some("12 Hours"), "時刻が無ければ今の時（試験台では 12 時）から");
        h.press(Key::Down).press(Key::Right);
        assert_eq!(focused_name(&h).as_deref(), Some("19 Hours"), "下は 6 時あと、右は 1 時あと");
        h.press(Key::Enter);
        assert_eq!(h.app().start, time(19, 0));
        h.press(Key::Right).press(Key::Right).press(Key::Space);
        assert_eq!(h.app().start, time(19, 10), "分は 5 分ずつ");
        assert!(h.find("10 Minutes").is_none(), "閉じた");
        // 開いて Esc なら、時刻は変わらない
        h.press(Key::Enter);
        assert_eq!(focused_name(&h).as_deref(), Some("19 Hours"), "開くと今の時刻の時に焦点");
        h.press(Key::Left).press(Key::Escape);
        assert!(h.find("19 Hours").is_none());
        assert_eq!(h.app().start, time(19, 10));
    }

    #[test]
    fn minute_steps_decide_the_minutes_offered() {
        let mut h = clock(15);
        h.click("Start");
        let offered: Vec<u32> = (0..60).filter(|m| h.find(&format!("{m:02} Minutes")).is_some()).collect();
        assert_eq!(offered, [0, 15, 30, 45]);
        let mut h = clock(1);
        h.click("Start");
        assert_eq!((0..60).filter(|m| h.find(&format!("{m:02} Minutes")).is_some()).count(), 60, "1 分ずつなら 60 個");
    }

    struct Paint {
        color: crate::Color,
        alpha: bool,
    }

    impl App for Paint {
        fn view(&mut self, ui: &mut Ui) {
            let picker = ui.color_picker(&mut self.color).label("Fill");
            if self.alpha {
                picker.alpha();
            }
            ui.button("Other");
        }
    }

    fn paint(color: u32) -> Harness<Paint> {
        Harness::new(Paint { color: crate::rgb(color), alpha: false })
    }

    fn hex_of(h: &Harness<Paint>) -> String {
        crate::color::hex(h.app().color, true)
    }

    #[test]
    fn a_ready_made_color_or_a_typed_code_sets_the_color() {
        let mut h = paint(0x1a73e8);
        assert_eq!(a11y_node(&h, "Fill").value(), Some("#1A73E8"));
        assert_eq!(a11y_node(&h, "Fill").role(), accesskit::Role::ColorWell);
        h.click("Fill");
        assert!(h.find("Hue").is_some() && h.find("Opacity").is_none(), "不透明度の帯は .alpha() の時だけ");
        h.click("#D93025");
        assert_eq!(hex_of(&h), "#D93025");
        assert_eq!(a11y_node(&h, "Fill").value(), Some("#D93025"));
        assert!(h.find("Hue").is_some(), "見本を選んでも開いたまま");
        // 番号を打つと、読める番号になった時に色が変わる
        h.click("Color code").press_with(Key::A, Mods::CTRL).type_text("#00ff0");
        assert_eq!(hex_of(&h), "#D93025", "途中（5 桁）では変えない");
        h.type_text("0");
        assert_eq!(hex_of(&h), "#00FF00");
        h.press(Key::Escape);
        assert!(h.find("Hue").is_none(), "Esc で閉じる");
        assert_eq!(hex_of(&h), "#00FF00", "閉じても選んだ色のまま");
    }

    #[test]
    fn the_square_and_the_hue_bar_follow_the_pointer_and_the_keys() {
        let mut h = paint(0x808080);
        h.click("Fill");
        // 右上の画素を押すと、彩度と明るさが最大（灰色の色相は 0 = 赤）
        let r = h.find("Saturation and brightness").unwrap();
        h.click_at(r.right() - 1.0, r.y);
        assert_eq!(hex_of(&h), "#FF0000");
        // 色相の帯の真ん中は 180 度（シアン）
        let b = h.find("Hue").unwrap();
        h.click_at(b.x + b.w / 2.0, b.y + b.h / 2.0);
        let c = h.app().color;
        assert!(c[0] < 0.05 && c[1] > 0.95 && c[2] > 0.95, "{}", hex_of(&h));
        // キー: 色相の帯で Shift+左 を 3 回（-30 度）、面で下を 50 回（明るさ -50%）
        h.press_with(Key::Left, Mods::SHIFT).press_with(Key::Left, Mods::SHIFT).press_with(Key::Left, Mods::SHIFT);
        let [hue, _, _] = crate::color::rgb_to_hsv([h.app().color[0], h.app().color[1], h.app().color[2]]);
        assert!((hue - 150.0).abs() < 1.5, "{hue}");
        h.press_with(Key::Tab, Mods::SHIFT);
        assert_eq!(focused_name(&h).as_deref(), Some("Saturation and brightness"));
        for _ in 0..50 {
            h.press(Key::Down);
        }
        let [_, _, v] = crate::color::rgb_to_hsv([h.app().color[0], h.app().color[1], h.app().color[2]]);
        assert!((v - 0.5).abs() < 0.02, "{v}");
    }

    #[test]
    fn a_grey_keeps_its_hue_while_the_picker_is_open() {
        let mut h = paint(0xff8000);
        h.click("Fill");
        // 彩度を 0（灰色）まで下げてから戻しても、橙のまま
        for _ in 0..100 {
            h.press(Key::Left);
        }
        assert_eq!(hex_of(&h), "#FFFFFF");
        for _ in 0..100 {
            h.press(Key::Right);
        }
        assert_eq!(hex_of(&h), "#FF8000", "灰色を通っても色相を忘れない");
    }

    /// 番号を消したり打ったりしている途中の 3 桁（#8E2・#FF9）で色を変えない。3 桁は Enter で決めた時だけ。
    /// 作者が #8E24AA を消して #FF8800 と打った時、途中で #88EE22・#FFFF99・#FFFF88 に変わった（2026-11-22 の記録）
    #[test]
    fn a_half_typed_code_does_not_change_the_color() {
        let mut h = paint(0x8e24aa);
        h.click("Fill").click("Color code").press(Key::End);
        for _ in 0..3 {
            h.press(Key::Backspace);
        }
        assert_eq!(hex_of(&h), "#8E24AA", "消している途中の #8E2 では変えない");
        h.press_with(Key::A, Mods::CTRL).type_text("#FF9");
        assert_eq!(hex_of(&h), "#8E24AA", "打っている途中の #FF9 でも変えない");
        h.press(Key::Enter);
        assert_eq!(hex_of(&h), "#FFFF99", "Enter で決めれば 3 桁も読む");
        h.press_with(Key::A, Mods::CTRL).type_text("#ff8800");
        assert_eq!(hex_of(&h), "#FF8800", "6 桁は打ち終えた時に変わる");
    }

    #[test]
    fn alpha_adds_an_opacity_bar_and_eight_digit_codes() {
        let mut h = Harness::new(Paint { color: crate::rgb(0x000000), alpha: true });
        h.click("Fill");
        let b = h.find("Opacity").expect("不透明度の帯");
        h.click_at(b.x + b.w / 2.0, b.y + b.h / 2.0);
        assert!((h.app().color[3] - 0.5).abs() < 0.01, "{}", h.app().color[3]);
        assert_eq!(a11y_node(&h, "Fill").value(), Some(hex_of(&h).as_str()), "欄は 8 桁");
        assert_eq!(hex_of(&h).len(), 9);
        h.click("Color code").press_with(Key::A, Mods::CTRL).type_text("#11223344");
        assert_eq!(hex_of(&h), "#11223344");
    }

    struct Fields {
        line: String,
        notes: String,
    }

    impl App for Fields {
        fn view(&mut self, ui: &mut Ui) {
            ui.text_input(&mut self.line).label("Line");
            ui.text_area(&mut self.notes).label("Notes");
        }
    }

    fn fields() -> Harness<Fields> {
        Harness::new(Fields { line: "hello world".into(), notes: "first line
second line".into() })
    }

    /// 欄の中で選んでいる文字
    fn selected(h: &Harness<Fields>, name: &str) -> String {
        let id = id_named(h, name);
        let st = h.cur().cx.edits.get(&id).unwrap();
        let (a, b) = st.selection();
        let text = if name == "Line" { &h.app().line } else { &h.app().notes };
        text[a..b].to_string()
    }

    /// 押したまま動かすと、押した所から今の所までを選ぶ（作者が色の番号の欄で試して、選べなかった。2026-11-22）
    #[test]
    fn dragging_in_a_field_selects_from_the_press_to_the_pointer() {
        let mut h = fields();
        let r = h.find("Line").unwrap();
        // 欄の左の端から右の外まで引っ張ると、全部を選ぶ
        h.send(vec![Event::PointerDown { x: r.x + 2.0, y: r.y + r.h / 2.0, shift: false, count: 1 }, Event::PointerMove { x: r.right() + 50.0, y: r.y + r.h / 2.0 }]);
        h.send(vec![Event::PointerUp { x: r.right() + 50.0, y: r.y + r.h / 2.0 }]);
        assert_eq!(selected(&h, "Line"), "hello world");
        h.press(Key::Backspace);
        assert_eq!(h.app().line, "", "選んで消せる");
    }

    #[test]
    fn double_and_triple_clicks_select_a_word_all_or_a_line() {
        let mut h = fields();
        h.double_click("Line");
        assert_eq!(selected(&h, "Line"), "world", "真ん中が文字の後ろなら、最後の単語");
        h.triple_click("Line");
        assert_eq!(selected(&h, "Line"), "hello world", "1 行の欄は全部");
        // 別々の click は 2 回押しにならない
        h.click("Line").click("Line");
        assert_eq!(selected(&h, "Line"), "");
        // 複数行の欄の 3 回押しは、押した行だけ
        let r = h.find("Notes").unwrap();
        let (x, y) = (r.x + 30.0, r.y + 16.0);
        let mut events = Vec::new();
        for count in 1..=3 {
            events.push(Event::PointerDown { x, y, shift: false, count });
            events.push(Event::PointerUp { x, y });
        }
        h.send(events);
        assert_eq!(selected(&h, "Notes"), "first line");
    }

    #[test]
    fn the_color_code_can_be_selected_in_one_go() {
        let mut h = paint(0x1a73e8);
        h.click("Fill").double_click("Color code").type_text("ff8800");
        assert_eq!(hex_of(&h), "#FF8800", "2 回押しで番号の数字を選んで打ち直す");
        let r = h.find("Color code").unwrap();
        let y = r.y + r.h / 2.0;
        h.send(vec![Event::PointerDown { x: r.right() - 2.0, y, shift: false, count: 1 }, Event::PointerMove { x: r.x - 20.0, y }]);
        h.send(vec![Event::PointerUp { x: r.x - 20.0, y }]);
        h.type_text("#00ff00");
        assert_eq!(hex_of(&h), "#00FF00", "浮き出しの中の欄でも、引っ張って全部を選べる");
    }

    /// 2 行に折り返す長さのリンク（30 字）
    const LONG_LINK: &str = "バックアップの仕組みと、消したファイルを元に戻す方法の説明";

    /// 書式付きの文字（ui.rich_text）の見本
    #[derive(Default)]
    struct Notice {
        help: u32,
        off: bool,
        /// 2 行に折り返すリンクを出す
        long: bool,
    }

    impl App for Notice {
        fn view(&mut self, ui: &mut Ui) {
            let (help, long) = (&mut self.help, self.long);
            ui.enabled(!self.off, |ui| {
                if long {
                    // 短い文の後の長いリンク（どの書体でも、1 行目の途中から始まり、次の行へ折り返す）
                    ui.rich_text(|t| {
                        t.text("詳しくは ");
                        if t.link(LONG_LINK).clicked() {
                            *help += 1;
                        }
                    });
                    return;
                }
                ui.rich_text(|t| {
                    t.text("保存すると ");
                    t.text("元に戻せません").bold();
                    t.text("。詳しくは ");
                    if t.link("ヘルプ").clicked() {
                        *help += 1;
                    }
                    t.text(" を見てください。");
                    t.text("赤い字").color(crate::rgb(0xff0000));
                });
            });
            ui.button("Next");
        }
    }

    /// 読み上げでは段落の中の部分が順に並び、リンクはリンク、ほかは文字として伝わる
    #[test]
    fn rich_text_parts_are_read_in_order_and_links_are_links() {
        let h = Harness::new(Notice::default());
        assert_eq!(a11y_node(&h, "ヘルプ").role(), accesskit::Role::Link);
        assert_eq!(a11y_node(&h, "元に戻せません").role(), accesskit::Role::Label);
        let paragraph = a11y_nodes(&h).into_iter().find(|n| n.role() == accesskit::Role::Paragraph).expect("段落がある");
        let order: Vec<u64> = paragraph.children().iter().map(|c| c.0).collect();
        let want: Vec<u64> = ["保存すると ", "元に戻せません", "。詳しくは ", "ヘルプ", " を見てください。", "赤い字"].iter().map(|n| id_named(&h, n).0).collect();
        assert_eq!(order, want);
        // 部分は、並べた所に四角を持つ（リンクは「。詳しくは 」の右）
        let (before, link) = (h.find("。詳しくは ").unwrap(), h.find("ヘルプ").unwrap());
        assert!(link.w > 10.0 && link.h > 10.0, "{link:?}");
        assert!(link.x >= before.right() - 1.0 && (link.y - before.y).abs() < 1.0, "{before:?} {link:?}");
    }

    /// リンクは押すと 1 回だけ clicked になる。Tab で焦点が移り、Enter でも押せる。押せない時は何もしない
    #[test]
    fn rich_text_links_click_with_the_pointer_and_with_enter() {
        let mut h = Harness::new(Notice::default());
        h.click("ヘルプ");
        assert_eq!(h.app().help, 1, "ビューを何回回しても 1 回");
        h.rt.cx.focus = None;
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("ヘルプ"), "Tab はリンクに止まる（ほかの部分には止まらない）");
        h.press(Key::Enter);
        assert_eq!(h.app().help, 2);
        h.press(Key::Tab);
        assert_eq!(focused_name(&h).as_deref(), Some("Next"));
        h.app_mut().off = true;
        h.frame();
        h.click("ヘルプ");
        assert_eq!(h.app().help, 2, "押せない時は数えない");
    }

    /// 2 行に折り返したリンクは、2 行目を押しても clicked になる
    #[test]
    fn a_rich_text_link_that_wraps_can_be_clicked_on_its_second_line() {
        let mut h = Harness::with_size(Notice { long: true, ..Default::default() }, 260.0, 300.0, 1.0);
        let first = h.find(LONG_LINK).unwrap();
        let start = h.find("詳しくは ").unwrap();
        // 2 行目は段落の左端から始まる。1 行目の四角の下の行の、左端の少し右を押す
        let (x, y) = (start.x + 4.0, first.bottom() + first.h / 2.0);
        assert!(first.x > start.x + 20.0, "リンクは 1 行目の途中から始まる: {first:?}");
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
        assert_eq!(h.app().help, 1);
        // 段落の外（ボタンの下）を押しても数えない
        let next = h.find("Next").unwrap();
        let (x, y) = (next.x + 2.0, next.bottom() + 10.0);
        h.send(vec![Event::PointerDown { x, y, shift: false, count: 1 }, Event::PointerUp { x, y }]);
        assert_eq!(h.app().help, 1);
    }

    /// 色を決めた部分はその色で、リンクの下には主の色の下線を描く
    #[test]
    fn rich_text_draws_colors_and_link_underlines() {
        let mut h = Harness::new(Notice::default());
        let img = h.render();
        let rgb = |c: crate::Color| ((c[0] * 255.0).round() as u32) << 16 | ((c[1] * 255.0).round() as u32) << 8 | (c[2] * 255.0).round() as u32;
        let red = h.find("赤い字").unwrap();
        let reds = (red.y as u32..red.bottom() as u32)
            .flat_map(|y| (red.x as u32..red.right() as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = img.rgb(x, y);
                (p >> 16) > 200 && ((p >> 8) & 0xff) < 80 && (p & 0xff) < 80
            })
            .count();
        assert!(reds > 20, "赤い画素 {reds}");
        // リンクの四角の中に、幅いっぱいの主の色の行（下線）がある
        let accent = rgb(h.rt.cx.theme.accent);
        let link = h.find("ヘルプ").unwrap();
        let underline = (link.y as u32..link.bottom() as u32 + 3).any(|y| {
            let n = (link.x as u32 + 1..link.right() as u32 - 1).filter(|&x| img.rgb(x, y) == accent).count();
            n as f32 >= (link.w - 2.0) * 0.9
        });
        assert!(underline, "{link:?}");
    }
}
