<!-- covers: Ui::scroll Label::wrap Label::size Handle::scroll_into_view sig=ca0d715071c2fdd1 -->
# スクロールと長い文

## スクロールの入れ物

窓より高くなる画面（設定の並び、長い文）は `ui.scroll` に入れます。中の部品は上から下へ並びます。
高さは既定で残りを取ります。右にはスクロールの棒の分の幅をいつも空けます。

```rust
# kargui::test::example(|ui| {
ui.heading("設定");
ui.scroll(|ui| {
    for i in 0..50 {
        ui.label(format!("{i} 行目"));
    }
});
ui.button("保存");
# });
```

見えない所にある部品にも Tab で移れます。移ると、その部品が見えるようにスクロールします。

## 折り返す文

`wrap()` を付けると、幅に合わせて折り返します。読む画面では、文字の大きさを `size(..)` で変えられるようにすると読みやすくなります。

```rust
# let font = 16.0;
# kargui::test::example(|ui| {
ui.scroll(|ui| {
    ui.label("第 1 章").bold().size(font * 1.35);
    ui.label("長い段落は、入れ物の幅に合わせて折り返します。").size(font).wrap();
});
# });
```

## 目次から飛ぶ

見出しを作る時に `scroll_into_view()` を呼ぶと、そのフレームの配置の後で、見出しをスクロールの入れ物の上端へ持ってきます。

```rust
# let mut jump_to: Option<usize> = Some(2);
# kargui::test::example(|ui| {
ui.scroll(|ui| {
    for chapter in 0..5 {
        let heading = ui.heading(format!("第 {chapter} 章"));
        if jump_to == Some(chapter) {
            heading.scroll_into_view();
        }
        ui.label("…").wrap();
    }
});
jump_to = None;
# });
```

目次と本文の見出しを同じ名前にしても構いません。`examples/reader` が、目次・前の章・次の章・文字の大きさを持つ読む画面の例です。
