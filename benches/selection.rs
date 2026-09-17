//! 选中文本提取基准。
//! 复现 selection 提取的 O(scrollback×cols) 遍历成本。

use criterion::{criterion_group, criterion_main, Criterion};
use portal::terminal::{TerminalCell, TerminalGrid};

fn fill_scrollback(grid: &mut TerminalGrid, rows: usize) {
    for _ in 0..rows {
        grid.scrollback.push_back(vec![TerminalCell::default(); grid.cols]);
        grid.scrollback_wrapped.push_back(false);
    }
}

fn bench_extract(c: &mut Criterion) {
    let mut group = c.benchmark_group("selection_extract");
    group.bench_function("extract_30000_rows", |b| {
        let mut grid = TerminalGrid::with_scrollback_limit(200, 50, 100 * 1024 * 1024);
        fill_scrollback(&mut grid, 30_000);
        b.iter(|| {
            let mut text = String::with_capacity(30_000 * (grid.cols + 1));
            for row in grid.scrollback.iter().chain(grid.cells.iter()) {
                for cell in row {
                    if !cell.wide_continuation {
                        text.push(cell.c);
                    }
                }
                text.push('\n');
            }
            criterion::black_box(text);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_extract);
criterion_main!(benches);
