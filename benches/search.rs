//! 全历史搜索基准。
//! 暴露 O(scrollback×cols) 遍历 + 每行 String 拼接。

use criterion::{criterion_group, criterion_main, Criterion};
use portal::terminal::{TerminalCell, TerminalGrid};

fn bench_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_search");
    group.bench_function("search_30000_rows", |b| {
        let mut grid = TerminalGrid::with_scrollback_limit(200, 50, 100 * 1024 * 1024);
        for _ in 0..30_000 {
            grid.scrollback.push_back(vec![TerminalCell::default(); grid.cols]);
            grid.scrollback_wrapped.push_back(false);
        }
        b.iter(|| {
            criterion::black_box(grid.search("needle", false));
        });
    });
    group.finish();
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
