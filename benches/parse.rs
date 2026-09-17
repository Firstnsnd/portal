//! VT/ANSI 解析吞吐基准。
//! 喂代表性终端字节流（LF 滚动 + SGR 颜色 + CSI 光标/擦除），测解析速度。

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use portal::terminal::{CellAttrs, TerminalGrid, VteHandler};

/// 生成代表性终端输出：LF 滚动、SGR 颜色、CSI 序列混合。
fn sample_bytes(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut i = 0usize;
    while out.len() < size {
        match i % 4 {
            0 => out.extend_from_slice(b"0123456789 abcdefghij 0123456789\n"),
            1 => out.extend_from_slice(b"\x1b[31mred\x1b[0m\x1b[1mbold\x1b[0m "),
            2 => out.extend_from_slice(b"\x1b[2K\x1b[1G\x1b[38;5;100m "),
            _ => out.extend_from_slice(b"padding-padding-padding "),
        }
        i += 1;
    }
    out.truncate(size);
    out
}

fn bench_parse(c: &mut Criterion) {
    let data = sample_bytes(1 << 20); // 1 MiB
    let mut group = c.benchmark_group("vte_parse");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("parse_1MiB_mixed", |b| {
        b.iter(|| {
            let mut grid = TerminalGrid::with_scrollback_limit(80, 24, 100 * 1024 * 1024);
            let mut attrs = CellAttrs::default();
            let mut handler = VteHandler { grid: &mut grid, attrs: &mut attrs };
            let mut parser = vte::Parser::new();
            for &byte in &data {
                parser.advance(&mut handler, byte);
            }
            criterion::black_box(grid);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
