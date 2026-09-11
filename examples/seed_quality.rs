//! 两件事：
//!
//! 1. 检查 `--repeat` 所用的「基准种子 + i」派生方式，会不会让相邻两次模拟产生相关的随机流。
//! 2. 把 `theory::expectations` 的精确期望与实测均值对照。
//!
//! ```console
//! cargo run --release --example seed_quality
//! ```

use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use wuwa_gacha_sim::gacha::{Banner, theory};
use wuwa_gacha_sim::rng;
use wuwa_gacha_sim::stats::Stats;

const PULLS: u64 = 200;
const RUNS: usize = 20_000;

fn main() {
    println!("== 1. 比特级：两个种子产生的 u64 流是否相关 ==");
    println!("   （把两个流逐元素 XOR 后统计 1 的个数；无关时应为 64 位里的 32 位）");
    println!();
    bit_level();

    println!();
    println!("== 2. 模拟层：{RUNS} 次 × {PULLS} 抽，限定 5★ 的分布 ==");
    println!();
    let consecutive = distribution_level();

    println!();
    println!("== 3. 精确期望（DP）与实测均值对照 ==");
    println!();
    expectation_level(consecutive);
}

/// 用 `seed` 跑一次 `PULLS` 抽，返回 (限定, 常驻, 4★)。
fn run_once(seed: u64) -> (u64, u64, u64) {
    let mut banner = Banner::new(rng::seeded(seed));
    let mut stats = Stats::new();
    for index in 1..=PULLS {
        let outcome = banner.pull();
        stats.record(index, outcome, false);
    }
    (stats.limited5, stats.standard5, stats.four)
}

fn bit_level() {
    const PAIRS: u64 = 256;
    const PER_PAIR: u64 = 1024;
    let samples = PAIRS * PER_PAIR;

    for (label, step) in [
        ("+1", 1u64),
        ("+65_536", 1 << 16),
        ("+2^32", 1 << 32),
        ("+2^63", 1 << 63),
    ] {
        let mut histogram = [0u64; 65];
        let mut sum = 0u64;
        for base in 0..PAIRS {
            let mut a = ChaCha8Rng::seed_from_u64(base);
            let mut b = ChaCha8Rng::seed_from_u64(base.wrapping_add(step));
            for _ in 0..PER_PAIR {
                let distance = (a.next_u64() ^ b.next_u64()).count_ones();
                histogram[distance as usize] += 1;
                sum += u64::from(distance);
            }
        }
        let mean = sum as f64 / samples as f64;
        let (chi2, df) = binomial_chi_square(&histogram, samples);
        let critical = chi_square_critical(df);
        println!(
            "   种子相差 {label:>8} : 平均 Hamming 距离 {mean:.4} 位   卡方 {chi2:5.2} (df={df}, 临界 {critical:.2})  {}",
            if chi2 < critical { "✓ 通过" } else { "✗ 偏离" }
        );
    }

    // 对照组：同一个种子自比，距离必须恰好为 0，用来证明上面的检验确实有分辨力。
    let mut same = 0u64;
    for base in 0..PAIRS {
        let mut a = ChaCha8Rng::seed_from_u64(base);
        let mut b = ChaCha8Rng::seed_from_u64(base);
        for _ in 0..PER_PAIR {
            same += u64::from((a.next_u64() ^ b.next_u64()).count_ones());
        }
    }
    println!(
        "   对照组（同一种子自比）: 平均 Hamming 距离 {:.4} 位  ← 检验灵敏度确认",
        same as f64 / samples as f64
    );
}

fn distribution_level() -> Vec<(u64, u64, u64)> {
    let consecutive: Vec<u64> = (0..RUNS as u64).map(|i| 1 + i).collect();
    let spread: Vec<u64> = (0..RUNS as u64)
        .map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .collect();
    let mut master = ChaCha8Rng::seed_from_u64(20_240_614);
    let derived: Vec<u64> = (0..RUNS).map(|_| master.random::<u64>()).collect();

    let groups = [
        ("基准种子 + i （--repeat 的方式）", &consecutive),
        ("大跨度种子 i × 黄金比常数", &spread),
        ("单个主 RNG 派生", &derived),
    ];

    println!(
        "   {:<34} {:>8} {:>8} {:>12} {:>10}",
        "派生方式", "均值", "方差", "观察范围", "P(限定=0)"
    );
    let mut histograms = Vec::new();
    let mut consecutive_counts = Vec::new();
    for (label, seeds) in groups {
        let samples: Vec<(u64, u64, u64)> = seeds.iter().map(|seed| run_once(*seed)).collect();
        if label.starts_with("基准") {
            consecutive_counts = samples.clone();
        }
        let counts: Vec<u64> = samples.iter().map(|s| s.0).collect();
        let n = counts.len() as f64;
        let mean = counts.iter().sum::<u64>() as f64 / n;
        let variance =
            counts.iter().map(|c| (*c as f64 - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let zero_share = counts.iter().filter(|c| **c == 0).count() as f64 / n;
        let low = counts.iter().copied().min().unwrap_or(0);
        let high = counts.iter().copied().max().unwrap_or(0);

        let mut histogram = vec![0u64; 9];
        for c in &counts {
            histogram[(*c as usize).min(8)] += 1;
        }
        histograms.push(histogram);

        println!(
            "   {label:<34} {mean:>8.4} {variance:>8.4} {:>6}..{:<5} {:>9.2}%",
            low,
            high,
            zero_share * 100.0
        );
    }
    println!("   {:<34} {:>8.4}", "长期理论值", PULLS as f64 * theory::P5_LIMITED);

    let (chi2, df) = chi_square(&histograms[0], &histograms[2]);
    let critical = chi_square_critical(df);
    println!();
    println!(
        "   连续种子 vs 主 RNG 派生 的卡方: {chi2:.2} (df={df}, 临界 {critical:.2})  {}",
        if chi2 < critical { "✓ 通过" } else { "✗ 偏离" }
    );

    consecutive_counts
}

fn expectation_level(samples: Vec<(u64, u64, u64)>) {
    let n = samples.len() as f64;
    let measured_limited = samples.iter().map(|s| s.0).sum::<u64>() as f64 / n;
    let measured_standard = samples.iter().map(|s| s.1).sum::<u64>() as f64 / n;
    let measured_four = samples.iter().map(|s| s.2).sum::<u64>() as f64 / n;

    let exact = theory::expectations(PULLS);
    println!(
        "   {PULLS} 抽（从零保底起步）的期望：{:>10} {:>10} {:>10} {:>12}",
        "限定", "常驻", "5★ 合计", "4★"
    );
    println!(
        "   {:<28} {:>10.4} {:>10.4} {:>10.4} {:>12.4}",
        "精确 DP",
        exact.limited5,
        exact.standard5,
        exact.five_star(),
        exact.four
    );
    println!(
        "   {:<28} {:>10.4} {:>10.4} {:>10.4} {:>12.4}",
        "实测（20000 次平均）",
        measured_limited,
        measured_standard,
        measured_limited + measured_standard,
        measured_four
    );
    println!(
        "   {:<28} {:>10.4} {:>10.4} {:>10.4} {:>12.4}",
        "长期口径 抽数 × 出率",
        PULLS as f64 * theory::P5_LIMITED,
        PULLS as f64 * theory::P5_STANDARD,
        PULLS as f64 * theory::P5,
        PULLS as f64 * theory::P4
    );

    println!();
    println!("   不同抽数下的精确期望，以及折算出的率：");
    println!(
        "   {:<10} {:>10} {:>10} {:>12} {:>12} {:>12}",
        "抽数", "E[5★]", "E[限定]", "E[4★]", "折算 5★率", "折算 4★率"
    );
    for pulls in [10_u64, 80, 200, 1000, 2_000, 10_000, 1_000_000, 20_000_000] {
        let e = theory::expectations(pulls);
        println!(
            "   {pulls:<10} {:>10.3} {:>10.3} {:>12.1} {:>11.4}% {:>11.4}%",
            e.five_star(),
            e.limited5,
            e.four,
            e.five_star_rate(pulls) * 100.0,
            e.four_rate(pulls) * 100.0
        );
    }
    println!(
        "   {:<10} {:>10.4}% {:>9} {:>11.4}%",
        "长期率", theory::P5 * 100.0, "—", theory::P4 * 100.0
    );
}

/// 把 64 位 Hamming 距离的直方图按 4 位一档归并，与 Binomial(64, 0.5) 做卡方检验。
/// 返回 `(卡方, 自由度)`。
fn binomial_chi_square(histogram: &[u64; 65], samples: u64) -> (f64, usize) {
    let bin_of = |distance: usize| match distance {
        0..=15 => 0,
        16..=19 => 1,
        20..=23 => 2,
        24..=27 => 3,
        28..=31 => 4,
        32..=35 => 5,
        36..=39 => 6,
        40..=43 => 7,
        44..=47 => 8,
        _ => 9,
    };

    let mut observed = [0u64; 10];
    for (distance, count) in histogram.iter().enumerate() {
        observed[bin_of(distance)] += count;
    }

    // Binomial(64, 1/2) 的概率质量函数，迭代计算避免组合数溢出。
    let mut pmf = [0.0f64; 65];
    pmf[0] = 2f64.powi(-64);
    for k in 1..=64usize {
        pmf[k] = pmf[k - 1] * (65 - k) as f64 / k as f64;
    }
    let mut expected = [0.0f64; 10];
    for (distance, probability) in pmf.iter().enumerate() {
        expected[bin_of(distance)] += probability * samples as f64;
    }

    chi_square_generic(&observed, &expected)
}

/// 两组以上频数的卡方齐性检验，自动跳过全零的档位。
fn chi_square(a: &[u64], b: &[u64]) -> (f64, usize) {
    let total_a: u64 = a.iter().sum();
    let total_b: u64 = b.iter().sum();
    let combined = (total_a + total_b) as f64;

    let mut chi2 = 0.0;
    let mut columns: usize = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        if *x + *y == 0 {
            continue; // 两组都为 0 的档位没有信息量，跳过（否则会除零）
        }
        columns += 1;
        let column = (*x + *y) as f64;
        let expected_a = column * total_a as f64 / combined;
        let expected_b = column * total_b as f64 / combined;
        chi2 += (*x as f64 - expected_a).powi(2) / expected_a
            + (*y as f64 - expected_b).powi(2) / expected_b;
    }
    // 自由度 = (档位数 - 1) × (组数 - 1)
    (chi2, columns.saturating_sub(1))
}

fn chi_square_generic(observed: &[u64], expected: &[f64]) -> (f64, usize) {
    let mut chi2 = 0.0;
    let mut columns: usize = 0;
    for (o, e) in observed.iter().zip(expected.iter()) {
        if *e <= 0.0 {
            continue;
        }
        columns += 1;
        chi2 += (*o as f64 - e).powi(2) / e;
    }
    (chi2, columns.saturating_sub(1))
}

/// 卡方分布 α = 0.05 的临界值，df = 1..=12。
fn chi_square_critical(df: usize) -> f64 {
    const TABLE: [f64; 12] = [
        3.84, 5.99, 7.81, 9.49, 11.07, 12.59, 14.07, 15.51, 16.92, 18.31, 19.68, 21.03,
    ];
    TABLE[df.clamp(1, 12) - 1]
}
