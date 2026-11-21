// 長い文を読む画面（kargui の例）。左に目次、右に折り返す段落をスクロールの入れ物に入れる。
// 目次や「次の章」を押すと、その章の見出しを入れ物の上端へ持ってくる（scroll_into_view）。
// 文字の大きさはスライダーで変える。文は例のために書いた物（著作権の心配がない）。
// --repeat N で章を N 回繰り返し、長い文での速さを測れる。
#![windows_subsystem = "windows"]

use std::time::Instant;

use kargui::{Align, App, Frame, Options, Size, Ui};
use taskcore::{args, log};

const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/reader.log");

/// 章の題と段落
const BOOK: &[(&str, &[&str])] = &[
    (
        "春 ・ 窓を開ける日",
        &[
            "町の図書館は、駅から歩いて十分ほどの坂の上にある。三月の終わりの朝、館長は二階の窓を一枚ずつ開けて回る。冬のあいだ閉じこもっていた紙の匂いが、ゆっくりと外の空気と入れ替わっていく。",
            "この日から、入口の前に小さな黒板が出る。「今月の一冊」を書くのは、毎年いちばん新しく入った職員の役目だ。今年の黒板には、少し右に傾いた字で、植物の図鑑の名前が書かれていた。",
            "開館の十時より前に、もう三人が石段に座って待っている。新聞を読みに来る人、勉強の場所を取りに来る学生、そして返し忘れた本を抱えて、申し訳なさそうに笑う人。",
        ],
    ),
    (
        "夏 ・ 涼しい場所",
        &[
            "七月になると、図書館は町でいちばん涼しい場所のひとつになる。子どもたちは自由研究の題を探しに来て、たいていは恐竜か宇宙の棚の前で動かなくなる。",
            "職員は、返ってきた本の背を一冊ずつ指で確かめる。日に焼けた本、砂の入った本、ページの角が折れた本。どれも、この夏にどこかで読まれていたしるしだ。",
            "閉館の前の三十分だけ、窓の外の蝉の声が急に大きく聞こえる。誰も話していない部屋で、ページをめくる音と、遠くの踏切の音だけが重なっている。",
            "夏休みの最後の週には、読書感想文の相談が増える。何を書けばいいか分からないと言う子に、職員は「いちばん覚えている場面を一つだけ教えて」と聞く。たいていの子は、そこから急に話し始める。",
        ],
    ),
    (
        "秋 ・ 本を直す",
        &[
            "秋は、傷んだ本を直す季節だ。背が割れた本、表紙が外れかけた本が、作業台の上に順番に並べられる。のりは薄く、待つ時間は長く、手順は毎年ほとんど変わらない。",
            "直した本は、すぐには棚に戻さない。重しを載せて一晩置き、次の朝にもう一度開いて、閉じる。きちんと閉じた本だけが、また貸し出しの棚に帰っていく。",
            "作業の手を止めて窓の外を見ると、坂の下の銀杏が少しずつ色を変えている。去年直した本を、今年また直すこともある。それだけ読まれたということなので、職員は少しうれしい。",
        ],
    ),
    (
        "冬 ・ 灯りの下で",
        &[
            "冬の夕方は早く暗くなる。四時を過ぎると、閲覧室の灯りが一つずつ点き、窓ガラスに部屋の中が映り始める。外を歩く人からは、本を読む人の背中がよく見える。",
            "この季節は、古い新聞の綴じ込みを調べに来る人が多い。自分の生まれた日の新聞、祖父が働いていた工場の記事、昔の駅前の写真。探し物が見つかると、たいていの人は小さく声を上げる。",
            "年の終わりの日、館長は最後に二階の窓の鍵を確かめる。一年でいちばん静かな部屋で、壁の時計の音だけが聞こえる。",
        ],
    ),
    (
        "記録 ・ 一年の数字",
        &[
            "この図書館の一年は、数字にするとそれほど大きくない。貸し出しは一日に二百冊ほど、新しく入る本は一年に三千冊ほど、直す本は四百冊ほどだ。",
            "けれども、数字に出ない仕事も多い。道を聞かれて地図を描くこと、雨の日に傘を貸すこと、長い時間座っている人に、何も言わずに温かいお茶を出すこと。",
            "館長は、毎年の報告書の最後に一行だけ、数字ではないことを書くと決めている。今年は「黒板の字が、少しまっすぐになった」と書いた。",
        ],
    ),
    (
        "また春 ・ 次の一冊",
        &[
            "三月の終わり、また窓を開ける日が来る。去年の新しい職員は、今年はもう新しい職員ではない。黒板の前で少し迷ってから、詩の本の名前を書いた。",
            "坂を上ってくる人の数は、去年とほとんど変わらない。けれども、その中の何人かは、去年とは違う棚の前で足を止める。図書館の一年は、そうやって少しずつ形を変えていく。",
        ],
    ),
];

struct Reader {
    /// 章の並び（--repeat で繰り返す）
    chapters: Vec<(String, &'static [&'static str])>,
    font: f32,
    dark: bool,
    current: usize,
    jump: Option<usize>,
    started: Instant,
    ready: bool,
}

impl Reader {
    fn new(repeat: usize) -> Self {
        let chapters = (0..repeat.max(1))
            .flat_map(|r| BOOK.iter().map(move |(t, p)| (if r == 0 { t.to_string() } else { format!("{t}（{}）", r + 1) }, *p)))
            .collect();
        Self { chapters, font: 16.0, dark: false, current: 0, jump: None, started: Instant::now(), ready: false }
    }

    fn go(&mut self, ch: usize, why: &str) {
        self.current = ch.min(self.chapters.len() - 1);
        self.jump = Some(self.current);
        log::event("jump", &format!("why={why} chapter={}", self.current), "ok");
    }
}

impl App for Reader {
    fn view(&mut self, ui: &mut Ui) {
        ui.set_dark(self.dark);
        ui.row(|ui| {
            ui.heading("町の図書館の一年");
            ui.fill();
            ui.label("文字");
            ui.slider(&mut self.font, 12.0..=28.0).label("文字の大きさ").width(Size::Fixed(140.0));
            ui.toggle(&mut self.dark, "ダーク");
        });
        let mut go = None;
        ui.row(|ui| {
            ui.column(|ui| {
                ui.label("目次").bold();
                for (i, (title, _)) in self.chapters.iter().enumerate() {
                    let b = ui.button(title.as_str()).small();
                    let b = if i == self.current { b.primary() } else { b };
                    if b.clicked() {
                        go = Some(i);
                    }
                }
            })
            .width(Size::Fixed(200.0));
            let (font, jump) = (self.font, self.jump.take());
            ui.scroll(|ui| {
                for (i, (title, paragraphs)) in self.chapters.iter().enumerate() {
                    let h = ui.label(title.as_str()).bold().size(font * 1.35);
                    if jump == Some(i) {
                        h.scroll_into_view();
                    }
                    for p in paragraphs.iter() {
                        ui.label(*p).size(font).wrap();
                    }
                    ui.space(font);
                }
            })
            .grow();
        })
        .align(Align::Stretch)
        .grow();
        ui.row(|ui| {
            if ui.button("前の章").clicked() {
                go = Some(self.current.saturating_sub(1));
            }
            if ui.button("次の章").clicked() {
                go = Some(self.current + 1);
            }
            ui.label(format!("第 {} 章 / 全 {} 章", self.current + 1, self.chapters.len())).weak();
        });
        if let Some(ch) = go {
            self.go(ch, "button");
        }
    }

    fn presented(&mut self, _frame: &Frame) {
        if !self.ready {
            self.ready = true;
            log::event("ready", &format!("app=reader chapters={}", self.chapters.len()), &format!("since_main_ms={}", self.started.elapsed().as_millis()));
        }
    }
}

fn main() {
    let a = args::from_env();
    let path = log::init("kargui-reader", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    // 段落の数を増やして測る時は --repeat N（taskcore の引数に無いので、ここで読む）
    let argv: Vec<String> = std::env::args().collect();
    let repeat = argv.iter().position(|a| a == "--repeat").and_then(|i| argv.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1);
    log::event("startup", &format!("app=reader repeat={repeat}"), &format!("log={path:?}"));
    let mut options = Options::new("町の図書館の一年（kargui）");
    options.size = (820.0, 600.0);
    options.position = a.pos;
    options.activate = !a.no_focus;
    if let Err(e) = kargui::run(options, Reader::new(repeat)) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}

#[cfg(test)]
mod tests {
    use super::*;
    use kargui::test::Harness;

    fn app() -> Harness<Reader> {
        let mut h = Harness::with_size(Reader::new(1), 820.0, 600.0, 1.0);
        h.frame();
        h
    }

    #[test]
    fn the_table_of_contents_jumps_to_a_chapter() {
        let mut h = app();
        // 目次のボタンと本文の見出しは同じ名前。1 つ目が目次（左の列）、2 つ目が本文
        let top = h.find("目次").unwrap().y;
        h.click("冬 ・ 灯りの下で");
        assert_eq!(h.app().current, 3);
        h.frame();
        let both = h.find_all("冬 ・ 灯りの下で");
        assert_eq!(both.len(), 2);
        assert!(both[0].x < 220.0 && both[1].x > 220.0);
        // 本文の見出しが、入れ物の上端（目次の列の上端と同じ高さ）に来る
        assert!((both[1].y - top).abs() <= 2.0, "見出し {:?} / 上端 {top}", both[1]);
        h.click("次の章");
        assert_eq!(h.app().current, 4);
        h.click("前の章").click("前の章");
        assert_eq!(h.app().current, 2);
    }

    #[test]
    fn larger_text_makes_the_book_longer() {
        let mut h = app();
        let body_y = |h: &Harness<Reader>| h.find_all("また春 ・ 次の一冊")[1].y;
        let before = body_y(&h);
        h.app_mut().font = 26.0;
        h.frame().frame();
        assert!(body_y(&h) > before * 1.3, "文字を大きくすると最後の章が下へ: {before} → {}", body_y(&h));
    }
}
