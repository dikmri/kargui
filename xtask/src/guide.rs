//! ガイド（docs/guide/）を古くしないための検査。
//!
//! - 扱う API: 各ページの先頭の `<!-- covers: Ui::slider Label::wrap ... sig=XXXX -->` に、そのページが説明する
//!   公開の API を書く。名前がソースに無ければ落とす。書いた API の宣言の文（シグネチャ）から作った値を sig= に
//!   入れておき、宣言が変わったのにページを見直していなければ（sig が合わなければ）落とす
//! - 訳の古さ: 英語のページの先頭の `<!-- source: ja/01-first-app.md fnv=XXXX -->` に、訳した時の日本語のページの値を
//!   入れておく。日本語のページが変わったのに訳を直していなければ落とす
//!
//! 見直した後に `guide-check --update` で値を入れ直す。値は FNV-1a（64 ビット）。改行は LF にそろえて数える。

use std::collections::BTreeMap;

/// FNV-1a（64 ビット）の 16 けたの 16 進
pub fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.replace("\r\n", "\n").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// `impl<'a> Ui<'a> {` などの行から型の名前（Ui）を取り出す
fn impl_type(line: &str) -> Option<String> {
    let rest = line.strip_prefix("impl")?;
    // impl の後の <...> を飛ばす
    let rest = if rest.starts_with('<') {
        let mut depth = 0;
        let mut end = 0;
        for (i, c) in rest.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        &rest[end..]
    } else {
        rest
    };
    let rest = rest.split(" for ").last()?.trim();
    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').collect();
    let name = name.rsplit("::").next()?.to_string();
    (!name.is_empty()).then_some(name)
}

fn ident_after<'a>(t: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = t.strip_prefix(prefix)?;
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// ソース 1 ファイルの公開の API の名前 → 宣言の文。型（struct・enum・trait）、自由な関数、impl と trait の中の関数を集める。
/// マクロで作る関数は数えない
pub fn api(source: &str, out: &mut BTreeMap<String, String>) {
    let mut depth = 0i32;
    // (入った時の深さ, 型の名前, trait の中か)
    let mut blocks: Vec<(i32, String, bool)> = Vec::new();
    let mut pending: Option<(String, String)> = None;
    let mut in_macro = 0i32;
    for line in source.lines() {
        let t = line.trim();
        if t.starts_with("//") || t.starts_with("#[") {
            continue;
        }
        // 宣言が何行かに分かれている時は、{ か ; まで足す
        if let Some((name, sig)) = pending.as_mut() {
            sig.push(' ');
            sig.push_str(t);
            if t.contains('{') || t.ends_with(';') {
                let s = sig.split('{').next().unwrap().trim().to_string();
                out.insert(std::mem::take(name), s);
                pending = None;
            }
        } else if t.starts_with("macro_rules!") {
            in_macro = depth + 1;
        } else if in_macro == 0 {
            let owner = blocks.last().map(|(_, n, tr)| (n.clone(), *tr));
            let name = if let Some(n) = ident_after(t, "pub fn ") {
                Some(match &owner {
                    Some((o, _)) => format!("{o}::{n}"),
                    None => n.to_string(),
                })
            } else if let (Some((o, true)), Some(n)) = (&owner, ident_after(t, "fn ")) {
                Some(format!("{o}::{n}"))
            } else {
                ["pub struct ", "pub enum ", "pub trait "].iter().find_map(|p| ident_after(t, p)).map(str::to_string)
            };
            if let Some(name) = name {
                if t.contains('{') || t.ends_with(';') {
                    out.insert(name, t.split('{').next().unwrap().trim().to_string());
                } else {
                    pending = Some((name, t.to_string()));
                }
            }
        }
        // 中かっこの深さ（文字列の中は数えない）
        let code: String = t.split('"').step_by(2).collect();
        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;
        if opens > 0 && in_macro == 0 {
            if let Some(ty) = impl_type(t) {
                blocks.push((depth + 1, ty, false));
            } else if let Some(n) = ident_after(t, "pub trait ") {
                blocks.push((depth + 1, n.to_string(), true));
            }
        }
        depth += opens - closes;
        while blocks.last().is_some_and(|(d, _, _)| depth < *d) {
            blocks.pop();
        }
        if in_macro > 0 && depth < in_macro {
            in_macro = 0;
        }
    }
}

/// ページの先頭のコメント `<!-- key: ... -->` の中身
fn header<'a>(page: &'a str, key: &str) -> Option<&'a str> {
    page.lines().take(4).find_map(|l| l.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim().strip_prefix(key)).map(str::trim)
}

/// 扱う API の名前と、入っている sig=
pub fn covers(page: &str) -> Option<(Vec<String>, Option<String>)> {
    let body = header(page, "covers:")?;
    let mut names = Vec::new();
    let mut sig = None;
    for w in body.split_whitespace() {
        match w.strip_prefix("sig=") {
            Some(s) => sig = Some(s.to_string()),
            None => names.push(w.trim_start_matches("test::").to_string()),
        }
    }
    Some((names, sig))
}

/// 扱う API の宣言の文から作る値（名前の順に並べてつなぐ）。無い名前は Err
pub fn signature_hash(names: &[String], api: &BTreeMap<String, String>) -> Result<String, Vec<String>> {
    let mut missing = Vec::new();
    let mut sorted = names.to_vec();
    sorted.sort();
    let mut text = String::new();
    for n in &sorted {
        match api.get(n) {
            Some(s) => {
                text.push_str(n);
                text.push('=');
                text.push_str(s);
                text.push('\n');
            }
            None => missing.push(n.clone()),
        }
    }
    if missing.is_empty() { Ok(fnv(&text)) } else { Err(missing) }
}

/// 訳の元（`ja/01-first-app.md`）と、入っている fnv=
pub fn source(page: &str) -> Option<(String, Option<String>)> {
    let body = header(page, "source:")?;
    let mut it = body.split_whitespace();
    let path = it.next()?.to_string();
    let hash = it.find_map(|w| w.strip_prefix("fnv=")).map(str::to_string);
    Some((path, hash))
}

/// 訳の元の値。扱う API の行は除く（sig を入れ直しただけで訳が古くならないように）
pub fn source_hash(ja: &str) -> String {
    let body: Vec<&str> = ja.lines().filter(|l| !l.trim_start().starts_with("<!-- covers:")).collect();
    fn_join(&body)
}

fn fn_join(lines: &[&str]) -> String {
    fnv(&lines.join("\n"))
}

/// ページの先頭のコメントの値を入れ直した文
pub fn stamp(page: &str, key: &str, field: &str, value: &str) -> String {
    let mut out = Vec::new();
    for (i, line) in page.lines().enumerate() {
        if i < 4 && line.trim_start().starts_with(&format!("<!-- {key}")) {
            let inner = line.trim().trim_start_matches("<!--").trim_end_matches("-->").trim();
            let kept: Vec<&str> = inner.split_whitespace().filter(|w| !w.starts_with(&format!("{field}="))).collect();
            out.push(format!("<!-- {} {field}={value} -->", kept.join(" ")));
        } else {
            out.push(line.to_string());
        }
    }
    let mut s = out.join("\n");
    if page.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
/// A thing.
pub struct Ui<'a> {
    cx: u8,
}
impl<'a> Ui<'a> {
    /// Docs with { braces } and "strings {".
    #[track_caller]
    pub fn button(&mut self, text: impl Into<String>) -> Button<'_> {
        let s = "}";
    }
    pub fn slider<T: SliderValue>(
        &mut self,
        value: &mut T,
    ) -> Slider<'_> {
    }
    fn private(&self) {}
}
pub trait App: 'static {
    fn view(&mut self, ui: &mut Ui);
    fn presented(&mut self, _frame: &Frame) {}
}
impl std::fmt::Display for Error {
    fn fmt(&self) {}
}
macro_rules! common {
    ($t:ident) => {
        impl<'t> $t<'t> {
            pub fn width(self) -> Self {}
        }
    };
}
impl<A: App> Harness<A> {
    pub fn click(&mut self, name: &str) -> &mut Self {
        self
    }
}
pub fn run(options: Options, app: impl App) -> Result<(), Error> {
}
"#;

    #[test]
    fn api_finds_public_items_and_their_signatures() {
        let mut m = BTreeMap::new();
        api(SRC, &mut m);
        let names: Vec<&str> = m.keys().map(String::as_str).collect();
        assert_eq!(names, ["App", "App::presented", "App::view", "Harness::click", "Ui", "Ui::button", "Ui::slider", "run"]);
        assert_eq!(m["Ui::button"], "pub fn button(&mut self, text: impl Into<String>) -> Button<'_>");
        assert_eq!(m["Ui::slider"], "pub fn slider<T: SliderValue>( &mut self, value: &mut T, ) -> Slider<'_>");
        assert_eq!(m["App::view"], "fn view(&mut self, ui: &mut Ui);");
    }

    #[test]
    fn covers_and_sources_are_read_and_stamped() {
        let page = "<!-- source: ja/01.md fnv=abc -->\n<!-- covers: Ui::button test::Harness sig=111 -->\n# Title\n";
        assert_eq!(covers(page), Some((vec!["Ui::button".to_string(), "Harness".to_string()], Some("111".to_string()))));
        assert_eq!(source(page), Some(("ja/01.md".to_string(), Some("abc".to_string()))));
        let p = stamp(page, "covers:", "sig", "222");
        assert!(p.contains("<!-- covers: Ui::button test::Harness sig=222 -->"), "{p}");
        let p = stamp(&p, "source:", "fnv", "def");
        assert!(p.starts_with("<!-- source: ja/01.md fnv=def -->\n"), "{p}");
        // 値の無いページにも入れられる
        let p = stamp("<!-- covers: App -->\nx\n", "covers:", "sig", "9");
        assert_eq!(p, "<!-- covers: App sig=9 -->\nx\n");
    }

    #[test]
    fn signature_hash_changes_when_a_covered_signature_changes() {
        let mut m = BTreeMap::new();
        api(SRC, &mut m);
        let names = vec!["Ui::button".to_string(), "run".to_string()];
        let a = signature_hash(&names, &m).unwrap();
        m.insert("Ui::button".into(), "pub fn button(&mut self, text: &str) -> Button<'_>".into());
        assert_ne!(signature_hash(&names, &m).unwrap(), a);
        // 扱っていない API が変わっても変わらない
        let b = signature_hash(&names, &m).unwrap();
        m.insert("Ui::slider".into(), "changed".into());
        assert_eq!(signature_hash(&names, &m).unwrap(), b);
        assert_eq!(signature_hash(&["Nope".to_string()], &m), Err(vec!["Nope".to_string()]));
    }

    #[test]
    fn the_source_hash_ignores_the_covers_line() {
        let a = source_hash("<!-- covers: App sig=1 -->\n# 題\n本文\n");
        let b = source_hash("<!-- covers: App sig=2 -->\n# 題\n本文\n");
        let c = source_hash("<!-- covers: App sig=2 -->\n# 題\n本文を直した\n");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // 改行の形は数えない
        assert_eq!(fnv("a\r\nb"), fnv("a\nb"));
    }
}
