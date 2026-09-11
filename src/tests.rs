//! 单元测试。
//!
//! 大样本校验被标记为 `#[ignore]`，默认不跑；运行方式见 `README.md`。

use rand::RngCore;

use crate::gacha::{
    Banner, CORAL_4STAR, CORAL_4STAR_MAXED_BONUS, CORAL_5STAR_LIMITED, CORAL_5STAR_STANDARD,
    CORAL_STANDARD_5STAR_BONUS, Item, P4_HARD_PITY, P5_HARD_PITY, p4, p5, theory,
};
use crate::rng;
use crate::stats::Stats;

// ─────────────────────────── 测试工具 ───────────────────────────

/// 测试用随机源：`next_u64` 恒定返回同一个值。
///
/// rand 0.9 的 `StandardUniform<f64>` 实现是 `(next_u64() >> 11) as f64 * 2^-53`，
/// 因此取 `0` 会让所有概率判定成功，取 `u64::MAX` 会让所有 `p < 1` 的判定失败
/// （`p == 1.0` 的硬保底仍会命中）。
struct FixedRng(u64);

impl FixedRng {
    /// 所有概率判定都成功。
    const PASS: Self = Self(0);
    /// 所有 `p < 1` 的概率判定都失败。
    const FAIL: Self = Self(u64::MAX);
}

impl RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        (self.0 >> 32) as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.0
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        for byte in dst {
            *byte = (self.0 & 0xff) as u8;
        }
    }
}

/// 用给定随机源跑 `pulls` 次，返回每次的产出与统计。
fn run<R: rand::Rng>(rng: R, pulls: u64) -> (Vec<Item>, Stats) {
    let mut banner = Banner::new(rng);
    let mut stats = Stats::new();
    let mut items = Vec::with_capacity(pulls as usize);
    for index in 1..=pulls {
        let outcome = banner.pull();
        items.push(outcome.item);
        stats.record(index, outcome, true);
    }
    (items, stats)
}

/// 与理论值的相对误差是否在 `tolerance` 之内。
fn within(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= expected.abs() * tolerance
}

// ───────────────────────── 概率曲线本身 ─────────────────────────

#[test]
fn p5_curve_matches_spec() {
    assert!((p5(1) - 0.008).abs() < 1e-12, "p5(1) = {}", p5(1));
    assert!((p5(2) - 0.008).abs() < 1e-12);
    assert!((p5(65) - 0.008).abs() < 1e-12, "p5(65) = {}", p5(65));
    assert!((p5(66) - 0.068).abs() < 1e-12, "p5(66) = {}", p5(66));
    assert!((p5(67) - 0.128).abs() < 1e-12);
    assert!((p5(79) - 0.848).abs() < 1e-12, "p5(79) = {}", p5(79));
    assert!((p5(80) - 1.0).abs() < 1e-12, "p5(80) = {}", p5(80));
}

#[test]
fn p5_is_monotonic_and_bounded() {
    let mut previous = 0.0;
    for n in 1..=P5_HARD_PITY {
        let current = p5(n);
        assert!(current >= previous, "p5 在 n={n} 处下降了");
        assert!((0.0..=1.0).contains(&current), "p5({n}) = {current} 越界");
        previous = current;
    }
    // 超过硬保底也必须是 1.0
    assert!((p5(P5_HARD_PITY + 10) - 1.0).abs() < 1e-12);
}

#[test]
fn p4_curve_matches_spec() {
    for n in 1..P4_HARD_PITY {
        assert!((p4(n) - 0.06).abs() < 1e-12, "p4({n}) = {}", p4(n));
    }
    assert!((p4(P4_HARD_PITY) - 1.0).abs() < 1e-12);
    assert!((p4(P4_HARD_PITY + 5) - 1.0).abs() < 1e-12);
}

// ───────────────────────── 保底与控制流 ─────────────────────────

#[test]
fn five_star_hard_pity_hits_at_pull_80() {
    // FAIL 让所有 p<1 的判定失败，因此 5★ 只可能由第 80 抽的硬保底给出。
    let (items, stats) = run(FixedRng::FAIL, 80);

    for (i, item) in items.iter().enumerate().take(79) {
        assert!(
            !item.is_five_star(),
            "第 {} 抽不应该是 5★（p5 < 1 时不应命中）",
            i + 1
        );
    }
    assert_eq!(items[79], Item::Standard5, "第 80 抽必须由硬保底给出 5★");
    assert_eq!(stats.five_star_total(), 1);
}

#[test]
fn pity5_resets_after_gold() {
    let (_, stats) = run(FixedRng::FAIL, 80);
    // 80 抽里只出了第 80 抽那一金，所以最欧与最非都是 80。
    assert_eq!(stats.min_gap(), Some(80));
    assert_eq!(stats.max_gap(), Some(80));
}

#[test]
fn five_star_guarantee_alternates_after_losing_5050() {
    // FAIL 下每次出金都会歪（50/50 判定也失败），于是下一次必为限定。
    let (items, stats) = run(FixedRng::FAIL, 160);

    assert_eq!(stats.standard5, 1, "第一次出金应歪");
    assert_eq!(stats.limited5, 1, "第二次出金应为大保底限定");
    assert_eq!(items[79], Item::Standard5);
    assert_eq!(items[159], Item::Limited5);
}

#[test]
fn pass_rng_yields_five_star_every_pull() {
    // PASS 让每次 5★ 判定都成功，且 50/50 也总是命中 UP。
    let (items, stats) = run(FixedRng::PASS, 10);
    assert!(items.iter().all(|i| *i == Item::Limited5));
    assert_eq!(stats.limited5, 10);
    assert_eq!(stats.four, 0);
    assert_eq!(stats.three, 0);
}

#[test]
fn pity4_is_reset_by_five_star() {
    // 这一条专门锁住 3.2 节的「5★ 重置 4★ 保底计数」语义。
    //
    // FAIL 下的节奏：4★ 固定出现在第 10/20/…/70 抽；第 80 抽是硬保底 5★，
    // 它把 pity4 归零，于是被顶掉的那个 4★ 顺延到第 90 抽而不是第 81 抽。
    let (items, _) = run(FixedRng::FAIL, 100);

    let four_star_pulls: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| **item == Item::Four)
        .map(|(i, _)| i + 1)
        .collect();

    assert_eq!(
        four_star_pulls,
        vec![10, 20, 30, 40, 50, 60, 70, 90, 100],
        "5★ 必须重置 4★ 保底计数；若得到 81 说明漏了这次重置"
    );
    assert_eq!(items[79], Item::Standard5, "第 80 抽应是 5★");
    assert_eq!(items[80], Item::Three, "第 81 抽应是 3★（pity4 已被重置）");
}

// ───────────────────────────── 大珊瑚 ─────────────────────────────

#[test]
fn coral_constants_match_rules() {
    assert_eq!(CORAL_5STAR_LIMITED, 15);
    assert_eq!(CORAL_5STAR_STANDARD, 45);
    assert_eq!(CORAL_4STAR, 8);
    // 「歪」相对限定恰好多出 30。
    assert_eq!(CORAL_5STAR_STANDARD - CORAL_5STAR_LIMITED, CORAL_STANDARD_5STAR_BONUS);
    assert_eq!(CORAL_STANDARD_5STAR_BONUS, 30);
    // 4★ 的 8 = 3 基础 + 5 满共鸣链转化。
    assert_eq!(CORAL_4STAR - CORAL_4STAR_MAXED_BONUS, 3);
}

#[test]
fn only_three_star_weapons_produce_no_coral() {
    // 5 抽内既不到 4★ 保底也不到 5★ 保底，因此全是 3★ 武器。
    let (items, stats) = run(FixedRng::FAIL, 5);
    assert!(items.iter().all(|i| *i == Item::Three));
    assert_eq!(stats.coral, 0);
    assert_eq!(stats.exchangeable_pulls(), 0);
}

#[test]
fn coral_identity_holds_over_a_large_run() {
    let (_, stats) = run(rng::seeded(20240614), 200_000);

    let expected = stats.limited5 * u64::from(CORAL_5STAR_LIMITED)
        + stats.standard5 * u64::from(CORAL_5STAR_STANDARD)
        + stats.four * u64::from(CORAL_4STAR);
    assert_eq!(
        stats.coral, expected,
        "大珊瑚总量必须等于 限定5★×15 + 常驻5★×45 + 4★×8"
    );

    // 顺带确认三档稀有度把总抽数分干净了。
    assert_eq!(
        stats.limited5 + stats.standard5 + stats.four + stats.three,
        stats.pulls
    );
}

// ───────────────────────── 可复现性与边界 ─────────────────────────

#[test]
fn same_seed_produces_identical_results() {
    let a = run(rng::seeded(42), 5_000).1;
    let b = run(rng::seeded(42), 5_000).1;

    assert_eq!(a.limited5, b.limited5);
    assert_eq!(a.standard5, b.standard5);
    assert_eq!(a.four, b.four);
    assert_eq!(a.three, b.three);
    assert_eq!(a.coral, b.coral);
    assert_eq!(a.min_gap(), b.min_gap());
    assert_eq!(a.max_gap(), b.max_gap());
    assert_eq!(a.details, b.details, "逐条明细也必须完全一致");
}

#[test]
fn different_seeds_produce_different_results() {
    let a = run(rng::seeded(1), 5_000).1;
    let b = run(rng::seeded(2), 5_000).1;
    assert!(
        a.coral != b.coral || a.details != b.details,
        "不同种子不应给出完全相同的模拟"
    );
}

#[test]
fn zero_pulls_is_safe() {
    let stats = Stats::new();
    assert_eq!(stats.pulls, 0);
    assert_eq!(stats.coral, 0);
    assert_eq!(stats.exchangeable_pulls(), 0);
    assert_eq!(stats.five_star_total(), 0);
    // 除数为 0 时必须是 None，交由渲染层显示 '-'，绝不能是 NaN。
    assert!(stats.five_star_rate().is_none());
    assert!(stats.four_star_rate().is_none());
    assert!(stats.mean_pulls_per_5star().is_none());
    assert!(stats.mean_pulls_per_4star().is_none());
    assert!(stats.coral_per_pull().is_none());
    assert!(stats.min_gap().is_none());
    assert!(stats.max_gap().is_none());
}

// ─────────────────────────── 理论值自检 ───────────────────────────

#[test]
fn theory_constants_are_self_consistent() {
    let sum = theory::P5 + theory::P4 + theory::P3;
    assert!(
        (sum - 1.0).abs() < 1e-9,
        "三项出率之和应为 100%，实际 {sum}"
    );
    assert!((theory::P5_LIMITED + theory::P5_STANDARD - theory::P5).abs() < 1e-12);
    assert!((theory::MEAN_PULLS_PER_5STAR - 1.0 / theory::P5).abs() < 0.01);

    let expected_coral = theory::P5_LIMITED * f64::from(CORAL_5STAR_LIMITED)
        + theory::P5_STANDARD * f64::from(CORAL_5STAR_STANDARD)
        + theory::P4 * f64::from(CORAL_4STAR);
    assert!((theory::CORAL_PER_PULL - expected_coral).abs() < 1e-12);
}

// ─────────────────────────── 大样本校验 ───────────────────────────

/// 200 万抽的蒙特卡洛校验，用于确认实现与理论值一致。
///
/// 数据量大，默认跳过。运行方式：
///
/// ```text
/// cargo test --release -- --ignored --nocapture
/// ```
#[test]
#[ignore = "200 万抽，建议用 --release 运行"]
fn large_sample_matches_theory() {
    const PULLS: u64 = 2_000_000;
    let (_, stats) = run(rng::seeded(7), PULLS);

    let five_star_rate = stats.five_star_rate().expect("抽数不为 0");
    let mean_per_5star = stats.mean_pulls_per_5star().expect("抽数不为 0");
    let four_star_rate = stats.four_star_rate().expect("抽数不为 0");
    let coral_per_pull = stats.coral_per_pull().expect("抽数不为 0");

    println!("5★ 出率        : {:.4}%  (理论 {:.4}%)", five_star_rate * 100.0, theory::P5 * 100.0);
    println!("平均出金抽数   : {:.4}   (理论 {:.3})", mean_per_5star, theory::MEAN_PULLS_PER_5STAR);
    println!("4★ 出率        : {:.4}%  (理论 {:.4}%)", four_star_rate * 100.0, theory::P4 * 100.0);
    println!("3★ 武器出率    : {:.4}%", stats.three_star_rate().unwrap_or_default() * 100.0);
    println!("平均每抽大珊瑚 : {:.4}   (理论 {:.4})", coral_per_pull, theory::CORAL_PER_PULL);
    println!("平均每 100 抽  : {:.1}", coral_per_pull * 100.0);

    assert!(
        (0.0183..=0.0190).contains(&five_star_rate),
        "5★ 出率 {five_star_rate} 超出 [1.83%, 1.90%]"
    );
    assert!(
        (52.5..=54.8).contains(&mean_per_5star),
        "平均出金抽数 {mean_per_5star} 超出 [52.5, 54.8]"
    );
    // 上界卡在 12.5%：若跑出 12.7% 说明 5★ 重置 4★ 保底的逻辑漏实现了。
    assert!(
        (0.118..=0.125).contains(&four_star_rate),
        "4★ 出率 {four_star_rate} 超出 [11.8%, 12.5%]"
    );
    assert!(
        (1.40..=1.48).contains(&coral_per_pull),
        "平均每抽大珊瑚 {coral_per_pull} 超出 [1.40, 1.48]"
    );

    assert!(within(five_star_rate, theory::P5, 0.05));
    assert!(within(four_star_rate, theory::P4, 0.05));
    assert!(within(coral_per_pull, theory::CORAL_PER_PULL, 0.05));
}
