//! CPU の道の 1 フレーム分の描き方。前のフレームの絵（px）を、今の描く物の一覧に合うように直す。
//! 変わった所だけ描く（damage.rs の計画に従って、絵をずらしてから描き直す）。
//! 窓（platform.rs）と、全部描き直した絵と比べるテスト（test.rs）が同じ物を使う。

use crate::damage::{self, Summary};
use crate::geom::{Color, IRect};
use crate::paint::DrawList;
use crate::raster::Canvas;
use crate::text::Atlas;

pub(crate) struct CpuFrame {
    /// 今のフレームの要約（次のフレームに渡す）
    pub summary: Summary,
    /// 画面に写し直す範囲
    pub present: Vec<IRect>,
    /// 記録用の説明
    pub mode: String,
}

/// px（前のフレームの絵）を今の一覧に合わせて描き直す。full なら全部描き直す
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    px: &mut [u32],
    width: i32,
    height: i32,
    list: &DrawList,
    atlas: &Atlas,
    background: Color,
    prev: &Summary,
    full: bool,
) -> CpuFrame {
    let summary = damage::summarize(list, (width, height));
    let plan = damage::plan(prev, &summary);
    let mut canvas = Canvas { px, width, height };
    let mut present = Vec::new();
    let mode;
    if full || plan.full {
        let all = IRect::new(0, 0, width, height);
        canvas.draw(list, &atlas.pixels, atlas.size, all, background);
        present.push(all);
        mode = "full".to_string();
    } else {
        for (r, dy) in &plan.blits {
            canvas.scroll_up(*r, *dy);
            present.push(*r);
        }
        for r in &plan.damage {
            canvas.draw(list, &atlas.pixels, atlas.size, *r, background);
            if !present.iter().any(|p| p.intersect(r) == Some(*r)) {
                present.push(*r);
            }
        }
        mode = format!("blits={} damage={}", plan.blits.len(), plan.damage.len());
    }
    CpuFrame { summary, present, mode }
}
