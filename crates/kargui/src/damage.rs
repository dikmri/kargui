//! 変わった所だけ描く仕組み。前のフレームと今のフレームの描く物の一覧を比べ、
//! 「絵をずらす範囲」と「描き直す範囲」を決める（CPU の道で使う）。
//!
//! - 層の外（スクロールしない物）: 窓を TILE 画素のタイルに分け、タイルごとに描く物の要約（hash）を作って比べる
//! - スクロールする層: 中身の座標（スクロールの位置を戻した座標）で BAND 画素の帯ごとに要約を作る。
//!   スクロールの位置だけが変わったら絵をずらし、空いた帯と、中身が変わった帯だけを描き直す
//! - スクロールする層に重なる層の外の物（上に重なる物、一様でない背景）は、ずらすと崩れるので描き直す

use std::collections::HashMap;

use crate::geom::IRect;
use crate::paint::DrawList;

pub(crate) const TILE: i32 = 64;
pub(crate) const BAND: i32 = 32;

#[derive(Default, Clone, Debug)]
pub(crate) struct Summary {
    size: (i32, i32),
    tiles: HashMap<(i32, i32), u64>,
    scrolls: HashMap<u64, ScrollSummary>,
}

#[derive(Default, Clone, Debug)]
struct ScrollSummary {
    clip: IRect,
    offset: i32,
    bands: HashMap<i32, u64>,
    /// ずらすと崩れる、層の外の物の範囲（窓の座標）
    sticky: Vec<IRect>,
}

/// 絵をずらす範囲と量、描き直す範囲
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Plan {
    pub full: bool,
    /// (範囲, 上へずらす画素数)
    pub blits: Vec<(IRect, i32)>,
    pub damage: Vec<IRect>,
}

/// 描き直す範囲の合計がこの割合を超えたら、全体を描き直す
const FULL_RATIO: f32 = 0.6;

/// 描く物 1 つの要約（FNV-1a）。dy は y に足す量（スクロールする層では中身の座標にする）
fn hash_quad(q: &crate::paint::Quad, clip: Option<IRect>, dy: f32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: u32| {
        h ^= v as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    let [x, y, w, hh] = q.rect;
    for v in [x, y + dy, w, hh] {
        eat(v.to_bits());
    }
    for v in q.color.iter().chain(q.uv.iter()).chain(q.extra.iter()) {
        eat(v.to_bits());
    }
    if let Some(c) = clip {
        for v in [c.x, c.y + dy as i32, c.w, c.h] {
            eat(v as u32);
        }
    }
    h
}

/// 順番を区別して要約を重ねる（描く順が変われば見た目も変わるため）
fn combine(acc: &mut u64, h: u64) {
    *acc = (acc.rotate_left(7) ^ h).wrapping_mul(0x9e37_79b9_7f4a_7c15);
}

/// 一様な背景（不透明で角が丸くなく、範囲を覆う塗りつぶし）か。ずらしても崩れない
fn is_uniform_cover(q: &crate::paint::Quad, clip: IRect) -> bool {
    !q.is_glyph() && q.color[3] >= 1.0 && q.extra[0] <= 0.0 && {
        let r = IRect::covering(q.bounds());
        r.x <= clip.x && r.y <= clip.y && r.right() >= clip.right() && r.bottom() >= clip.bottom()
    }
}

pub(crate) fn summarize(list: &DrawList, size: (i32, i32)) -> Summary {
    let mut s = Summary { size, ..Default::default() };
    // スクロールする部品ごとに、最初の層（部品そのものの切り取り範囲）を覚える
    let mut first_layer: HashMap<u64, usize> = HashMap::new();
    for (li, layer) in list.layers.iter().enumerate() {
        let quads = &list.quads[layer.start..layer.end];
        match layer.scroll {
            None => {
                for q in quads {
                    let Some(r) = IRect::covering(q.bounds()).intersect(&layer.clip) else { continue };
                    let h = hash_quad(q, Some(layer.clip), 0.0);
                    for ty in r.y.div_euclid(TILE)..=(r.bottom() - 1).div_euclid(TILE) {
                        for tx in r.x.div_euclid(TILE)..=(r.right() - 1).div_euclid(TILE) {
                            combine(s.tiles.entry((tx, ty)).or_insert(0), h);
                        }
                    }
                }
            }
            Some(sc) => {
                first_layer.entry(sc.id).or_insert(li);
                let e = s.scrolls.entry(sc.id).or_insert_with(|| ScrollSummary {
                    clip: layer.clip,
                    offset: sc.offset,
                    ..Default::default()
                });
                // 部品そのものの範囲は窓に止まっているので要約に入れない。中の入れ子の範囲は中身と一緒に動くので入れる
                let nested = (layer.clip != e.clip).then_some(layer.clip);
                let dy = sc.offset as f32;
                for q in quads {
                    if IRect::covering(q.bounds()).intersect(&layer.clip).is_none() {
                        continue;
                    }
                    let content = IRect::covering(q.bounds().translate(0.0, dy));
                    if content.is_empty() {
                        continue;
                    }
                    let h = hash_quad(q, nested, dy);
                    for b in content.y.div_euclid(BAND)..=(content.bottom() - 1).div_euclid(BAND) {
                        combine(e.bands.entry(b).or_insert(0), h);
                    }
                }
            }
        }
    }
    // スクロールする層に重なる、層の外の物を集める
    for (id, &fi) in &first_layer {
        let clip = s.scrolls[id].clip;
        let mut sticky = Vec::new();
        for (li, layer) in list.layers.iter().enumerate().filter(|(_, l)| l.scroll.is_none()) {
            for q in &list.quads[layer.start..layer.end] {
                let Some(r) = IRect::covering(q.bounds()).intersect(&layer.clip).and_then(|r| r.intersect(&clip)) else {
                    continue;
                };
                if li < fi && is_uniform_cover(q, clip) {
                    continue;
                }
                sticky.push(r);
            }
        }
        s.scrolls.get_mut(id).unwrap().sticky = sticky;
    }
    s
}

pub(crate) fn plan(prev: &Summary, cur: &Summary) -> Plan {
    if prev.size != cur.size || prev.size == (0, 0) {
        return Plan { full: true, ..Default::default() };
    }
    let window = IRect::new(0, 0, cur.size.0, cur.size.1);
    let mut p = Plan::default();

    // 1) 層の外: 要約の違うタイルを、行ごとに横につないで描き直す
    let mut tiles: Vec<(i32, i32)> = prev
        .tiles
        .keys()
        .chain(cur.tiles.keys())
        .filter(|k| prev.tiles.get(k) != cur.tiles.get(k))
        .copied()
        .collect();
    tiles.sort_by_key(|&(x, y)| (y, x));
    tiles.dedup();
    let mut i = 0;
    while i < tiles.len() {
        let (x0, y) = tiles[i];
        let mut x1 = x0;
        while i + 1 < tiles.len() && tiles[i + 1] == (x1 + 1, y) {
            x1 += 1;
            i += 1;
        }
        if let Some(r) = IRect::new(x0 * TILE, y * TILE, (x1 - x0 + 1) * TILE, TILE).intersect(&window) {
            p.damage.push(r);
        }
        i += 1;
    }

    // 2) スクロールする層
    let mut ids: Vec<u64> = cur.scrolls.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let cs = &cur.scrolls[&id];
        match prev.scrolls.get(&id) {
            Some(ps) if ps.clip == cs.clip => {
                let clip = cs.clip;
                let dy = cs.offset - ps.offset;
                if dy.abs() >= clip.h {
                    p.damage.push(clip);
                    continue;
                }
                if dy != 0 {
                    p.blits.push((clip, dy));
                    let strip =
                        if dy > 0 { IRect::new(clip.x, clip.bottom() - dy, clip.w, dy) } else { IRect::new(clip.x, clip.y, clip.w, -dy) };
                    p.damage.push(strip);
                    for r in ps.sticky.iter().chain(cs.sticky.iter()) {
                        if !p.damage.contains(r) {
                            p.damage.push(*r);
                        }
                    }
                }
                // 前と今の両方で見えていた中身の範囲だけを、帯ごとに比べる（新しく見えた所は上の帯で描く）
                let s0 = (ps.clip.y + ps.offset).max(clip.y + cs.offset);
                let s1 = (ps.clip.bottom() + ps.offset).min(clip.bottom() + cs.offset);
                if s1 > s0 {
                    for b in s0.div_euclid(BAND)..=(s1 - 1).div_euclid(BAND) {
                        if ps.bands.get(&b) != cs.bands.get(&b) {
                            let r = IRect::new(clip.x, b * BAND - cs.offset, clip.w, BAND);
                            if let Some(r) = r.intersect(&clip) {
                                p.damage.push(r);
                            }
                        }
                    }
                }
            }
            Some(ps) => {
                p.damage.push(ps.clip);
                p.damage.push(cs.clip);
            }
            None => p.damage.push(cs.clip),
        }
    }
    for (id, ps) in &prev.scrolls {
        if !cur.scrolls.contains_key(id) {
            p.damage.push(ps.clip);
        }
    }

    // 3) 描き直す範囲が広すぎたら、全体を描き直す
    let area: i64 = p.damage.iter().map(|r| r.w as i64 * r.h as i64).sum();
    if area as f32 > window.w as f32 * window.h as f32 * FULL_RATIO {
        return Plan { full: true, ..Default::default() };
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Rect, rgb};
    use crate::paint::Scroll;

    const W: i32 = 256;
    const H: i32 = 256;

    /// 背景に灰色の四角、真ん中にスクロールする一覧（y=64〜192）、行は 32 画素ごと
    fn frame(offset: i32, changed_row: Option<i32>, overlay: bool) -> DrawList {
        let mut d = DrawList::default();
        d.begin(IRect::new(0, 0, W, H));
        d.fill(Rect::new(10.0, 10.0, 40.0, 20.0), rgb(0x888888), 0.0);
        let clip = IRect::new(0, 64, W, 128);
        d.push_clip(clip, Some(Scroll { id: 1, offset }));
        for row in 0..100 {
            let y = 64 + row * 32 - offset;
            if y + 32 <= 64 || y >= 192 {
                continue;
            }
            let color = if Some(row) == changed_row { rgb(0xff0000) } else { rgb(0x0000ff) };
            d.fill(Rect::new(8.0, y as f32 + 4.0, 100.0, 24.0), color, 0.0);
        }
        d.pop_clip();
        if overlay {
            d.fill(Rect::new(200.0, 100.0, 20.0, 20.0), rgb(0x00ff00), 0.0);
        }
        d.finish();
        d
    }

    fn sum(d: &DrawList) -> Summary {
        summarize(d, (W, H))
    }

    #[test]
    fn first_frame_and_resize_are_full() {
        let a = sum(&frame(0, None, false));
        assert!(plan(&Summary::default(), &a).full);
        let b = summarize(&frame(0, None, false), (W + 1, H));
        assert!(plan(&a, &b).full);
    }

    #[test]
    fn same_frame_needs_nothing() {
        let a = sum(&frame(40, None, true));
        let b = sum(&frame(40, None, true));
        assert_eq!(plan(&a, &b), Plan::default());
    }

    #[test]
    fn root_change_damages_only_its_tiles() {
        let a = sum(&frame(0, None, false));
        let mut d = frame(0, None, false);
        d.quads[0].color = rgb(0x111111);
        let b = sum(&d);
        let p = plan(&a, &b);
        assert!(!p.full && p.blits.is_empty());
        // 灰色の四角（10,10〜50,30）は左上のタイル 1 枚だけ
        assert_eq!(p.damage, vec![IRect::new(0, 0, TILE, TILE)]);
    }

    #[test]
    fn scroll_only_blits_and_redraws_the_strip() {
        let a = sum(&frame(0, None, false));
        let b = sum(&frame(20, None, false));
        let p = plan(&a, &b);
        assert!(!p.full);
        assert_eq!(p.blits, vec![(IRect::new(0, 64, W, 128), 20)]);
        assert_eq!(p.damage, vec![IRect::new(0, 172, W, 20)]);
    }

    #[test]
    fn scroll_down_by_negative_offset_blits_the_other_way() {
        let a = sum(&frame(100, None, false));
        let b = sum(&frame(90, None, false));
        let p = plan(&a, &b);
        assert_eq!(p.blits, vec![(IRect::new(0, 64, W, 128), -10)]);
        assert_eq!(p.damage, vec![IRect::new(0, 64, W, 10)]);
    }

    #[test]
    fn scroll_with_a_changed_row_also_redraws_that_band() {
        let a = sum(&frame(0, None, false));
        // 行 2（中身の y=128〜160）の色が変わり、同時に 20 画素スクロール
        let b = sum(&frame(20, Some(2), false));
        let p = plan(&a, &b);
        assert_eq!(p.blits, vec![(IRect::new(0, 64, W, 128), 20)]);
        // 空いた帯と、行 2 の帯（中身の y=64〜96 → 窓の y=108〜140）
        assert!(p.damage.contains(&IRect::new(0, 172, W, 20)), "{:?}", p.damage);
        assert!(p.damage.contains(&IRect::new(0, 108, W, 32)), "{:?}", p.damage);
        assert_eq!(p.damage.len(), 2);
    }

    #[test]
    fn overlay_on_a_scrolled_layer_is_redrawn() {
        let a = sum(&frame(0, None, true));
        let b = sum(&frame(20, None, true));
        let p = plan(&a, &b);
        assert_eq!(p.blits.len(), 1);
        assert!(p.damage.contains(&IRect::new(200, 100, 20, 20)), "{:?}", p.damage);
    }

    #[test]
    fn moved_scroll_clip_redraws_both_places() {
        let a = sum(&frame(0, None, false));
        let mut d = frame(0, None, false);
        for l in d.layers.iter_mut().filter(|l| l.scroll.is_some()) {
            l.clip = IRect::new(0, 70, W, 100);
        }
        let b = sum(&d);
        // この窓では、前と後の 2 か所で窓の 6 割を超えるので、全体を描き直す
        assert!(plan(&a, &b).full);
        // 窓が大きければ、前と後の 2 か所だけを描き直す
        let big = |d: &DrawList| summarize(d, (W * 4, H * 4));
        let p = plan(&big(&frame(0, None, false)), &big(&d));
        assert!(!p.full && p.blits.is_empty());
        assert!(p.damage.contains(&IRect::new(0, 64, W, 128)), "{:?}", p.damage);
        assert!(p.damage.contains(&IRect::new(0, 70, W, 100)), "{:?}", p.damage);
    }
}
