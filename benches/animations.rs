use baijia_suo::animation::{AnimConfig, AnimationPlayer};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use std::hint::black_box;
use std::time::Instant;

const BG: (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 1.0);
const MODES: [&str; 5] = ["petri", "binaryring", "moire", "lightning", "piecewise"];

fn player(mode: &str, width: u32, height: u32) -> AnimationPlayer {
    let params = AnimConfig {
        width,
        height,
        delay_us: 0,
        ..AnimConfig::default()
    };
    let mut player = AnimationPlayer::new(mode, params, BG).expect("registered mode");
    player.ensure_sized(width, height);
    player
}

fn animations(c: &mut Criterion) {
    let mut group = c.benchmark_group("player_advance");
    for mode in MODES {
        group.bench_function(mode, |b| {
            b.iter_batched(
                || {
                    let start = Instant::now();
                    let mut player = player(mode, 800, 600);
                    player.advance(start);
                    let tick_at = start + player.frame_delay();
                    (player, tick_at)
                },
                |(mut player, tick_at)| {
                    black_box(player.advance(tick_at));
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();

    let mut group = c.benchmark_group("blit_into");
    for (width, height) in [(800, 600), (1920, 1080), (3440, 1440)] {
        let mut player = player("binaryring", width, height);
        player.advance(Instant::now());
        let mut dst = vec![0; (width * height * 4) as usize];
        group.throughput(Throughput::Bytes(dst.len() as u64));
        group.bench_function(format!("{width}x{height}"), |b| {
            b.iter(|| {
                player
                    .blit_into(black_box(&mut dst), width as i32, height as i32)
                    .unwrap()
            });
        });
    }
    group.finish();
}

criterion_group!(benches, animations);
criterion_main!(benches);
