<!-- source: ja/README.md fnv=b2e4b5a744f29c56 -->
# The kargui guide (English)

kargui is a light GUI framework for writing Windows desktop apps in Rust.
This guide is written in Japanese first; this English version is a translation of `docs/guide/ja/`.

All code in this guide runs in `cargo test -p kargui --doc`. If the API changes and the guide is not
updated in the same change, the check (`cargo run -p xtask -- guide-check`) fails.

1. [Your first app](01-first-app.md)
2. [Widgets](02-widgets.md)
3. [Layout](03-layout.md)
4. [State and ids](04-state-and-ids.md)
5. [Lists and tables](05-lists-and-tables.md)
6. [Scrolling and long text](06-scroll-and-text.md)
7. [Japanese input (IME)](07-japanese-input.md)
8. [Screen readers and the keyboard](08-accessibility.md)
9. [Writing tests](09-testing.md)
10. [Keys, time and other threads](10-keys-time-threads.md)

Each item of the API is described in `cargo doc -p kargui --open`. When an AI writes code with kargui, give it `docs/llms.txt`.
