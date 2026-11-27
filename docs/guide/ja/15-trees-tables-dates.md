<!-- covers: Ui::tree TreeBuilder::leaf TreeBuilder::branch TreeView::changed Ui::table Table::sort Table::selection Table::label Table::grow Table::show Table::show_keyed TableRow::cell TableRow::text TableRow::context_menu TableColumn::new TableColumn::width TableColumn::fill TableColumn::right Ui::date_picker DatePicker::changed Ui::set_date_text Ui::today Date::new Date::today Date::year Date::month Date::day Date::weekday Date::add_days Date::add_months Date::days_in_month Date::from_unix_days Date::unix_days DateText::english DateText::japanese sig=1e24c244697ffaa8 -->
# 木の表示・表・日付

## 木の表示

フォルダーや見出しの階層のように、開いたり閉じたりする項目を並べます。

```rust
# let mut selected = None;
# kargui::test::example(|ui| {
ui.tree(&mut selected, |t| {
    t.branch("docs", "書類", |t| {
        t.leaf("report", "報告書.pdf");
        t.branch("photos", "写真", |t| {
            t.leaf("cat", "猫.jpg");
        });
    });
    t.leaf("notes", "メモ.txt");
})
.label("ファイル");
# });
```

`t.branch(キー, 文字, |t| ..)` は子を持つ項目、`t.leaf(キー, 文字)` は子を持たない項目です。項目はキーで見分け、選んでいる項目のキーが `selected` に入ります。枝は最初は閉じていて、開閉の状態は kargui が覚えます。閉じた枝の子は作らないので、大きな木でも軽く動きます。

項目を押すと選び、左の矢印の印を押すと開閉します。キーボードでは、Tab は選んでいる項目に 1 回だけ止まります。上下・Home・End・文字で移って選びます。右の矢印は、閉じた枝を開くか、開いた枝の最初の子へ移ります。左の矢印は、開いた枝を閉じるか、親へ移ります。Enter と Space で開閉します。

読み上げソフトには、木と、深さ・開いているか・選んでいるかを持つ項目として伝わります。

## 表

列の見出しの下に、行を並べます。行は一覧と同じく、見えている分だけ作ります。

```rust
use kargui::ui::TableColumn;
# struct File { name: String, size: u64 }
# let mut files = vec![File { name: "b.txt".into(), size: 20 }, File { name: "a.png".into(), size: 300 }];
# let (mut sort, mut selected) = (None, None);
# kargui::test::example(|ui| {
// 並べ替えはアプリが行う
match sort {
    Some((0, asc)) => files.sort_by(|a, b| if asc { a.name.cmp(&b.name) } else { b.name.cmp(&a.name) }),
    Some((1, asc)) => files.sort_by(|a, b| if asc { a.size.cmp(&b.size) } else { b.size.cmp(&a.size) }),
    _ => {}
}
let columns = [TableColumn::new("名前").width(220.0), TableColumn::new("大きさ").width(90.0).right(), TableColumn::new("メモ").fill()];
ui.table(&columns, files.len(), 30.0).sort(&mut sort).selection(&mut selected).label("ファイル").grow().show(|row, i| {
    row.text(&files[i].name);
    row.text(format!("{} KB", files[i].size));
    row.cell(|ui| {
        ui.link("開く");
    });
});
# });
```

- `TableColumn::new(見出し)` で列を作ります。`.width(幅)` で始めの幅、`.fill()` で残りの幅、`.right()` で右寄せ（数の列）にします
- `.sort(&mut 並べ方)` を付けると、見出しを押すたびに `Some((列, true))`（小さい順）と `Some((列, false))` が入れ替わります。データの並べ替えはアプリが行います
- `.selection(&mut 選んでいる行)` で行を選べるようにします（一覧の `selection` と同じ）
- 行の中では、`row.text(文字)` か `row.cell(|ui| ..)` で、列の順に升を足します
- `row.context_menu(|m| ..)` で、行に右クリックのメニューを付けます

見出しの横の小さいつまみをつかむと、列の幅を変えられます（kargui が覚えます）。つまみは列の右の端にあります。ただし、残りの幅を使う列（`.fill()`）より右の列は右の端が動かないので、つまみは左の端にあります。見出しに焦点がある時は、Shift+→ で広く、Shift+← で狭くなります（残りの幅を使う列では、隣の列との境が動きます）。見出しは Tab で 1 回だけ止まり、左右で移り、Enter か Space で並べ替えます。`.grow()` を付けない時は、8 行分の高さで表示してスクロールします。

読み上げソフトには、表・見出しの行（並べ替えの向きを含む）・行・升として、行と列の番号つきで伝わります。

## 日付の選択

日付の欄で、押すと下に月の暦が開きます。

```rust
# let mut due: Option<kargui::Date> = None;
# kargui::test::example(|ui| {
ui.set_date_text(kargui::DateText::japanese());
if ui.date_picker(&mut due).label("期限").changed() {
    // 選んだ日付を保存する
}
# });
```

日付は `kargui::Date` で、まだ選んでいなければ `None` です。暦の日を押すか、暦の中で Enter か Space を押すと選んで閉じます。Esc か外を押すと、選ばずに閉じます。

暦の中のキーは、矢印で 1 日か 1 週、Home と End で週の初めと終わり、PageUp と PageDown で 1 か月、Shift を付けると 1 年動きます。欄に焦点がある時は、Enter・Space・Alt+下・F4 で暦が開きます。

欄は日付を `2026-11-22` の形で出します。曜日と月の名前は、kargui の言語（17 章の「言語」）に合わせて英語か日本語になり、`ui.set_date_text(..)` で決め直せます（`DateText::japanese()` で日本語）。読み上げソフトには、値を持つ日付の欄と、日ごとの名前（2026年11月22日など）を持つ暦の格子として伝わります。

`Date` は、年・月・日を持つ型です。`Date::new(年, 月, 日)` で作り（無い日なら `None`）、`add_days`・`add_months`・`weekday` で計算します。ファイルの更新日時などからは、`Date::from_unix_days(秒 / 86400)` で UTC の日付を作れます（逆は `unix_days`）。`ui.today()` は今日の日付です（試験の中では、いつも 2026-11-22）。
