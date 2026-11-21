//! The application: what you implement, and how to open its window.

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
#[derive(Clone, Debug)]
pub struct Options {
    /// Window title (also the name screen readers use for the window).
    pub title: String,
    /// Size of the inside of the window in logical pixels.
    pub size: (f32, f32),
    /// Position of the window's top-left corner in logical pixels, or `None` to let the system choose.
    pub position: Option<(f32, f32)>,
    /// Whether the window takes the keyboard focus when it opens.
    pub activate: bool,
    /// How the window is drawn.
    pub renderer: Renderer,
    /// Keep drawing animations at the full rate when the window is in the background.
    /// By default background windows animate at about 30 frames per second to save power.
    pub full_rate: bool,
    /// Start in dark (`Some(true)`) or light (`Some(false)`) mode; `None` follows the system.
    pub dark: Option<bool>,
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
            activate: true,
            renderer: Renderer::default(),
            full_rate: false,
            dark: None,
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
    crate::platform::run(options, Box::new(app))
}
