//! 起動引数。3 つの版で同じ引数を受け付ける。知らない引数は無視する。
//!
//! --items N          起動時に見本のタスクを N 件作る
//! --bench-scroll S   表示から 1 秒後に S 秒間、一覧を自動でスクロールする
//! --software         GPU を使わずに描く（使える版だけ）
//! --log PATH         記録ファイルの置き場所（既定は logs/rust-gui/<版>.log）
//! --run-id ID        測定の道具がプロセスを見分けるための札（中身は使わない）
//! --pos X,Y          窓の左上の位置（論理ピクセル。サブモニターで測るときに使う）
//! --no-focus         窓を開いても前面に出さない（作業中の窓から入力を奪わない）
//! --renderer NAME    描き方を選ぶ（kargui の試作: d3d11 か cpu）
//! --full-rate        前面にない窓でもフレームを抑えない（比べるときに使う）
//! --light / --dark   OS の設定に関わらず明るい方・暗い方で描く（写真を撮るときに使う。使える版だけ）

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Args {
    pub items: usize,
    pub bench_scroll: Option<f32>,
    pub software: bool,
    pub log_path: Option<String>,
    pub run_id: Option<String>,
    pub pos: Option<(f32, f32)>,
    pub no_focus: bool,
    pub renderer: Option<String>,
    pub full_rate: bool,
    /// Some(true) は暗い方、Some(false) は明るい方。None なら OS の設定に従う
    pub dark: Option<bool>,
}

/// プログラム名を除いた引数の並びを読む。
pub fn parse<I: IntoIterator<Item = String>>(it: I) -> Args {
    let mut a = Args::default();
    let mut it = it.into_iter();
    while let Some(arg) = it.next() {
        // "--name=value" と "--name value" の両方を受け付ける
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        let value = |it: &mut I::IntoIter| inline.clone().or_else(|| it.next());
        match name.as_str() {
            "--items" => {
                if let Some(n) = value(&mut it).and_then(|v| v.parse().ok()) {
                    a.items = n;
                }
            }
            "--bench-scroll" => a.bench_scroll = value(&mut it).and_then(|v| v.parse().ok()),
            "--software" => a.software = true,
            "--log" => a.log_path = value(&mut it),
            "--run-id" => a.run_id = value(&mut it),
            "--pos" => a.pos = value(&mut it).and_then(|v| parse_pos(&v)),
            "--no-focus" => a.no_focus = true,
            "--renderer" => a.renderer = value(&mut it),
            "--full-rate" => a.full_rate = true,
            "--light" => a.dark = Some(false),
            "--dark" => a.dark = Some(true),
            _ => {}
        }
    }
    a
}

/// "x,y" を読む。負の数も受け付ける。
fn parse_pos(v: &str) -> Option<(f32, f32)> {
    let (x, y) = v.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

pub fn from_env() -> Args {
    parse(std::env::args().skip(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &[&str]) -> Args {
        parse(s.iter().map(|s| s.to_string()))
    }

    #[test]
    fn empty_means_defaults() {
        assert_eq!(p(&[]), Args::default());
    }

    #[test]
    fn reads_space_and_equals_forms() {
        let a = p(&["--items", "10000", "--bench-scroll=5", "--software", "--log", "x.log", "--run-id=r1"]);
        assert_eq!(a.items, 10_000);
        assert_eq!(a.bench_scroll, Some(5.0));
        assert!(a.software);
        assert_eq!(a.log_path.as_deref(), Some("x.log"));
        assert_eq!(a.run_id.as_deref(), Some("r1"));
    }

    #[test]
    fn reads_window_position_and_focus() {
        let a = p(&["--pos", "-1700,100", "--no-focus"]);
        assert_eq!(a.pos, Some((-1700.0, 100.0)));
        assert!(a.no_focus);
        assert_eq!(p(&["--pos=-5.5,-3"]).pos, Some((-5.5, -3.0)));
        assert_eq!(p(&["--pos", "12"]).pos, None);
        assert_eq!(p(&["--pos", "a,b"]).pos, None);
    }

    #[test]
    fn reads_renderer_and_full_rate() {
        let a = p(&["--renderer", "cpu", "--full-rate"]);
        assert_eq!(a.renderer.as_deref(), Some("cpu"));
        assert!(a.full_rate);
        assert_eq!(p(&["--renderer=d3d11"]).renderer.as_deref(), Some("d3d11"));
        assert_eq!(p(&["--renderer"]).renderer, None);
    }

    #[test]
    fn reads_light_and_dark() {
        assert_eq!(p(&[]).dark, None, "無ければ OS の設定");
        assert_eq!(p(&["--light"]).dark, Some(false));
        assert_eq!(p(&["--dark"]).dark, Some(true));
        assert_eq!(p(&["--dark", "--light"]).dark, Some(false), "後の物が勝つ");
    }

    #[test]
    fn ignores_unknown_and_bad_numbers() {
        let a = p(&["--unknown", "--items", "many", "--bench-scroll"]);
        assert_eq!(a.items, 0);
        assert_eq!(a.bench_scroll, None);
    }
}
