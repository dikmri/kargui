// タスク帳（kargui 版）。データと絞り込みは taskcore、画面は kargui の即時モードで書く。
// 前回の比べ（Tauri・egui・GPUI のタスク帳）と同じ機能・同じ引数・同じ形の記録なので、同じ測定の道具で測れる。
#![windows_subsystem = "windows"]

use std::time::Instant;

use kargui::{App, Frame, Options, Renderer, Size, Ui};
use taskcore::bench::{ScrollBench, Step};
use taskcore::{Counts, ROW_HEIGHT, Status, Store, WINDOW_SIZE, args, group, log};

/// 記録の既定の置き場所（このリポジトリの logs/taskbook.log）
const DEFAULT_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../logs/taskbook.log");

struct TaskBook {
    store: Store,
    input: String,
    query: String,
    status: Status,
    dark: bool,
    /// 絞り込みの結果（store.tasks() の添字）
    visible: Vec<usize>,
    counts: Counts,
    message: String,
    renderer: &'static str,
    main_started: Instant,
    ready_at: Option<Instant>,
    bench: Option<ScrollBench>,
    scroll_to: Option<f32>,
}

impl TaskBook {
    /// 絞り込みを計算し直す。語・状態・データが変わったときだけ呼ぶ。
    fn refilter(&mut self, why: &str) {
        let t = Instant::now();
        self.visible = self.store.filter(&self.query, self.status);
        self.counts = self.store.counts();
        log::event(
            "filter",
            &format!("why={why} q={} status={}", self.query, self.status.key()),
            &format!("hits={} {}", self.visible.len(), ms_since(t)),
        );
    }

    fn add_task(&mut self) {
        match self.store.add(&self.input) {
            Some(id) => {
                log::event("add", &format!("text={}", self.input.trim()), &format!("id={id}"));
                self.message = format!("「{}」を足しました", self.input.trim());
                self.input.clear();
                self.refilter("add");
            }
            None => log::event("add", "text=", "skipped=empty"),
        }
    }
}

impl App for TaskBook {
    fn view(&mut self, ui: &mut Ui) {
        ui.set_dark(self.dark);
        ui.row(|ui| {
            ui.heading("タスク帳");
            ui.label(format!("kargui ・ {}", self.renderer)).weak();
            ui.fill();
            if ui.toggle(&mut self.dark, "ダーク").changed() {
                log::event("theme", &format!("dark={}", self.dark), "ok");
            }
        });
        ui.row(|ui| {
            let submitted = ui.text_input(&mut self.input).hint("新しいタスクを入力して Enter").grow().submitted();
            if ui.button("追加").primary().clicked() || submitted {
                self.add_task();
            }
        });
        ui.row(|ui| {
            let query = ui.text_input(&mut self.query).hint("絞り込み（例: 資料）").width(Size::Fixed(240.0));
            let options = Status::LIST.map(|s| (s, s.label()));
            if query.changed() | ui.segmented(&mut self.status, &options) {
                self.refilter("query/status");
            }
            ui.label(format!("{} 件中 {} 件を表示", group(self.counts.total), group(self.visible.len()))).weak();
        });

        let (store, visible) = (&self.store, &self.visible);
        let (mut toggled, mut removed) = (None, None);
        let mut list = ui.list(visible.len(), ROW_HEIGHT).grow();
        if let Some(y) = self.scroll_to {
            list = list.scroll_to(y);
        }
        list.show(|ui, row| {
            let task = &store.tasks()[visible[row]];
            let mut done = task.done;
            if ui.checkbox(&mut done, "").label(&task.text).changed() {
                toggled = Some(task.id);
            }
            ui.label(&task.text).strike(task.done).grow();
            if ui.button("削除").small().label(format!("削除: {}", task.text)).clicked() {
                removed = Some(task.id);
            }
        });
        if let Some(id) = toggled {
            log::event("toggle", &format!("id={id}"), &format!("done={:?}", self.store.toggle(id)));
            self.refilter("toggle");
        }
        if let Some(id) = removed {
            log::event("remove", &format!("id={id}"), &format!("ok={}", self.store.remove(id)));
            self.message = "1 件を消しました".into();
            self.refilter("remove");
        }

        ui.row(|ui| {
            if ui.button("1万件を作る").clicked() {
                let t = Instant::now();
                self.store.generate(10_000);
                log::event("generate", "n=10000", &ms_since(t));
                self.message = "1万件を作りました".into();
                self.refilter("generate");
            }
            if ui.button("完了を消す").clicked() {
                let n = self.store.clear_done();
                log::event("clear_done", "", &format!("removed={n}"));
                self.message = format!("完了 {} 件を消しました", group(n));
                self.refilter("clear_done");
            }
            if ui.button("すべて消す").clicked() {
                let n = self.store.clear();
                log::event("clear", "", &format!("removed={n}"));
                self.message = format!("{} 件を消しました", group(n));
                self.refilter("clear");
            }
            ui.label(format!("未完了 {} ・ 完了 {}", group(self.counts.active), group(self.counts.done))).weak();
            ui.fill();
            ui.label(&self.message).weak();
        });

        // 測定のスクロールの間は、続けて描く
        if self.bench.as_ref().is_some_and(|b| !b.is_done()) {
            ui.request_frame();
        }
    }

    /// 画面に出たあとに呼ばれる。1 枚目で「表示できた」を記録し、測定のスクロールを進める
    fn presented(&mut self, _frame: &Frame) {
        let ready = *self.ready_at.get_or_insert_with(|| {
            log::event(
                "ready",
                &format!("renderer={} items={}", self.renderer, self.counts.total),
                &format!("since_main_ms={}", self.main_started.elapsed().as_millis()),
            );
            Instant::now()
        });
        let Some(bench) = self.bench.as_mut() else { return };
        match bench.tick(ready.elapsed().as_secs_f32()) {
            Step::Scroll { offset, first } => {
                if first {
                    log::event("bench.scroll.start", "", "ok");
                }
                self.scroll_to = Some(offset);
            }
            Step::Finished { frames, secs, max_gap_ms } => log::event(
                "bench.scroll.done",
                &format!("secs={secs:.3}"),
                &format!("frames={frames} fps={:.1} max_gap_ms={max_gap_ms:.1}", frames as f32 / secs),
            ),
            Step::Wait | Step::Idle => {}
        }
    }
}

fn ms_since(t: Instant) -> String {
    format!("ms={:.2}", t.elapsed().as_secs_f64() * 1000.0)
}

fn main() {
    let main_started = Instant::now();
    let a = args::from_env();
    let path = log::init("kargui", Some(a.log_path.as_deref().unwrap_or(DEFAULT_LOG)));
    kargui::set_logger(log::event);
    let gpu = !a.software && matches!(a.renderer.as_deref(), Some("d3d11" | "gpu"));
    let renderer = if gpu { "d3d11" } else { "cpu" };
    log::event(
        "startup",
        &format!("items={} bench={:?} renderer={renderer} full_rate={} run_id={:?}", a.items, a.bench_scroll, a.full_rate, a.run_id),
        &format!("log={path:?}"),
    );
    let mut store = Store::new();
    if a.items > 0 {
        let t = Instant::now();
        store.generate(a.items);
        log::event("generate", &format!("n={}", a.items), &ms_since(t));
    }
    let mut app = TaskBook {
        store,
        input: String::new(),
        query: String::new(),
        status: Status::All,
        dark: false,
        visible: Vec::new(),
        counts: Counts::default(),
        message: "準備できました".into(),
        renderer,
        main_started,
        ready_at: None,
        bench: a.bench_scroll.map(ScrollBench::new),
        scroll_to: None,
    };
    app.refilter("startup");
    let mut options = Options::new("タスク帳（kargui）");
    options.size = WINDOW_SIZE;
    options.position = a.pos;
    options.activate = !a.no_focus;
    options.renderer = if gpu { Renderer::Gpu } else { Renderer::Cpu };
    options.full_rate = a.full_rate;
    options.dark = Some(false);
    if let Err(e) = kargui::run(options, app) {
        log::event("error", "run", &e.to_string());
    }
    log::event("exit", "", "ok");
}
