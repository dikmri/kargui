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

/// file:// の URI（Linux のポータルの答え）を、ファイルの場所のバイトにする。%XX を元のバイトに戻す。
/// ほかの形（http:// など、ほかの機械の file://host/…）は None
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn file_uri_bytes(uri: &str) -> Option<Vec<u8>> {
    let rest = uri.strip_prefix("file://")?;
    // 「file://機械の名前/場所」。名前が無いか localhost の時だけ、この機械のファイル
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(i) if &rest[..i] == "localhost" => &rest[i..],
        _ => return None,
    };
    let b = path.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push(h << 4 | l);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

/// text/uri-list（ほかのアプリから引っ張ってきたファイルの一覧）を、ファイルの場所のバイトの並びにする。
/// 行は CRLF で区切られ、「#」で始まる行は注釈。file:// でない行とほかの機械の行は飛ばす
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn uri_list(text: &str) -> Vec<Vec<u8>> {
    text.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(file_uri_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_lists_become_paths_skipping_comments_and_other_machines() {
        let list = "# 注釈\r\nfile:///home/a/b%20c.txt\r\nfile:///tmp/%E6%9D%B1%E4%BA%AC\r\nhttps://example.com/x\r\nfile://other-pc/y\r\n\r\n";
        assert_eq!(uri_list(list), vec![b"/home/a/b c.txt".to_vec(), "/tmp/東京".as_bytes().to_vec()]);
        // 最後の改行が無くても、LF だけでも読む
        assert_eq!(uri_list("file:///a\nfile:///b"), vec![b"/a".to_vec(), b"/b".to_vec()]);
        assert!(uri_list("").is_empty());
    }

    #[test]
    fn file_uris_become_paths_with_percent_escapes_decoded() {
        assert_eq!(file_uri_bytes("file:///home/a/b.txt").as_deref(), Some(&b"/home/a/b.txt"[..]));
        assert_eq!(file_uri_bytes("file:///home/a/b%20c.txt").as_deref(), Some(&b"/home/a/b c.txt"[..]));
        // 日本語の名前は UTF-8 のバイトを %XX で並べてある
        assert_eq!(file_uri_bytes("file:///tmp/%E6%9D%B1%E4%BA%AC.md").as_deref(), Some("/tmp/東京.md".as_bytes()));
        assert_eq!(file_uri_bytes("file://localhost/tmp/x").as_deref(), Some(&b"/tmp/x"[..]));
        // 小文字の %xx も読む。% の後ろが 16 進でなければ、そのままにする
        assert_eq!(file_uri_bytes("file:///a%2fb%zz").as_deref(), Some(&b"/a/b%zz"[..]));
        assert_eq!(file_uri_bytes("https://example.com/a"), None);
        assert_eq!(file_uri_bytes("file://other-pc/a"), None);
    }
}
