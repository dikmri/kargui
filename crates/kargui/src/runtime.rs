//! 1 フレームを回す処理。窓（platform.rs）と試験台（test.rs）が同じ物を使う。
//!
//! 入力を振り分ける → ビューの 1 回目（入力を部品に渡す）→ ビューの 2 回目（描く）→ 配置 → 描く物の一覧 → 読み上げの木。
//! 入力が無いフレーム（窓の大きさが変わった、アニメーション）は 2 回目だけを回す。
//! 入力があったフレームは、描く回の後にもう一度回し、画面の木が変わらなくなるまで（最大 SETTLE 回）回し直す。
//! アプリが描いた後で値を作り直す書き方（数の表示の後で絞り込みを作り直すなど）でも、古い値を画面に出さない。

use std::collections::HashSet;

use accesskit::TreeUpdate;

use crate::app::App;
use crate::input::Event;
use crate::paint::DrawList;
use crate::text::Text;
use crate::ui::context::{Clipboard, Context, OverlayInfo};
use crate::ui::{Id, OverlayKind, Tree, Ui, a11y, draw};

/// 画面の木の根の番号（どのフレームでも同じ）
pub(crate) const ROOT: Id = Id(0x6b61_7267_7569); // "kargui"

/// 入力があったフレームで、描く回の後に回し直す回数の上限（毎回変わる画面でも止まるように）
const SETTLE: usize = 2;

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
    /// この時間がたったら起こしてほしい（Ui::request_frame_after、ツールチップ）
    pub wake_after: Option<std::time::Duration>,

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
        self.cx.now = self.cx.clock();
        self.cx.wake_at = None;
        let mut want = false;
        let had_input = !events.is_empty();
        if had_input {
            self.cx.handle(events);
            let mut tree = Tree::new(ROOT, self.cx.scale);
            app.view(&mut Ui::new(&mut self.cx, &mut tree));
            self.cx.end_input();
            want |= std::mem::take(&mut self.cx.want_frame);
        }
        let mut tree = Tree::new(ROOT, self.cx.scale);
        app.view(&mut Ui::new(&mut self.cx, &mut tree));
        want |= std::mem::take(&mut self.cx.want_frame);
        if had_input {
            for _ in 0..SETTLE {
                let mut again = Tree::new(ROOT, self.cx.scale);
                app.view(&mut Ui::new(&mut self.cx, &mut again));
                want |= std::mem::take(&mut self.cx.want_frame);
                let settled = again.same_as(&tree);
                tree = again;
                if settled {
                    break;
                }
            }
        }
        // 前のフレームで重ねて出していた物（開いた・閉じたを見分けて焦点を動かす）
        let before = self.cx.prev.overlays.clone();
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
        self.overlay_focus(&before, &tree);
        // 焦点のある部品が消えたら、焦点を外す
        if let Some(f) = self.cx.focus {
            if !tree.info.iter().any(|n| n.id == f) {
                self.cx.focus = None;
            }
        }
        self.tree = tree;
        self.frames += 1;
        let wake_after = self.cx.wake_at.map(|t| t.saturating_duration_since(self.cx.now));
        FrameOut { want_frame: want, wake_after }
    }

    /// 重ねて出す物が開いたら、開く前の焦点を覚えて中の最初の部品へ焦点を移す。閉じたら、焦点が中にあった
    /// （か消えた）時に、開く前の焦点（無ければ出す元の部品）へ戻す。ツールチップは焦点に関わらない
    fn overlay_focus(&mut self, before: &[OverlayInfo], tree: &Tree) {
        let exists = |id: Id| tree.info.iter().any(|n| n.id == id);
        let now = self.cx.prev.overlays.clone();
        for o in now.iter().filter(|o| !before.iter().any(|p| p.id == o.id)) {
            crate::log::event("overlay.open", &format!("kind={:?}", o.kind), &format!("id={:x}", o.id.0));
        }
        for o in before.iter().filter(|o| !now.iter().any(|n| n.id == o.id)) {
            crate::log::event("overlay.close", &format!("kind={:?}", o.kind), &format!("id={:x}", o.id.0));
        }
        for o in before.iter().filter(|o| o.kind != OverlayKind::Tooltip) {
            if now.iter().any(|n| n.id == o.id) {
                continue;
            }
            let back = self.cx.overlay_return.remove(&o.id).or(o.anchor);
            let inside = self.cx.focus.is_none_or(|f| o.focus.contains(&f) || !exists(f));
            if let (true, Some(b)) = (inside, back.filter(|&b| exists(b))) {
                self.cx.focus = Some(b);
            }
        }
        for o in now.iter().filter(|o| o.kind != OverlayKind::Tooltip) {
            if before.iter().any(|p| p.id == o.id) {
                continue;
            }
            if let Some(f) = self.cx.focus {
                self.cx.overlay_return.insert(o.id, f);
            }
            let hint = self.cx.overlay_focus_hint.get(&o.id).copied().filter(|h| o.focus.contains(h));
            if let Some(f) = hint.or_else(|| o.focus.first().copied()) {
                self.cx.focus = Some(f);
            }
        }
    }

    pub fn a11y(&mut self) -> (TreeUpdate, usize) {
        let b = a11y::build(&self.tree, &self.cx, &self.title, &mut self.warned);
        (b.update, b.unlabeled)
    }

    /// 前のフレームで (x, y) にあった押せる部品（ポインターが乗っている部品）
    pub fn hover_at(&self, x: f32, y: f32) -> Option<Id> {
        self.cx.hit(x, y).filter(|h| h.enabled).map(|h| h.id)
    }
}
