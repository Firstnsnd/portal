//! scroll_up / scroll_down 滚动基准。
//! 暴露每行滚动的 O(rows) memmove 与每次新行堆分配。

use criterion::{criterion_group, criterion_main, Criterion};
use portal::terminal::TerminalGrid;

fn bench_scroll(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_scroll");
    group.bench_function("scroll_up_200x50_100lines", |b| {
        b.iter(|| {
            let mut grid = TerminalGrid::with_scrollback_limit(200, 50, 100 * 1024 * 1024);
            for _ in 0..100 {
                grid.scroll_up(0, 49);
            }
            criterion::black_box(&grid);
        });
    });
    group.bench_function("scroll_down_200x50_100lines", |b| {
        b.iter(|| {
            let mut grid = TerminalGrid::with_scrollback_limit(200, 50, 100 * 1024 * 1024);
            for _ in 0..100 {
                grid.scroll_down(0, 49);
            }
            criterion::black_box(&grid);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_scroll);
criterion_main!(benches);
