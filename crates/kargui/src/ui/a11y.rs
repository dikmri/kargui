//! 読み上げの木（AccessKit）を画面の木から作る。
//!
//! 部品の種類から役割を決め、名前は部品の文字（Handle::label で上書きできる）から取る。
//! 名前の無い操作の部品は記録で警告する（段階 0 の比べで、egui と GPUI は警告しないまま名前の無い部品が 15〜18 個あった）。
//! 一覧は見えている行だけを入れ、行ごとに「何件中何件目」を持たせる。

use std::collections::HashSet;

use accesskit::{Action, Node, NodeId, Rect as AkRect, Role, TextPosition, TextSelection, Toggled, TreeId, TreeInfo, TreeUpdate};

use crate::geom::Rect;
use crate::log;
use crate::ui::context::Context;
use crate::ui::edit;
use crate::ui::{Kind, MenuMark, OverlayKind, Tree, visible_text};

/// 入力欄の k 番目の文字の並びの番号（部品の番号から作る。部品の番号は 64 ビットのハッシュなので重ならない）
fn run_id(input: u64, k: usize) -> NodeId {
    NodeId(input ^ 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(k as u64 + 1))
}

fn ak(r: Rect) -> AkRect {
    AkRect { x0: r.x as f64, y0: r.y as f64, x1: r.right() as f64, y1: r.bottom() as f64 }
}

/// 読み上げの名前
pub(crate) fn name_of(tree: &Tree, i: usize) -> Option<String> {
    let n = &tree.info[i];
    if let Some(l) = &n.label {
        return Some(l.clone());
    }
    match &n.kind {
        Kind::TextInput { hint, .. } | Kind::TextArea { hint, .. } if !hint.is_empty() => Some(hint.clone()),
        Kind::Item { .. } => Some(row_name(tree, i)).filter(|s| !s.is_empty()),
        k => visible_text(k).filter(|t| !t.is_empty()).map(str::to_string),
    }
}

/// 引っ張っている物の影に書く文字: 読み上げの名前（無ければ中の文字をつないだ物）
pub(crate) fn drag_text(tree: &Tree, i: usize) -> String {
    name_of(tree, i).filter(|s| !s.is_empty()).unwrap_or_else(|| row_name(tree, i))
}

/// 行の中の文字をつないだ名前（一覧の行の読み上げに使う）
fn row_name(tree: &Tree, i: usize) -> String {
    let mut parts = Vec::new();
    let mut stack = vec![i];
    while let Some(k) = stack.pop() {
        match &tree.info[k].kind {
            Kind::Label { text, .. } | Kind::Checkbox { text, .. } if !text.is_empty() => parts.push(text.clone()),
            // 書式付きの文字は、つないだ文字列を 1 つにする（部分ごとに空白を入れない）
            Kind::RichText { text, .. } if !text.is_empty() => {
                parts.push(text.clone());
                continue;
            }
            _ => {}
        }
        stack.extend(tree.layout[k].children.iter().rev());
    }
    parts.join(" ")
}

/// 読み上げの木に出さない物（空き・数の入力欄の小さいボタン。欄が増やす・減らす操作を持つ）
fn hidden(kind: &Kind) -> bool {
    matches!(kind, Kind::Space | Kind::SpinButton { .. } | Kind::ColumnGrip)
}

pub(crate) struct A11yBuild {
    pub update: TreeUpdate,
    /// 名前の無い操作の部品（記録用）
    pub unlabeled: usize,
}

pub(crate) fn build(tree: &Tree, cx: &Context, title: &str, warned: &mut HashSet<u64>) -> A11yBuild {
    let mut nodes = Vec::with_capacity(tree.info.len());
    let mut unlabeled = 0;
    let root_id = NodeId(tree.info[0].id.0);
    let mut in_tree = HashSet::new();
    // 親の番号: 一覧の行の「何件目」を決めるため
    let mut list_count = vec![None; tree.info.len()];
    // 表の行（見出しの行は 0、ほかは 1 から）と、行の中の升の列
    let mut table_row = vec![None; tree.info.len()];
    let mut table_cell = vec![None; tree.info.len()];
    for i in 0..tree.info.len() {
        if let Kind::List { count, table, .. } = tree.info[i].kind {
            for &c in &tree.layout[i].children {
                list_count[c] = Some(count);
                if table {
                    table_row[c] = Some(tree.layout[c].row_index + 1);
                    for (k, &cell) in tree.layout[c].children.iter().enumerate() {
                        table_cell[cell] = Some(k);
                    }
                }
            }
        }
    }
    for i in 0..tree.info.len() {
        let info = &tree.info[i];
        let l = &tree.layout[i];
        if hidden(&info.kind) {
            continue;
        }
        let name = name_of(tree, i);
        let mut node = match &info.kind {
            // 表の行と升（一覧の行と、その中の入れ物）
            Kind::Row | Kind::Item { .. } if table_row[i].is_some() => {
                let mut n = Node::new(Role::Row);
                n.set_row_index(table_row[i].unwrap_or(0));
                if let Kind::Item { selected, .. } = &info.kind {
                    n.set_selected(*selected);
                    n.add_action(Action::Click);
                    n.add_action(Action::Focus);
                }
                n
            }
            _ if table_cell[i].is_some() => {
                let mut n = Node::new(Role::Cell);
                n.set_column_index(table_cell[i].unwrap_or(0));
                n
            }
            Kind::Root => {
                let mut n = Node::new(Role::Window);
                n.set_label(title);
                n
            }
            Kind::Row | Kind::Column | Kind::Stack => {
                if let Some(count) = list_count[i] {
                    let mut n = Node::new(Role::ListItem);
                    n.set_label(row_name(tree, i));
                    n.set_size_of_set(count);
                    n.set_position_in_set(l.row_index + 1);
                    n
                } else if tree.layout[i].children.iter().any(|&c| matches!(tree.info[c].kind, Kind::Segment { .. } | Kind::Radio { .. })) {
                    Node::new(Role::RadioGroup)
                } else {
                    Node::new(Role::GenericContainer)
                }
            }
            Kind::Label { text, size, .. } => {
                if *size == crate::ui::TextKind::Heading {
                    let mut n = Node::new(Role::Heading);
                    n.set_label(text.clone());
                    n.set_level(1);
                    n
                } else {
                    let mut n = Node::new(Role::Label);
                    n.set_value(text.clone());
                    n
                }
            }
            // 書式付きの文字は段落。中の部分を順に読み、リンクはリンクとして伝える
            Kind::RichText { .. } => Node::new(Role::Paragraph),
            Kind::RichSpan { link: true, .. } => {
                let mut n = Node::new(Role::Link);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::RichSpan { text, .. } => {
                let mut n = Node::new(Role::Label);
                n.set_value(text.clone());
                n
            }
            Kind::Button { .. } => {
                let mut n = Node::new(Role::Button);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Checkbox { checked, mixed, .. } => {
                let mut n = Node::new(Role::CheckBox);
                n.set_toggled(if *mixed {
                    Toggled::Mixed
                } else if *checked {
                    Toggled::True
                } else {
                    Toggled::False
                });
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Toggle { on, .. } => {
                let mut n = Node::new(Role::Switch);
                n.set_toggled(if *on { Toggled::True } else { Toggled::False });
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Segment { selected, .. } => {
                let mut n = Node::new(Role::RadioButton);
                n.set_toggled(if *selected { Toggled::True } else { Toggled::False });
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::TextInput { value, hint, password, .. } => {
                // パスワードは値を出さない
                let mut n = Node::new(if *password { Role::PasswordInput } else { Role::TextInput });
                if !*password {
                    n.set_value(value.clone());
                }
                if !hint.is_empty() {
                    n.set_placeholder(hint.clone());
                }
                n.add_action(Action::Focus);
                n
            }
            Kind::TextArea { value, hint, .. } => {
                let mut n = Node::new(Role::MultilineTextInput);
                n.set_value(value.clone());
                if !hint.is_empty() {
                    n.set_placeholder(hint.clone());
                }
                n.add_action(Action::Focus);
                n
            }
            Kind::List { table: true, .. } => Node::new(Role::RowGroup),
            Kind::List { count, .. } => {
                let mut n = Node::new(Role::List);
                n.set_size_of_set(*count);
                n
            }
            Kind::Table { columns, rows } => {
                let mut n = Node::new(Role::Table);
                n.set_row_count(rows + 1);
                n.set_column_count(*columns);
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
            Kind::TableHeader => {
                let mut n = Node::new(Role::Row);
                n.set_row_index(0);
                n
            }
            Kind::ColumnHeader { sort, sortable, column, .. } => {
                let mut n = Node::new(Role::ColumnHeader);
                n.set_column_index(*column);
                n.set_row_index(0);
                if let Some(asc) = sort {
                    n.set_sort_direction(if *asc { accesskit::SortDirection::Ascending } else { accesskit::SortDirection::Descending });
                }
                if *sortable {
                    n.add_action(Action::Click);
                    n.add_action(Action::Focus);
                }
                n
            }
            Kind::ColumnGrip => unreachable!(),
            Kind::DateField { text, open, .. } => {
                let mut n = Node::new(Role::DateInput);
                n.set_value(text.clone());
                n.set_has_popup(accesskit::HasPopup::Dialog);
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::DayGrid => {
                let mut n = Node::new(Role::Grid);
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
            Kind::TimeField { text, open, .. } => {
                let mut n = Node::new(Role::TimeInput);
                n.set_value(text.clone());
                n.set_has_popup(accesskit::HasPopup::Dialog);
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::ColorField { text, open, .. } => {
                let mut n = Node::new(Role::ColorWell);
                n.set_value(text.clone());
                n.set_has_popup(accesskit::HasPopup::Dialog);
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::ColorArea { sat, val, .. } => {
                let w = &cx.color_text;
                let mut n = Node::new(Role::Slider);
                n.set_value(format!("{} {:.0}%, {} {:.0}%", w.saturation, sat * 100.0, w.brightness, val * 100.0));
                n.add_action(Action::Focus);
                n
            }
            Kind::HueBar { hue } => {
                let mut n = Node::new(Role::Slider);
                n.set_numeric_value(hue.round() as f64);
                n.set_min_numeric_value(0.0);
                n.set_max_numeric_value(360.0);
                n.set_value(format!("{:.0}°", hue));
                n.add_action(Action::Focus);
                n
            }
            Kind::AlphaBar { alpha, .. } => {
                let mut n = Node::new(Role::Slider);
                n.set_numeric_value((alpha * 100.0).round() as f64);
                n.set_min_numeric_value(0.0);
                n.set_max_numeric_value(100.0);
                n.set_value(format!("{:.0}%", alpha * 100.0));
                n.add_action(Action::Focus);
                n
            }
            Kind::Swatch { selected, .. } => {
                let mut n = Node::new(Role::Button);
                n.set_selected(*selected);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::ClockCell { selected, .. } => {
                let mut n = Node::new(Role::GridCell);
                n.set_selected(*selected);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::CalendarDay { selected, .. } => {
                let mut n = Node::new(Role::GridCell);
                n.set_selected(*selected);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Scroll => Node::new(Role::ScrollView),
            Kind::Item { selected, .. } => {
                let mut n = Node::new(Role::ListItem);
                if let Some(count) = list_count[i] {
                    n.set_size_of_set(count);
                    n.set_position_in_set(l.row_index + 1);
                }
                n.set_selected(*selected);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Slider { value, min, max, step } => {
                let mut n = Node::new(Role::Slider);
                n.set_numeric_value(*value);
                n.set_min_numeric_value(*min);
                n.set_max_numeric_value(*max);
                n.set_numeric_value_step(*step);
                n.add_action(Action::Focus);
                n.add_action(Action::Increment);
                n.add_action(Action::Decrement);
                n
            }
            Kind::Radio { selected, .. } => {
                let mut n = Node::new(Role::RadioButton);
                n.set_toggled(if *selected { Toggled::True } else { Toggled::False });
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Dropdown { text, open, .. } => {
                let mut n = Node::new(Role::ComboBox);
                n.set_value(text.clone());
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::ListOption { selected, .. } => {
                let mut n = Node::new(Role::ListBoxOption);
                n.set_selected(*selected);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::MenuButton { open, .. } => {
                let mut n = Node::new(Role::Button);
                n.set_has_popup(accesskit::HasPopup::Menu);
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::MenuBar => Node::new(Role::MenuBar),
            Kind::MenuBarItem { open, .. } => {
                let mut n = Node::new(Role::MenuItem);
                n.set_has_popup(accesskit::HasPopup::Menu);
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::MenuItem { mark, shortcut, submenu, open, .. } => {
                let mut n = Node::new(match mark {
                    MenuMark::None => Role::MenuItem,
                    MenuMark::Check(_) => Role::MenuItemCheckBox,
                    MenuMark::Radio(_) => Role::MenuItemRadio,
                });
                if let MenuMark::Check(on) | MenuMark::Radio(on) = mark {
                    n.set_toggled(if *on { accesskit::Toggled::True } else { accesskit::Toggled::False });
                }
                if let Some(k) = shortcut {
                    n.set_keyboard_shortcut(k.clone());
                }
                if *submenu {
                    n.set_has_popup(accesskit::HasPopup::Menu);
                    n.set_expanded(*open);
                }
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Separator => {
                let mut n = Node::new(Role::GenericContainer);
                n.set_hidden();
                n
            }
            Kind::Progress { value, .. } => {
                let mut n = Node::new(Role::ProgressIndicator);
                if let Some(v) = value {
                    n.set_numeric_value((*v as f64 * 100.0).round());
                    n.set_min_numeric_value(0.0);
                    n.set_max_numeric_value(100.0);
                }
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
            Kind::Card { title } => match title {
                Some(t) => {
                    let mut n = Node::new(Role::Group);
                    n.set_label(t.clone());
                    n
                }
                None => Node::new(Role::GenericContainer),
            },
            Kind::Expander { open, .. } => {
                let mut n = Node::new(Role::Button);
                n.set_expanded(*open);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Link { .. } => {
                let mut n = Node::new(Role::Link);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::ToggleButton { on, .. } => {
                let mut n = Node::new(Role::Button);
                n.set_toggled(if *on { Toggled::True } else { Toggled::False });
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            // 名前の無いアイコンは飾りなので、読み上げない
            Kind::Icon { .. } | Kind::Image { .. } => {
                let mut n = Node::new(Role::Image);
                match &info.label {
                    Some(l) => n.set_label(l.clone()),
                    None => n.set_hidden(),
                }
                n
            }
            Kind::Overlay { kind, title } => match kind {
                OverlayKind::Popup => Node::new(Role::Pane),
                OverlayKind::Dialog => {
                    let mut n = Node::new(Role::Dialog);
                    n.set_modal();
                    n.set_label(title.clone());
                    n
                }
                OverlayKind::Tooltip => Node::new(Role::Tooltip),
                OverlayKind::ListBox => Node::new(Role::ListBox),
                OverlayKind::Menu => Node::new(Role::Menu),
            },
            Kind::Space | Kind::SpinButton { .. } => unreachable!(),
            Kind::Tabs => Node::new(Role::GenericContainer),
            Kind::TabList => Node::new(Role::TabList),
            Kind::Tab { selected, .. } => {
                let mut n = Node::new(Role::Tab);
                n.set_selected(*selected);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::TabPanel => {
                let mut n = Node::new(Role::TabPanel);
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
            Kind::NumberInput { text, value, min, max, step } => {
                let mut n = Node::new(Role::SpinButton);
                n.set_numeric_value(*value);
                n.set_min_numeric_value(*min);
                n.set_max_numeric_value(*max);
                n.set_numeric_value_step(*step);
                n.set_value(text.clone());
                n.add_action(Action::Focus);
                n.add_action(Action::Increment);
                n.add_action(Action::Decrement);
                n
            }
            Kind::Grid | Kind::Split { .. } => Node::new(Role::GenericContainer),
            Kind::Splitter { stacked, value, min, max } => {
                let mut n = Node::new(Role::Splitter);
                n.set_numeric_value(*value as f64);
                n.set_min_numeric_value(*min as f64);
                n.set_max_numeric_value(*max as f64);
                // 左右に分ける仕切りは縦の線
                n.set_orientation(if *stacked { accesskit::Orientation::Horizontal } else { accesskit::Orientation::Vertical });
                n.add_action(Action::Focus);
                n.add_action(Action::Increment);
                n.add_action(Action::Decrement);
                n
            }
            Kind::Tree => {
                let mut n = Node::new(Role::Tree);
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
            Kind::TreeItem { depth, open, selected, .. } => {
                let mut n = Node::new(Role::TreeItem);
                n.set_level(depth + 1);
                n.set_selected(*selected);
                if let Some(o) = open {
                    n.set_expanded(*o);
                }
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Canvas { .. } => {
                let mut n = Node::new(Role::Canvas);
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
            Kind::TitleBar { .. } => Node::new(Role::TitleBar),
            Kind::Toolbar => {
                let mut n = Node::new(Role::Toolbar);
                if let Some(l) = &info.label {
                    n.set_label(l.clone());
                }
                n
            }
        };
        if let Some(d) = &info.description {
            node.set_description(d.clone());
        }
        if info.kind.interactive() || matches!(info.kind, Kind::List { .. }) {
            match &name {
                Some(nm) => node.set_label(nm.clone()),
                None if info.kind.interactive() => {
                    unlabeled += 1;
                    if warned.insert(info.id.0) {
                        log::event("a11y.unlabeled", &format!("kind={:?}", std::mem::discriminant(&info.kind)), "add a label with .label(..)");
                    }
                }
                None => {}
            }
        }
        // 押せない部品: 読み上げソフトに「使用不可」と伝え、操作を出さない
        if info.disabled && info.kind.interactive() {
            node.set_disabled();
            node.clear_actions();
        }
        if !matches!(info.kind, Kind::Root) {
            node.set_bounds(ak(l.rect));
        }
        let mut children: Vec<NodeId> =
            l.children.iter().filter(|&&c| !hidden(&tree.info[c].kind)).map(|&c| NodeId(tree.info[c].id.0)).collect();
        // 重ねて出す物は親に付いていないので、根の子として読み上げの木に入れる
        if i == 0 {
            children.extend(tree.overlays.iter().map(|o| NodeId(o.id.0)));
        }
        // 入力欄の中身: 行ごとの文字の並び（TextRun）とキャレット。AT-SPI（Linux の Orca）は Text の窓口で中身と
        // キャレットを読み、AccessKit は文字の並びを持つ入力欄にだけその窓口を付ける。パスワードは中身を出さない
        let text = match &info.kind {
            Kind::TextInput { value, password: false, .. } | Kind::TextArea { value, .. } => Some(value),
            _ => None,
        };
        if let Some(value) = text {
            let runs = edit::text_runs(value);
            let ids: Vec<NodeId> = (0..runs.len()).map(|k| run_id(info.id.0, k)).collect();
            for (r, &id) in runs.iter().zip(&ids) {
                let mut n = Node::new(Role::TextRun);
                n.set_value(&value[r.start..r.end]);
                n.set_character_lengths(r.lengths.clone());
                if !r.words.is_empty() {
                    n.set_word_starts(r.words.clone());
                }
                n.set_bounds(ak(l.rect));
                nodes.push((id, n));
            }
            if let Some(e) = cx.edits.get(&info.id) {
                let at = |b: usize| {
                    let (k, j) = edit::run_position(&runs, b.min(value.len()));
                    TextPosition { node: ids[k], character_index: j }
                };
                node.set_text_selection(TextSelection { anchor: at(e.anchor), focus: at(e.cursor) });
            }
            children.extend(ids);
        }
        if !children.is_empty() {
            node.set_children(children);
        }
        in_tree.insert(info.id.0);
        nodes.push((NodeId(info.id.0), node));
    }
    let focus = cx.focus.filter(|f| in_tree.contains(&f.0)).map_or(root_id, |f| NodeId(f.0));
    let mut info = TreeInfo::new(root_id);
    info.toolkit_name = Some("kargui".into());
    info.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    A11yBuild { update: TreeUpdate { nodes, tree: Some(info), tree_id: TreeId::ROOT, focus }, unlabeled }
}
