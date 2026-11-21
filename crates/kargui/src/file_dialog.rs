//! システムのファイルのダイアログ（開く・保存する）の頼み。出すのは窓の処理（Windows は platform.rs）。

use std::path::PathBuf;

/// The system's dialog for opening or saving files, described for [`Ui::file_dialog`](crate::Ui::file_dialog).
///
/// ```
/// use kargui::FileDialog;
///
/// let open = FileDialog::open().title("Open a picture").filter("Pictures", &["png", "jpg"]).multiple();
/// let save = FileDialog::save().file_name("report.txt").filter("Text", &["txt"]);
/// assert!(!open.is_save() && open.allows_multiple());
/// assert!(save.is_save());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct FileDialog {
    pub(crate) save: bool,
    pub(crate) multiple: bool,
    pub(crate) title: Option<String>,
    /// 種類の名前と、拡張子（点を付けない）
    pub(crate) filters: Vec<(String, Vec<String>)>,
    pub(crate) file_name: Option<String>,
    pub(crate) folder: Option<PathBuf>,
}

impl FileDialog {
    /// A dialog for choosing a file to open.
    ///
    /// ```
    /// let dialog = kargui::FileDialog::open();
    /// assert!(!dialog.is_save());
    /// ```
    pub fn open() -> Self {
        Self { save: false, multiple: false, title: None, filters: Vec::new(), file_name: None, folder: None }
    }

    /// A dialog for choosing where to save a file. It asks before replacing an existing file.
    ///
    /// ```
    /// let dialog = kargui::FileDialog::save();
    /// assert!(dialog.is_save());
    /// ```
    pub fn save() -> Self {
        Self { save: true, ..Self::open() }
    }

    /// Lets the user choose more than one file (open dialogs only).
    ///
    /// ```
    /// let dialog = kargui::FileDialog::open().multiple();
    /// assert!(dialog.allows_multiple());
    /// ```
    pub fn multiple(mut self) -> Self {
        self.multiple = true;
        self
    }

    /// The dialog's title.
    ///
    /// ```
    /// let dialog = kargui::FileDialog::open().title("Import contacts");
    /// # let _ = dialog;
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a kind of file the user can pick, by name and extensions without the dot. The first
    /// one is selected at first; when saving, its first extension is added to a name without one.
    ///
    /// ```
    /// let dialog = kargui::FileDialog::save().filter("Markdown", &["md"]).filter("Text", &["txt"]);
    /// # let _ = dialog;
    /// ```
    pub fn filter(mut self, name: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push((name.into(), extensions.iter().map(|e| e.trim_start_matches('.').to_string()).collect()));
        self
    }

    /// The file name filled in at first (for saving).
    ///
    /// ```
    /// let dialog = kargui::FileDialog::save().file_name("untitled.txt");
    /// # let _ = dialog;
    /// ```
    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }

    /// The folder shown at first.
    ///
    /// ```
    /// let dialog = kargui::FileDialog::open().folder("C:/Users/Public/Documents");
    /// # let _ = dialog;
    /// ```
    pub fn folder(mut self, folder: impl Into<PathBuf>) -> Self {
        self.folder = Some(folder.into());
        self
    }

    /// Whether this is a save dialog.
    ///
    /// ```
    /// assert!(kargui::FileDialog::save().is_save());
    /// ```
    pub fn is_save(&self) -> bool {
        self.save
    }

    /// Whether more than one file can be chosen.
    ///
    /// ```
    /// assert!(!kargui::FileDialog::open().allows_multiple());
    /// ```
    pub fn allows_multiple(&self) -> bool {
        self.multiple && !self.save
    }
}
