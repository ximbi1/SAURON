use sauron::{
    app::state::State, config::Settings, filters::Expr, kube::watch::Query, resources::Object,
};
use serde_json::json;
use std::{hint::black_box, time::Instant};

fn main() {
    println!(
        "profile={} samples=5; elapsed wall time; synthetic objects, no Kubernetes I/O",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!("objects,build_ms,filter_sort_median_ms,render_median_ms,estimated_json_bytes");
    for count in [100, 1000, 5000] {
        let mut state = State::new(
            Query {
                resource: "pods".into(),
                namespace: Some("benchmark".into()),
                ..Default::default()
            },
            &Settings::default(),
        );
        let start = Instant::now();
        for i in 0..count {
            state.store.apply(Object::new(json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":format!("api-{i:05}"),"namespace":"benchmark","uid":format!("uid-{i}"),"resourceVersion":"1","creationTimestamp":"2026-09-15T10:00:00Z"},"spec":{"containers":[{"name":"worker"}]},"status":{"phase":"Running","conditions":[{"type":"Ready","status":"True"}],"containerStatuses":[{"name":"worker","ready":true,"restartCount":i%10,"state":{"running":{}}}]}})),false);
        }
        let build = start.elapsed();
        state.filter = Expr::parse("restarts>=3 && !/999/").expect("filter");
        state.sort = "RESTARTS".into();
        let mut filters = Vec::new();
        let mut renders = Vec::new();
        let backend = ratatui::backend::TestBackend::new(120, 35);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        for _ in 0..5 {
            let start = Instant::now();
            state.rebuild();
            black_box(&state.rows);
            filters.push(start.elapsed().as_secs_f64() * 1000.0);
            let start = Instant::now();
            terminal
                .draw(|frame| sauron::ui::render(frame, &mut state, &[]))
                .expect("render");
            renders.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        filters.sort_by(f64::total_cmp);
        renders.sort_by(f64::total_cmp);
        println!(
            "{count},{:.3},{:.3},{:.3},{}",
            build.as_secs_f64() * 1000.0,
            filters[2],
            renders[2],
            state.store.bytes()
        );
    }
}
