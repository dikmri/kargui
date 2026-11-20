//! 1 フレームを回す処理。窓（platform.rs）と試験台（test.rs）が同じ物を使う。
//!
//! 入力を振り分ける → ビューの 1 回目（入力を部品に渡す）→ ビューの 2 回目（描く）→ 配置 → 描く物の一覧 → 読み上げの木。
//! 入力が無いフレーム（窓の大きさが変わった、アニメーション）は 2 回目だけを回す。

use std::collections::HashSet;

use accesskit::TreeUpdate;

use crate::app::App;
use crate::input::Event;
use crate::paint::DrawList;
use crate::text::Text;
use crate::ui::context::{Clipboard, Context};
use crate::ui::{Id, Tree, Ui, a11y, draw};

/// 画面の木の根の番号（どのフレームでも同じ）
pub(crate) const ROOT: Id = Id(0x6b61_7267_7569); // "kargui"

pub(crate) struct Runtime {
    pub cx: Context,
    pub draw: DrawList,
    /// 最後に描いた画面の木（試験台が部品を名前で探すのに使う）
    pub tree: Tree,
    pub title: String,
    pub frames: u64,
    warned: HashSet<u64>,
}

pub(crate) struct FrameOut {
    /// もう 1 フレーム描いてほしい（アニメーション、一覧の高さのずれ）
    pub want_frame: bool,
    pub unlabeled: usize,
}

impl Runtime {
    pub fn new(text: Text, dark: bool, clipboard: Box<dyn Clipboard>, title: String) -> Self {
        Self {
            cx: Context::new(text, dark, clipboard),
            draw: DrawList::default(),
            tree: Tree::new(ROOT, 1.0),
            title,
            frames: 0,
            warned: HashSet::new(),
        }
    }

    /// 窓の中の大きさ（物理ピクセル）と拡大率
    pub fn resize(&mut self, width: f32, height: f32, scale: f32) {
        if (self.cx.scale - scale).abs() > f32::EPSILON {
            self.cx.text.reset();
        }
        self.cx.width = width;
        self.cx.height = height;
        self.cx.scale = scale;
    }

    pub fn frame(&mut self, app: &mut dyn App, events: Vec<Event>) -> FrameOut {
        let mut want = false;
        if !events.is_empty() {
            self.cx.handle(events);
            let mut tree = Tree::new(ROOT, self.cx.scale);
            app.view(&mut Ui::new(&mut self.cx, &mut tree));
            self.cx.end_input();
            want |= std::mem::take(&mut self.cx.want_frame);
        }
        let mut tree = Tree::new(ROOT, self.cx.scale);
        app.view(&mut Ui::new(&mut self.cx, &mut tree));
        want |= std::mem::take(&mut self.cx.want_frame);
        draw::layout(&mut tree, &mut self.cx);
        draw::paint(&tree, &mut self.cx, &mut self.draw);
        // 一覧の見える範囲を決めた高さがずれていたら（最初のフレーム、窓の大きさが変わった時）、
        // 決まった高さでこのフレームの中でもう一度回す。余分なフレームを画面に出さない
        if draw::list_needs_refresh(&tree) {
            tree = Tree::new(ROOT, self.cx.scale);
            app.view(&mut Ui::new(&mut self.cx, &mut tree));
            want |= std::mem::take(&mut self.cx.want_frame);
            draw::layout(&mut tree, &mut self.cx);
            draw::paint(&tree, &mut self.cx, &mut self.draw);
        }
        // 焦点のある部品が消えたら、焦点を外す
        if let Some(f) = self.cx.focus {
            if !tree.info.iter().any(|n| n.id == f) {
                self.cx.focus = None;
            }
        }
        self.tree = tree;
        self.frames += 1;
        FrameOut { want_frame: want, unlabeled: 0 }
    }

    pub fn a11y(&mut self) -> (TreeUpdate, usize) {
        let b = a11y::build(&self.tree, &self.cx, &self.title, &mut self.warned);
        (b.update, b.unlabeled)
    }

    /// 前のフレームで (x, y) にあった押せる部品（ポインターが乗っている部品）
    pub fn hover_at(&self, x: f32, y: f32) -> Option<Id> {
        self.cx.hit(x, y).map(|h| h.id)
    }
}
