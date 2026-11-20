//! タスク帳の共通部分。3 つの GUI（Tauri・egui・GPUI）で同じものを使い、
//! 違いが画面の層だけに出るようにする。
//! Tauri 版は画面の側（ui/core.js）に同じ規則の写しを持つ。

pub mod args;
pub mod bench;
pub mod log;

/// 1 行の高さ（論理ピクセル）。3 つの版で同じ値を使う。
pub const ROW_HEIGHT: f32 = 32.0;
/// 窓の大きさ（論理ピクセル）。
pub const WINDOW_SIZE: (f32, f32) = (900.0, 640.0);

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: u32,
    pub text: String,
    pub done: bool,
    /// 絞り込み用に小文字へそろえた本文
    key: String,
}

impl Task {
    fn new(id: u32, text: String, done: bool) -> Self {
        let key = text.to_lowercase();
        Self { id, text, done, key }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    All,
    Active,
    Done,
}

impl Status {
    pub const LIST: [Status; 3] = [Status::All, Status::Active, Status::Done];

    pub fn label(self) -> &'static str {
        match self {
            Status::All => "すべて",
            Status::Active => "未完了",
            Status::Done => "完了",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Status::All => "all",
            Status::Active => "active",
            Status::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Counts {
    pub total: usize,
    pub active: usize,
    pub done: usize,
}

const WHAT: [&str; 8] = [
    "企画書", "見積もり", "議事録", "請求書", "発表資料", "週報", "契約書", "設計図",
];
const DO: [&str; 6] = [
    "を作る", "を見直す", "を送る", "を印刷する", "を共有する", "をまとめる",
];

/// i 番目（0 始まり）の見本のタスクの本文。Tauri 版の ui/core.js と同じ規則。
pub fn sample_text(i: usize) -> String {
    format!("{:05} {}{}", i + 1, WHAT[i % WHAT.len()], DO[(i / WHAT.len()) % DO.len()])
}

/// i 番目の見本のタスクを完了にしておくか。3 件に 1 件を完了にする。
pub fn sample_done(i: usize) -> bool {
    i % 3 == 2
}

/// 3 桁ごとに , を入れる
pub fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Default)]
pub struct Store {
    tasks: Vec<Task>,
    next_id: u32,
}

impl Store {
    pub fn new() -> Self {
        Self { tasks: Vec::new(), next_id: 1 }
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    pub fn get(&self, index: usize) -> Option<&Task> {
        self.tasks.get(index)
    }

    /// 見本のタスクを n 件、末尾に足す。番号は id に合わせる。
    pub fn generate(&mut self, n: usize) {
        self.tasks.reserve(n);
        for _ in 0..n {
            let id = self.next_id;
            self.next_id += 1;
            let i = (id - 1) as usize;
            self.tasks.push(Task::new(id, sample_text(i), sample_done(i)));
        }
    }

    /// 前後の空白を除いた本文でタスクを先頭に足す。空なら足さずに None を返す。
    pub fn add(&mut self, text: &str) -> Option<u32> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.insert(0, Task::new(id, text.to_string(), false));
        Some(id)
    }

    /// 完了と未完了を入れ替え、入れ替えたあとの状態を返す。
    pub fn toggle(&mut self, id: u32) -> Option<bool> {
        let task = self.tasks.iter_mut().find(|t| t.id == id)?;
        task.done = !task.done;
        Some(task.done)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        match self.tasks.iter().position(|t| t.id == id) {
            Some(pos) => {
                self.tasks.remove(pos);
                true
            }
            None => false,
        }
    }

    /// 完了したタスクを消し、消した件数を返す。
    pub fn clear_done(&mut self) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|t| !t.done);
        before - self.tasks.len()
    }

    /// すべて消し、消した件数を返す。番号は 1 から振り直す。
    pub fn clear(&mut self) -> usize {
        let n = self.tasks.len();
        self.tasks.clear();
        self.next_id = 1;
        n
    }

    pub fn counts(&self) -> Counts {
        let done = self.tasks.iter().filter(|t| t.done).count();
        Counts { total: self.tasks.len(), active: self.tasks.len() - done, done }
    }

    /// 絞り込みに合うタスクの位置（tasks() の添字）を並び順のまま返す。
    /// 語は前後の空白を除き、大文字と小文字を区別しない。
    pub fn filter(&self, query: &str, status: Status) -> Vec<usize> {
        let q = query.trim().to_lowercase();
        self.tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| match status {
                Status::All => true,
                Status::Active => !t.done,
                Status::Done => t.done,
            })
            .filter(|(_, t)| q.is_empty() || t.key.contains(&q))
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_inserts_commas() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(10_000), "10,000");
        assert_eq!(group(1_234_567), "1,234,567");
    }

    #[test]
    fn sample_text_matches_the_js_copy() {
        // ui/core.js のテストにも同じ値を書いてある
        assert_eq!(sample_text(0), "00001 企画書を作る");
        assert_eq!(sample_text(9), "00010 見積もりを見直す");
        assert_eq!(sample_text(9999), "10000 設計図を見直す");
        assert!(!sample_done(0) && !sample_done(1) && sample_done(2));
    }

    #[test]
    fn generate_numbers_follow_ids() {
        let mut s = Store::new();
        s.generate(10_000);
        assert_eq!(s.counts(), Counts { total: 10_000, active: 6_667, done: 3_333 });
        assert_eq!(s.tasks()[9_999].text, "10000 設計図を見直す");
        s.generate(1);
        assert_eq!(s.tasks()[10_000].text, "10001 企画書を送る");
    }

    #[test]
    fn add_puts_trimmed_text_on_top_and_rejects_blank() {
        let mut s = Store::new();
        s.generate(2);
        assert_eq!(s.add("   "), None);
        let id = s.add("  牛乳を買う ").unwrap();
        assert_eq!(id, 3);
        assert_eq!(s.tasks()[0].text, "牛乳を買う");
        assert!(!s.tasks()[0].done);
        assert_eq!(s.counts().total, 3);
    }

    #[test]
    fn toggle_and_remove_by_id() {
        let mut s = Store::new();
        s.generate(3);
        assert_eq!(s.toggle(1), Some(true));
        assert_eq!(s.toggle(1), Some(false));
        assert_eq!(s.toggle(99), None);
        assert!(s.remove(2));
        assert!(!s.remove(2));
        assert_eq!(s.tasks().iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn filter_by_word_and_status() {
        let mut s = Store::new();
        s.generate(10_000);
        // 8 種類のうち「発表資料」だけが「資料」を含む
        assert_eq!(s.filter("資料", Status::All).len(), 1_250);
        assert_eq!(s.filter("", Status::Done).len(), 3_333);
        assert_eq!(s.filter("  資料 ", Status::Done).len(), s
            .tasks()
            .iter()
            .filter(|t| t.done && t.text.contains("資料"))
            .count());
        assert!(s.filter("存在しない語", Status::All).is_empty());
    }

    #[test]
    fn filter_ignores_ascii_case_and_keeps_order() {
        let mut s = Store::new();
        s.add("Buy Milk");
        s.add("call bob");
        assert_eq!(s.filter("MILK", Status::All), vec![1]);
        assert_eq!(s.filter("", Status::Active), vec![0, 1]);
    }

    #[test]
    fn clear_done_and_clear() {
        let mut s = Store::new();
        s.generate(9);
        assert_eq!(s.clear_done(), 3);
        assert_eq!(s.counts(), Counts { total: 6, active: 6, done: 0 });
        assert_eq!(s.clear(), 6);
        s.generate(1);
        assert_eq!(s.tasks()[0].text, "00001 企画書を作る");
    }
}
