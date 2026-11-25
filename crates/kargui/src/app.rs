//! The application: what you implement, and how to open its window.

use std::borrow::Cow;
use std::time::Duration;

use crate::ui::Ui;

/// Your application. Keep your state in the struct and describe the screen in [`App::view`].
///
/// ```no_run
/// use kargui::{App, Options, Ui};
///
/// struct Hello;
///
/// impl App for Hello {
///     fn view(&mut self, ui: &mut Ui) {
///         ui.heading("Hello, kargui");
///     }
/// }
///
/// fn main() {
///     kargui::run(Options::new("Hello"), Hello).unwrap();
/// }
/// ```
pub trait App: 'static {
    /// Describes the whole screen. kargui calls this when something changed (input, window size,
    /// [`Ui::request_frame`]). It may be called twice in a frame: once to deliver input to the
    /// widgets, once to draw the result.
    ///
    /// ```
    /// use kargui::{App, Ui};
    ///
    /// struct Greeting {
    ///     name: String,
    /// }
    ///
    /// impl App for Greeting {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.label(format!("Hello, {}", self.name));
    ///     }
    /// }
    ///
    /// let h = kargui::test::Harness::new(Greeting { name: "Ada".into() });
    /// assert!(h.find("Hello, Ada").is_some());
    /// ```
    fn view(&mut self, ui: &mut Ui);

    /// Called after each frame is on the screen. The first call happens once the window's first
    /// image has been composed by the operating system, which is a good point to measure startup.
    ///
    /// ```
    /// use kargui::{App, Frame, Ui};
    ///
    /// struct Timed {
    ///     first: Option<std::time::Duration>,
    /// }
    ///
    /// impl App for Timed {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         ui.label("Hello");
    ///     }
    ///
    ///     fn presented(&mut self, frame: &Frame) {
    ///         if frame.number == 0 {
    ///             self.first = Some(frame.since_start);
    ///         }
    ///     }
    /// }
    /// ```
    fn presented(&mut self, _frame: &Frame) {}

    /// Describes the content of another window opened with [`Ui::window`]; `key` tells which.
    /// All windows share the app, so a change made in one shows in the others.
    ///
    /// ```
    /// use kargui::{App, Options, Ui};
    ///
    /// #[derive(Default)]
    /// struct Editor {
    ///     about: bool,
    /// }
    ///
    /// impl App for Editor {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         if ui.button("About").clicked() {
    ///             self.about = true;
    ///         }
    ///         ui.window("about", &Options::new("About"), &mut self.about);
    ///     }
    ///
    ///     fn window(&mut self, key: &str, ui: &mut Ui) {
    ///         if key == "about" {
    ///             ui.label("Editor 1.0");
    ///         }
    ///     }
    /// }
    ///
    /// let mut h = kargui::test::Harness::new(Editor::default());
    /// h.click("About").window("about");
    /// assert!(h.find("Editor 1.0").is_some());
    /// ```
    fn window(&mut self, key: &str, ui: &mut Ui) {
        let _ = (key, ui);
    }

    /// Called when the user asks to close the main window (its close button, Alt+F4). Return
    /// `false` to keep it open, for example to ask about unsaved changes first; close it later
    /// with [`Ui::quit`]. Returns `true` (close) by default.
    ///
    /// ```
    /// use kargui::{App, Ui};
    ///
    /// #[derive(Default)]
    /// struct Notes {
    ///     unsaved: bool,
    ///     asking: bool,
    /// }
    ///
    /// impl App for Notes {
    ///     fn view(&mut self, ui: &mut Ui) {
    ///         let mut discard = false;
    ///         ui.dialog(&mut self.asking, "Discard your changes?", |ui| {
    ///             discard = ui.button("Discard").clicked();
    ///         });
    ///         if discard {
    ///             ui.quit();
    ///         }
    ///     }
    ///
    ///     fn close_requested(&mut self) -> bool {
    ///         self.asking = self.unsaved;
    ///         !self.unsaved
    ///     }
    /// }
    ///
    /// let mut h = kargui::test::Harness::new(Notes { unsaved: true, asking: false });
    /// h.request_close();
    /// assert!(!h.is_closed());
    /// h.click("Discard");
    /// assert!(h.is_closed());
    /// ```
    fn close_requested(&mut self) -> bool {
        true
    }
}

/// Information about a frame that was shown. See [`App::presented`].
///
/// ```
/// use kargui::{App, Frame, Ui};
///
/// struct Counter {
///     frames: u64,
/// }
///
/// impl App for Counter {
///     fn view(&mut self, ui: &mut Ui) {
///         ui.label("Hello");
///     }
///
///     fn presented(&mut self, frame: &Frame) {
///         self.frames = frame.number + 1;
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// 0 for the first frame.
    pub number: u64,
    /// Time since [`run`] was called.
    pub since_start: Duration,
}

/// How the window is drawn.
///
/// ```
/// use kargui::{Options, Renderer};
///
/// let mut options = Options::new("Fast");
/// assert_eq!(options.renderer, Renderer::Cpu);
/// options.renderer = Renderer::Gpu;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Renderer {
    /// With the CPU, redrawing only what changed. Starts fastest and uses the least memory;
    /// the default.
    #[default]
    Cpu,
    /// With the GPU (Direct3D 11).
    Gpu,
}

/// Window settings for [`run`].
///
/// ```
/// use kargui::Options;
///
/// let mut options = Options::new("Notes");
/// options.size = (480.0, 640.0);
/// options.dark = Some(true);
/// assert_eq!(options.title, "Notes");
/// ```
#[derive(Clone)]
pub struct Options {
    /// Window title (also the name screen readers use for the window).
    pub title: String,
    /// Size of the inside of the window in logical pixels.
    pub size: (f32, f32),
    /// Position of the window's top-left corner in logical pixels, or `None` to let the system choose.
    pub position: Option<(f32, f32)>,
    /// Whether kargui draws the title bar on Windows (on GNOME it always does), so the app can put
    /// widgets in it with [`Ui::title_bar`](crate::Ui::title_bar). kargui keeps the minimize,
    /// maximize and close buttons at the bar's right end, and the empty part of the bar moves the
    /// window, maximizes it on a double click and opens the window menu on a right click, as the
    /// system's bar does. `false` by default.
    pub custom_title_bar: bool,
    /// High contrast: `None` (the default) follows the system's setting (Windows: Settings >
    /// Accessibility > Contrast themes; Linux: the desktop portal's contrast setting),
    /// `Some(true)` always uses a high contrast palette, `Some(false)` never does. With the
    /// system's high contrast on Windows, kargui uses the system's colors; otherwise
    /// [`Theme::high_contrast_dark`](crate::Theme::high_contrast_dark) or
    /// [`Theme::high_contrast_light`](crate::Theme::high_contrast_light) by dark mode.
    pub high_contrast: Option<bool>,
    /// Whether the window takes the keyboard focus when it opens.
    pub activate: bool,
    /// How the window is drawn.
    pub renderer: Renderer,
    /// Keep drawing animations at the full rate when the window is in the background.
    /// By default background windows animate at about 30 frames per second to save power.
    pub full_rate: bool,
    /// Start in dark (`Some(true)`) or light (`Some(false)`) mode; `None` follows the system.
    pub dark: Option<bool>,
    /// The window's icon (title bar and taskbar). With `None`, kargui shows its own icon (a
    /// light "k" on a dark rounded square), or the system's plain icon when
    /// [`default_icon`](Self::default_icon) is `false`. Windows opened with [`Ui::window`] use
    /// the main window's icon unless they have their own.
    pub icon: Option<crate::Image>,
    /// Whether a window without an [`icon`](Self::icon) shows kargui's icon (`true`, the
    /// default) or the system's plain icon (`false`). A [`tray`](crate::Ui::tray) icon without
    /// an image uses kargui's icon too, or a plain square when this is `false`. On Wayland the
    /// compositor takes the window's icon from the app's desktop file, so this changes only the
    /// tray there.
    pub default_icon: bool,
    /// The smallest size of the inside of the window, in logical pixels, that the user can make
    /// it, or `None` for no limit.
    pub min_size: Option<(f32, f32)>,
    /// The font of the text in the window and in windows opened with [`Ui::window`], unless a
    /// widget has its own (see [`Font`](crate::Font)). [`Ui::set_font`] changes it while the
    /// app runs.
    pub font: crate::Font,
    /// Font files the app brings along (TrueType or OpenType: `.ttf`, `.otf`, `.ttc`), for
    /// example with `include_bytes!`. Choose them by family name with
    /// [`Font::family`](crate::Font::family). Empty by default, and then no font data is read
    /// when the window opens.
    ///
    /// ```
    /// use kargui::{Font, Options};
    ///
    /// let mut options = Options::new("Shop");
    /// // In an app: options.fonts.push(include_bytes!("../fonts/ShopSans.ttf").as_slice().into());
    /// if let Ok(data) = std::fs::read("fonts/ShopSans.ttf") {
    ///     options.fonts.push(data.into());
    /// }
    /// options.font = Font::family("Shop Sans");
    /// ```
    pub fonts: Vec<Cow<'static, [u8]>>,
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 書体のファイルは中身を出さず、数と大きさだけ
        let bytes: usize = self.fonts.iter().map(|d| d.len()).sum();
        f.debug_struct("Options")
            .field("title", &self.title)
            .field("size", &self.size)
            .field("position", &self.position)
            .field("activate", &self.activate)
            .field("renderer", &self.renderer)
            .field("full_rate", &self.full_rate)
            .field("dark", &self.dark)
            .field("high_contrast", &self.high_contrast)
            .field("icon", &self.icon)
            .field("default_icon", &self.default_icon)
            .field("min_size", &self.min_size)
            .field("font", &self.font)
            .field("fonts", &format_args!("{} files, {bytes} bytes", self.fonts.len()))
            .finish()
    }
}

impl Options {
    /// Default options with the given title.
    ///
    /// ```
    /// let options = kargui::Options::new("Notes");
    /// assert_eq!(options.size, (900.0, 640.0));
    /// assert!(options.activate);
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            size: (900.0, 640.0),
            position: None,
            custom_title_bar: false,
            activate: true,
            renderer: Renderer::default(),
            full_rate: false,
            dark: None,
            high_contrast: None,
            icon: None,
            default_icon: true,
            min_size: None,
            font: crate::Font::Ui,
            fonts: Vec::new(),
        }
    }
}

/// Opens a window for `app` and runs until the window is closed.
///
/// ```no_run
/// use kargui::{App, Options, Ui};
///
/// struct Hello;
///
/// impl App for Hello {
///     fn view(&mut self, ui: &mut Ui) {
///         ui.heading("Hello, kargui");
///     }
/// }
///
/// if let Err(e) = kargui::run(Options::new("Hello"), Hello) {
///     eprintln!("could not open the window: {e}");
/// }
/// ```
pub fn run(options: Options, app: impl App) -> Result<(), crate::Error> {
    crate::theme::set_option(options.high_contrast);
    crate::logo::set_enabled(options.default_icon);
    #[cfg(windows)]
    {
        crate::platform::run(options, Box::new(app))
    }
    // Linux は Wayland か X11 の窓（KARGUI_BACKEND で選べる。決め方は linux::choose_backend）
    #[cfg(target_os = "linux")]
    {
        let backend = crate::linux::choose_backend(|k| std::env::var(k).ok(), |p| p.exists());
        crate::log::event("backend", "", &format!("{backend:?}"));
        match backend {
            crate::linux::Backend::Wayland => crate::wayland::run(options, Box::new(app)),
            crate::linux::Backend::X11 => crate::x11::run(options, Box::new(app)),
        }
    }
    // macOS の窓はまだ無い（部品・並べ方・描き方・試験台は動く）
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (options, app);
        Err(crate::Error("kargui cannot open windows on this operating system yet: Windows and Linux (Wayland and X11) are supported (macOS is planned)".into()))
    }
}
