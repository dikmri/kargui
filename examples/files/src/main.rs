// ファイルの閲覧（kargui の例。ふつうのアプリで確かめる 3 本目）。仕切りで左右に分け、左にフォルダーの木、右に
// ファイルの表（並べ替え・選択・行の右クリックのメニュー）、上にツールバー、プロパティは別の窓に出す。
#![windows_subsystem = "windows"]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use kargui::ui::{TableColumn, TreeBuilder};
use kargui::{App, Date, Frame, Icon, Options, Ui};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/files.log");

#[derive(Clone, PartialEq, Debug)]
struct FileEntry {
    name: String,
    path: PathBuf,
    size: u64,
    modified: Option<Date>,
}

struct Files {
    root: PathBuf,
    /// 木で選んでいるフォルダー（キーはパス）
    folder: Option<PathBuf>,
    /// フォルダーの中のフォルダー（読んだ物を覚える。毎フレーム読まない）
    dirs: HashMap<PathBuf, Vec<PathBuf>>,
    files: Vec<FileEntry>,
    loaded: Option<PathBuf>,
    split: f32,
    sort: Option<(usize, bool)>,
    selected: Option<usize>,
    props: Option<FileEntry>,
    props_open: bool,
    started: Instant,
    ready: bool,
}

/// 大きさを「12.3 KB」の形にする
fn size_text(b: u64) -> String {
    match b {
        0..=1023 => format!("{b} バイト"),
        1024..=1_048_575 => format!("{:.1} KB", b as f64 / 1024.0),
        _ => format!("{:.1} MB", b as f64 / 1_048_576.0),
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl Files {
    fn new(root: PathBuf, started: Instant) -> Self {
        Self {
            folder: Some(root.clone()),
            root,
            dirs: HashMap::new(),
            files: Vec::new(),
            loaded: None,
            split: 220.0,
            sort: Some((0, true)),
            selected: None,
            props: None,
            props_open: false,
            started,
            ready: false,
        }
    }

    /// フォルダーの中のフォルダー（名前の順。読めなければ空）
    fn subdirs(&mut self, p: &Path) -> Vec<PathBuf> {
        self.dirs
            .entry(p.to_path_buf())
            .or_insert_with(|| {
                let mut v: Vec<PathBuf> = std::fs::read_dir(p).map(|it| it.flatten().map(|e| e.path()).filter(|q| q.is_dir()).collect()).unwrap_or_default();
                v.sort();
                v
            })
            .clone()
    }

    /// 選んだフォルダーのファイルを読む（フォルダーが変わった時だけ）
    fn load_files(&mut self) {
        if self.loaded == self.folder {
            return;
        }
        self.loaded = self.folder.clone();
        self.selected = None;
        self.files.clear();
        let Some(dir) = &self.folder else { return };
        if let Ok(it) = std::fs::read_dir(dir) {
            for e in it.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_file() {
                    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| Date::from_unix_days((d.as_secs() / 86_400) as i64));
                    self.files.push(FileEntry { name: name_of(&e.path()), path: e.path(), size: meta.len(), modified });
                }
            }
        }
        log::event("load", &dir.display().to_string(), &format!("files={}", self.files.len()));
    }

    fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.files.len()).collect();
        if let Some((col, asc)) = self.sort {
            order.sort_by(|&a, &b| {
                let (x, y) = (&self.files[a], &self.files[b]);
                let o = match col {
                    0 => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
                    1 => x.size.cmp(&y.size),
                    _ => x.modified.cmp(&y.modified),
                };
                if asc { o } else { o.reverse() }
            });
        }
        order
    }

    fn folder_tree(&mut self, ui: &mut Ui) {
        let root = self.root.clone();
        let mut folder = self.folder.clone();
        ui.tree(&mut folder, |t| branch(t, self, &root)).label("フォルダー");
        if folder != self.folder {
            self.folder = folder;
        }
    }

    fn file_table(&mut self, ui: &mut Ui) {
        self.load_files();
        let order = self.order();
        let mut props = None;
        let up = self.folder.as_ref().and_then(|f| f.parent()).filter(|p| p.starts_with(&self.root)).map(Path::to_path_buf);
        ui.toolbar(|tb| {
            if tb.button(Icon::ArrowUp, "上のフォルダーへ").enabled(up.is_some()).clicked() {
                self.folder = up.clone();
            }
            if tb.button(Icon::Info, "プロパティ").enabled(self.selected.is_some()).clicked() {
                props = self.selected.and_then(|r| order.get(r)).map(|&k| self.files[k].clone());
            }
        })
        .label("ファイルの操作");
        let where_text = self.folder.as_ref().map_or_else(String::new, |f| f.display().to_string());
        ui.label(where_text).weak().small();
        let columns = [TableColumn::new("名前").fill(), TableColumn::new("大きさ").width(100.0).right(), TableColumn::new("更新日").width(120.0)];
        let files = &self.files;
        ui.table(&columns, order.len(), 28.0).sort(&mut self.sort).selection(&mut self.selected).label("ファイル").grow().show_keyed(
            |r| files[order[r]].path.clone(),
            |row, r| {
                let f = &files[order[r]];
                row.text(&f.name);
                row.text(size_text(f.size));
                row.text(f.modified.map_or_else(String::new, |d| d.to_string()));
                row.context_menu(|m| {
                    if m.item("プロパティ").clicked() {
                        props = Some(f.clone());
                    }
                });
            },
        );
        if let Some(p) = props {
            log::event("props", &p.name, "open");
            self.props = Some(p);
            self.props_open = true;
        }
        if self.files.is_empty() {
            ui.label("このフォルダーにはファイルがありません").weak();
        }
    }
}

/// 木の 1 つの枝（フォルダー）と、その中のフォルダー
fn branch(t: &mut TreeBuilder<PathBuf>, app: &mut Files, dir: &Path) {
    t.branch(dir.to_path_buf(), name_of(dir), |t| {
        for sub in app.subdirs(dir) {
            branch(t, app, &sub);
        }
    });
}

impl App for Files {
    fn view(&mut self, ui: &mut Ui) {
        let mut split = self.split;
        ui.split_left_right(&mut split, |ui, pane| {
            if pane == 0 {
                self.folder_tree(ui);
            } else {
                self.file_table(ui);
            }
        })
        .label("フォルダーの一覧の幅")
        .grow();
        self.split = split;
        let title = self.props.as_ref().map_or_else(|| "プロパティ".to_string(), |p| format!("{} のプロパティ", p.name));
        let options = Options { size: (420.0, 220.0), ..Options::new(title) };
        ui.window("props", &options, &mut self.props_open);
    }

    fn window(&mut self, key: &str, ui: &mut Ui) {
        if key != "props" {
            return;
        }
        let Some(p) = self.props.clone() else { return };
        ui.grid(&[kargui::Size::Auto, kargui::Size::Fill(1.0)], |ui| {
            ui.label("名前").weak();
            ui.label(&p.name);
            ui.label("大きさ").weak();
            ui.label(format!("{}（{} バイト）", size_text(p.size), p.size));
            ui.label("更新日").weak();
            ui.label(p.modified.map_or_else(|| "不明".to_string(), |d| d.to_string()));
            ui.label("場所").weak();
            ui.label(p.path.parent().map_or_else(String::new, |d| d.display().to_string())).wrap();
        });
        ui.row(|ui| {
            ui.fill();
            if ui.button("閉じる").clicked() {
                self.props_open = false;
            }
        });
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", "app=files", &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

fn main() {
    let started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui-files", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    log::event("startup", "app=files", &format!("log={path:?}"));
    // 見本: kargui のリポジトリを見る（個人のフォルダーの中身が写らないように）
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    // 正規化すると Windows では \\?\ の印が付くので外す（表示に出るため）
    let root = root.canonicalize().map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_string())).unwrap_or(root);
    let mut options = Options::new("ファイルの閲覧（kargui）");
    options.size = (820.0, 520.0);
    options.min_size = Some((520.0, 320.0));
    options.position = a.pos;
    options.activate = !a.no_focus;
    if let Err(e) = kargui::run(options, Files::new(root, started)) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::Key;
    use kargui::test::Harness;

    /// 試験のフォルダー: top/{a.txt, big.bin, docs/{b.md, old/}}（試験は同時に走るので、試験ごとに別の場所）
    fn sample(tag: &str) -> PathBuf {
        let top = std::env::temp_dir().join("kargui-files-test").join(tag).join("top");
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join("docs").join("old")).unwrap();
        std::fs::write(top.join("a.txt"), "abc").unwrap();
        std::fs::write(top.join("big.bin"), vec![0u8; 5000]).unwrap();
        std::fs::write(top.join("docs").join("b.md"), "# b").unwrap();
        top
    }

    fn app(tag: &str) -> Harness<Files> {
        Harness::with_size(Files::new(sample(tag), Instant::now()), 820.0, 520.0, 1.0)
    }

    #[test]
    fn the_tree_picks_the_folder_the_table_shows() {
        let mut h = app("tree");
        assert!(h.find("a.txt").is_some() && h.find("big.bin").is_some(), "最初は一番上のフォルダー");
        assert!(h.find("4.9 KB").is_some());
        // 木: 一番上を開いて docs を選ぶ
        h.click("top").press(Key::Right).press(Key::Down);
        assert!(h.find("b.md").is_some(), "docs の中");
        assert!(h.find("a.txt").is_none());
        assert_eq!(h.enabled("上のフォルダーへ"), Some(true));
        h.click("上のフォルダーへ");
        assert!(h.find("a.txt").is_some());
    }

    #[test]
    fn sorting_by_size_and_showing_properties_in_another_window() {
        let mut h = app("sort");
        h.click("大きさ");
        h.click("大きさ");
        assert_eq!(h.app().sort, Some((1, false)));
        assert!(h.find("big.bin").unwrap().y < h.find("a.txt").unwrap().y, "大きい順");
        h.right_click("a.txt").click("プロパティ");
        assert_eq!(h.windows(), ["props"]);
        h.window("props");
        assert!(h.find("3 バイト（3 バイト）").is_some());
        h.click("閉じる");
        assert!(h.windows().is_empty());
    }
}


