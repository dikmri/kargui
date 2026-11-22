# kargui のガイド（日本語）

kargui は、Rust で Windows のデスクトップアプリを書くための、軽い GUI フレームワークです。
このガイドは日本語が元で、英語版（`docs/guide/en/`）はこれを訳した物です。

ガイドの中のコードは、すべて `cargo test -p kargui --doc` で動かして確かめています。
API が変わったのにガイドを直していないと、検査（`cargo run -p xtask -- guide-check`）で落ちます。

1. [最初のアプリ](01-first-app.md)
2. [部品](02-widgets.md)
3. [並べ方](03-layout.md)
4. [状態と id](04-state-and-ids.md)
5. [一覧と表](05-lists-and-tables.md)
6. [スクロールと長い文](06-scroll-and-text.md)
7. [日本語入力](07-japanese-input.md)
8. [読み上げとキーボード](08-accessibility.md)
9. [試験の書き方](09-testing.md)
10. [キー・時間・ほかのスレッド](10-keys-time-threads.md)
11. [いろいろな部品](11-more-widgets.md)
12. [メニュー](12-menus.md)
13. [タブ・数の入力・ツールバー](13-tabs-numbers-toolbars.md)
14. [格子・仕切り・自由に描く面](14-grids-splits-canvas.md)
15. [木の表示・表・日付](15-trees-tables-dates.md)
16. [複数の窓](16-windows.md)
17. [アプリの仕組み](17-app-mechanisms.md)
18. [ドラッグ＆ドロップ](18-drag-and-drop.md)
19. [動き](19-animation.md)
20. [書体](20-fonts.md)
21. [色と時刻の選択](21-color-and-time.md)

API の一つ一つの説明は、`cargo doc -p kargui --open` で読めます。AI に書かせる時は `docs/llms.txt` を渡してください。
