//! 段階 0: 文字の並べ方を parley と DirectWrite で比べる。
//!
//! 使い方: text-probe parley|dwrite [行数]
//! 1 行目に数字（初期化・最初の 1 行・残りの 1 行あたり・メモリの増え方）、
//! 続けて日本語の折り返しの結果（幅 200 画素で折った各行）を書く。

use std::time::Instant;

use parley::{FontContext, FontFamily, LayoutContext, StyleProperty};
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::w;

const FONT: &str = "Yu Gothic UI";
const SIZE: f32 = 14.0;
const WRAP_WIDTH: f32 = 200.0;
const WRAP_TEXT: &str = "これは日本語の折り返しを確かめるための文です。「かぎかっこ」や、句読点（。、）が行の頭に来ないかを見ます。Rust と GUI の英単語も混ぜる。";

/// このプロセスだけが使っているメモリ（MB）
fn private_mb() -> f64 {
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut c as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        );
    }
    c.PrivateUsage as f64 / 1_048_576.0
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn texts(n: usize) -> Vec<String> {
    (0..n).map(taskcore::sample_text).collect()
}

fn run_parley(lines: &[String]) -> (f64, f64, f64, Vec<String>) {
    let t = Instant::now();
    let mut fcx = FontContext::new();
    let mut lcx: LayoutContext<()> = LayoutContext::new();
    let init = ms(t);
    let mut width_sum = 0.0f32;
    let mut layout_one = |text: &str, max: Option<f32>| {
        let mut b = lcx.ranged_builder(&mut fcx, text, 1.0, true);
        b.push_default(StyleProperty::FontSize(SIZE));
        b.push_default(StyleProperty::FontFamily(FontFamily::named(FONT)));
        let mut layout = b.build(text);
        layout.break_all_lines(max);
        layout
    };
    let t = Instant::now();
    width_sum += layout_one(&lines[0], None).width();
    let first = ms(t);
    let t = Instant::now();
    for s in &lines[1..] {
        width_sum += layout_one(s, None).width();
    }
    let per = ms(t) * 1000.0 / (lines.len() - 1) as f64;
    let wrapped = layout_one(WRAP_TEXT, Some(WRAP_WIDTH));
    let parts = wrapped.lines().map(|l| WRAP_TEXT[l.text_range()].to_string()).collect();
    std::hint::black_box(width_sum);
    (init, first, per, parts)
}

fn run_dwrite(lines: &[String]) -> (f64, f64, f64, Vec<String>) {
    unsafe {
        let t = Instant::now();
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
        let format = factory
            .CreateTextFormat(
                w!("Yu Gothic UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                SIZE,
                w!("ja-jp"),
            )
            .unwrap();
        let init = ms(t);
        let mut width_sum = 0.0f32;
        let layout_one = |text: &str, max: f32| {
            let wide: Vec<u16> = text.encode_utf16().collect();
            let layout = factory.CreateTextLayout(&wide, &format, max, 10_000.0).unwrap();
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m).unwrap();
            (layout, m.width, wide)
        };
        let t = Instant::now();
        width_sum += layout_one(&lines[0], 100_000.0).1;
        let first = ms(t);
        let t = Instant::now();
        for s in &lines[1..] {
            width_sum += layout_one(s, 100_000.0).1;
        }
        let per = ms(t) * 1000.0 / (lines.len() - 1) as f64;
        let (layout, _, wide) = layout_one(WRAP_TEXT, WRAP_WIDTH);
        let mut count = 0u32;
        let _ = layout.GetLineMetrics(None, &mut count);
        let mut metrics = vec![DWRITE_LINE_METRICS::default(); count as usize];
        layout.GetLineMetrics(Some(&mut metrics), &mut count).unwrap();
        let mut parts = Vec::new();
        let mut at = 0usize;
        for m in &metrics {
            let end = at + m.length as usize;
            parts.push(String::from_utf16_lossy(&wide[at..end]));
            at = end;
        }
        std::hint::black_box(width_sum);
        (init, first, per, parts)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let engine = args.get(1).map(String::as_str).unwrap_or("parley");
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let lines = texts(n.max(2));
    let mem0 = private_mb();
    let t = Instant::now();
    let (init, first, per, parts) = match engine {
        "dwrite" => run_dwrite(&lines),
        _ => run_parley(&lines),
    };
    let total = ms(t);
    let mem1 = private_mb();
    println!(
        "engine={engine} lines={} init_ms={init:.1} first_line_ms={first:.2} per_line_us={per:.1} total_ms={total:.1} private_mb_before={mem0:.1} after={mem1:.1} grew={:.1}",
        lines.len(),
        mem1 - mem0
    );
    for (i, p) in parts.iter().enumerate() {
        println!("wrap[{i}] {p}");
    }
}
