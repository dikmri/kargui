<!-- source: ja/README.md fnv=9894a6e96a8c8af9 -->
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
11. [More widgets](11-more-widgets.md)
12. [Menus](12-menus.md)
13. [Tabs, number inputs and toolbars](13-tabs-numbers-toolbars.md)
14. [Grids, split panes and drawing surfaces](14-grids-splits-canvas.md)
15. [Trees, tables and dates](15-trees-tables-dates.md)
16. [More windows](16-windows.md)
17. [App mechanisms](17-app-mechanisms.md)
18. [Drag and drop](18-drag-and-drop.md)
19. [Motion](19-animation.md)
20. [Fonts](20-fonts.md)
21. [Color and time pickers](21-color-and-time.md)
22. [Rich text](22-rich-text.md)

Each item of the API is described in `cargo doc -p kargui --open`. When an AI writes code with kargui, give it `docs/llms.txt`.
