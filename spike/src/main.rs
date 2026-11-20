//! kargui の段階 0 の試作。
//! タスク帳の形の窓を、Direct3D 11（--renderer d3d11、既定）か CPU（--renderer cpu か --software）で描く。
//! 引数と記録の形は前回の比べ（Tauri・egui・GPUI のタスク帳）と同じなので、同じ測定の道具で測れる。
#![windows_subsystem = "windows"]

mod app;
mod cpu;
mod draw;
mod gpu;
mod raster;
mod text;
mod win;

use std::time::Instant;

use taskcore::{args, log};

/// 記録の既定の置き場所（このリポジトリの logs/spike.log）
const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../logs/spike.log");

fn main() {
    let main_started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    let kind = win::RendererKind::from_args(&a);
    log::event(
        "startup",
        &format!(
            "items={} bench={:?} renderer={} full_rate={} no_focus={} pos={:?} run_id={:?}",
            a.items,
            a.bench_scroll,
            kind.name(),
            a.full_rate,
            a.no_focus,
            a.pos,
            a.run_id
        ),
        &format!("log={path:?}"),
    );
    if let Err(e) = win::run(a, kind, main_started) {
        log::event("error", "run", &format!("{e:?}"));
    }
    log::event("exit", "", "ok");
}
