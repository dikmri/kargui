<!-- source: ja/12-menus.md fnv=e1bb50b8c288f1e7 -->
<!-- covers: Ui::menu_button Ui::menu_bar MenuBar::menu Menu::item Menu::check Menu::radio Menu::separator Menu::submenu MenuItem::shortcut MenuItem::enabled MenuItem::icon MenuItem::clicked Handle::context_menu Label::context_menu Harness::right_click sig=fca41d6176841f35 -->
# Menus

## Menu buttons

A button that opens a menu below it. Build the items in the closure you pass.

```rust
use kargui::{Key, Mods};
# let (mut text, mut wrap) = (String::from("Notes"), false);
# kargui::test::example(|ui| {
ui.menu_button("Edit", |m| {
    if m.item("Clear all").shortcut(Key::L, Mods::CTRL).clicked() {
        text.clear();
    }
    m.separator();
    m.check(&mut wrap, "Word wrap");
});
# });
```

Choosing an item closes the menu. Escape or a click outside closes it without choosing.

The kinds of items:

- `m.item(text)`: an item whose `.clicked()` is true when it is chosen
- `m.check(&mut value, text)`: an item with a check mark that flips the value each time it is chosen
- `m.radio(&mut value, this, text)`: one of several items where one is chosen; the chosen one has a dot
- `m.separator()`: a line between items
- `m.submenu(text, |m| ..)`: an item that opens another menu beside it

An item with `.enabled(false)` is drawn dimmed and cannot be chosen. `.icon(..)` shows an icon at the left of the item.

## Shortcuts

`.shortcut(key, mods)` shows the keys at the right of the item. Pressing them chooses the item, even while the menu is closed. The closure that builds the menu runs every frame, also while the menu is closed; then it creates no items and only checks whether the keys were pressed.

Keys that the focused widget uses itself stay with it. For example, Ctrl+C in a text field copies its text.

## Menu bars

```rust
use kargui::{Key, Mods};
# let mut saved = 0;
# kargui::test::example(|ui| {
ui.menu_bar(|bar| {
    bar.menu("File", |m| {
        if m.item("Save").shortcut(Key::S, Mods::CTRL).clicked() {
            saved += 1;
        }
        m.submenu("Open recent", |m| {
            m.item("notes.txt");
            m.item("plan.md");
        });
    });
    bar.menu("Help", |m| {
        m.item("About");
    });
});
# });
```

While one menu is open, pointing at another heading switches to that menu. A submenu opens when its item is clicked or when the pointer rests on the item for 0.4 seconds.

## Keyboard

| Keys | What they do |
|---|---|
| F10 | Moves to the first heading of the menu bar (again: back to where the focus was) |
| Enter, Space, Down | Opens the menu at the first item |
| Up | Opens the menu at the last item |
| Up, Down, Home, End | Move between the items in the menu (skipping unavailable ones) |
| A letter | Moves to the next item starting with it |
| Right, Left | Open and close a submenu; in a menu of the menu bar, go to the next or previous menu |
| Escape | Closes the top menu |
| Tab | Closes the menu |

After an item of a menu bar's menu is chosen, the focus goes back to where it was before the menu bar. Tab stops once on the menu bar.

Screen readers hear a button that opens a menu, a menu and menu items. For check and radio items they also hear whether the item is chosen, and they hear the shortcut keys.

## Context menus

Any widget can have a menu that opens on a right-click, with `.context_menu(|m| ..)`. Containers such as rows and columns can have one too.

```rust
# let mut files = vec!["notes.txt".to_string(), "plan.md".to_string()];
# kargui::test::example(|ui| {
let mut remove = None;
for (k, name) in files.iter().enumerate() {
    ui.label(name).context_menu(|m| {
        if m.item("Delete").clicked() {
            remove = Some(k);
        }
    });
}
if let Some(k) = remove {
    files.remove(k);
}
# });
```

The menu opens where the widget was right-clicked. While the focus is on the widget (or on a widget inside it), Shift+F10 or the context menu key also opens it, at the widget's lower left corner. The shortcuts of a context menu's items work only while the focus is inside its widget.

In tests, `h.right_click(name)` right-clicks a widget.
