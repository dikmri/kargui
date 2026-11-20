//! 読み上げの木（AccessKit）を画面の木から作る。
//!
//! 部品の種類から役割を決め、名前は部品の文字（Handle::label で上書きできる）から取る。
//! 名前の無い操作の部品は記録で警告する（段階 0 の比べで、egui と GPUI は警告しないまま名前の無い部品が 15〜18 個あった）。
//! 一覧は見えている行だけを入れ、行ごとに「何件中何件目」を持たせる。

use std::collections::HashSet;

use accesskit::{Action, Node, NodeId, Rect as AkRect, Role, Toggled, TreeId, TreeInfo, TreeUpdate};

use crate::geom::Rect;
use crate::log;
use crate::ui::context::Context;
use crate::ui::{Kind, Tree, visible_text};

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
        Kind::TextInput { hint, .. } if !hint.is_empty() => Some(hint.clone()),
        k => visible_text(k).filter(|t| !t.is_empty()).map(str::to_string),
    }
}

/// 行の中の文字をつないだ名前（一覧の行の読み上げに使う）
fn row_name(tree: &Tree, i: usize) -> String {
    let mut parts = Vec::new();
    let mut stack = vec![i];
    while let Some(k) = stack.pop() {
        match &tree.info[k].kind {
            Kind::Label { text, .. } | Kind::Checkbox { text, .. } if !text.is_empty() => parts.push(text.clone()),
            _ => {}
        }
        stack.extend(tree.layout[k].children.iter().rev());
    }
    parts.join(" ")
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
    for i in 0..tree.info.len() {
        if let Kind::List { count, .. } = tree.info[i].kind {
            for &c in &tree.layout[i].children {
                list_count[c] = Some(count);
            }
        }
    }
    for i in 0..tree.info.len() {
        let info = &tree.info[i];
        let l = &tree.layout[i];
        if matches!(info.kind, Kind::Space) {
            continue;
        }
        let name = name_of(tree, i);
        let mut node = match &info.kind {
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
                } else if tree.layout[i].children.iter().any(|&c| matches!(tree.info[c].kind, Kind::Segment { .. })) {
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
            Kind::Button { .. } => {
                let mut n = Node::new(Role::Button);
                n.add_action(Action::Click);
                n.add_action(Action::Focus);
                n
            }
            Kind::Checkbox { checked, .. } => {
                let mut n = Node::new(Role::CheckBox);
                n.set_toggled(if *checked { Toggled::True } else { Toggled::False });
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
            Kind::TextInput { value, hint } => {
                let mut n = Node::new(Role::TextInput);
                n.set_value(value.clone());
                if !hint.is_empty() {
                    n.set_placeholder(hint.clone());
                }
                n.add_action(Action::Focus);
                n
            }
            Kind::List { count, .. } => {
                let mut n = Node::new(Role::List);
                n.set_size_of_set(*count);
                n
            }
            Kind::Space => unreachable!(),
        };
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
        if !matches!(info.kind, Kind::Root) {
            node.set_bounds(ak(l.rect));
        }
        let children: Vec<NodeId> =
            l.children.iter().filter(|&&c| !matches!(tree.info[c].kind, Kind::Space)).map(|&c| NodeId(tree.info[c].id.0)).collect();
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
