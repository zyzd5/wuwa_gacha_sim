//! 概率模型、保底状态机与大珊瑚结算。
//!
//! **所有概率与珊瑚常量都集中在本模块顶部**，其他模块不得重复定义。
//!
//! # 建模要点
//!
//! 产出 5★ 时会把 4★ 的保底计数 [`Banner::pity4`] **一并归零**，因为官方措辞是
//! 「至多 10 次唤取必获得至少 1 个 4★ **或以上**内容」，5★ 同样满足该保底条件。
//!
//! 这一条不是可有可无的细节：两种实现在 4★ 出率上相差 **0.64 个百分点**
//! （重置 12.12% ↔ 不重置 12.76%）。详见 `README.md`。

use rand::Rng;

// ───────────────────────── 5★ 概率模型 ─────────────────────────

/// 5★ 基础概率。
pub const P5_BASE: f64 = 0.008;
/// 5★ 软保底起始抽数：第 66 抽起概率开始提升。
pub const P5_SOFT_START: u32 = 66;
/// 5★ 软保底每抽增量。
pub const P5_SOFT_STEP: f64 = 0.06;
/// 5★ 硬保底抽数。
pub const P5_HARD_PITY: u32 = 80;
/// 非大保底状态下，5★ 命中 UP 限定角色的概率。
pub const P5_UP_CHANCE: f64 = 0.5;

// ───────────────────────── 4★ 概率模型 ─────────────────────────

/// 4★ 内容的基础概率。
pub const P4_BASE: f64 = 0.06;
/// 4★ 硬保底抽数。
pub const P4_HARD_PITY: u32 = 10;

// ─────────────────────── 大珊瑚（余波珊瑚）───────────────────────

/// 5★ **限定**角色：基础 15。
pub const CORAL_5STAR_LIMITED: u32 = 15;
/// 5★ **常驻**角色（即「歪」）：15 基础 + 30 歪补偿。
pub const CORAL_5STAR_STANDARD: u32 = 45;
/// 歪补偿的额度，仅用于输出说明。
pub const CORAL_STANDARD_5STAR_BONUS: u32 = 30;
/// 4★ 内容：3 基础 + 5（假设 4★ 角色均已满共鸣链）。
pub const CORAL_4STAR: u32 = 8;
/// 4★ 满共鸣链转化的额度，仅用于输出说明。
pub const CORAL_4STAR_MAXED_BONUS: u32 = 5;
/// 8 个大珊瑚兑换 1 抽。
pub const CORAL_PER_PULL_EXCHANGE: u32 = 8;

// ─────────────────────────── 理论值 ───────────────────────────

/// 「与理论值对比」输出所用的参考值。
///
/// 5★ 部分为精确解析解；4★/3★ 与珊瑚为 2000 万抽参考模拟的实测值。
/// [`crate::tests`] 中的大样本校验会重新用模拟验证这些数。
pub mod theory {
    use super::{CORAL_4STAR, CORAL_5STAR_LIMITED, CORAL_5STAR_STANDARD, p4, p5};

    /// 5★ 综合出率（含保底），**长期平稳值**。
    pub const P5: f64 = 0.018_648;
    /// 期望出金抽数，**长期平稳值**。
    pub const MEAN_PULLS_PER_5STAR: f64 = 53.625;
    /// 限定 5★ 出率：5★ 的 2/3。
    pub const P5_LIMITED: f64 = P5 * 2.0 / 3.0;
    /// 常驻 5★ 出率：5★ 的 1/3。
    pub const P5_STANDARD: f64 = P5 / 3.0;
    /// 4★ 出率。
    pub const P4: f64 = 0.121_207;
    /// 期望每 4★ 抽数。
    pub const MEAN_PULLS_PER_4STAR: f64 = 8.25;
    /// 3★ 武器出率。
    pub const P3: f64 = 1.0 - P5 - P4;
    /// 平均每抽大珊瑚，**长期平稳值**。
    pub const CORAL_PER_PULL: f64 = P5_LIMITED * CORAL_5STAR_LIMITED as f64
        + P5_STANDARD * CORAL_5STAR_STANDARD as f64
        + P4 * CORAL_4STAR as f64;

    // ────────────────── 从零保底起步的精确期望 ──────────────────
    //
    // 上面的常量是**长期平稳**数值。但一次真实的抽卡是从「保底 0 抽」开始的，
    // 前几十抽享受不到软保底，实际期望明显低于 `抽数 × 长期出率`。
    // 例如 200 抽：长期口径给 3.73 个 5★，真实期望却只有 3.25 个，差了 13%；
    // 限定数的差距更大（1.99 vs 2.49）。拿长期口径去评价运气会系统性偏严。
    //
    // 所以这里用动态规划精确算一遍：状态是 (剩余抽数, pity5, 是否大保底, pity4)，
    // 逆推该状态下的期望「限定 5★ / 常驻 5★ / 4★」数量。

    /// `pity5` 的状态数：`0..=79`。
    const P5_STATES: usize = super::P5_HARD_PITY as usize;
    /// `pity4` 的状态数：`0..=9`。
    const P4_STATES: usize = super::P4_HARD_PITY as usize;
    /// 「是否处于大保底」两种状态。
    const G_STATES: usize = 2;
    /// 每个状态携带的期望值个数：限定 5★ / 常驻 5★ / 4★。
    const LANES: usize = 3;
    /// 精确 DP 的步数上限；超过之后过程已进入平稳期，改用实测增量线性外推。
    const EXACT_LIMIT: u64 = 2_000;
    /// 估计平稳期每抽增量时额外多走的步数。
    const PROBE: u64 = 100;

    /// 从全新状态（保底 0 抽、非大保底）起抽若干次的期望产出。
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Expectations {
        /// 期望获得的限定 5★ 数。
        pub limited5: f64,
        /// 期望获得的常驻 5★ 数（即「歪」的次数）。
        pub standard5: f64,
        /// 期望获得的 4★ 内容数。
        pub four: f64,
    }

    impl Expectations {
        /// 期望 5★ 总数。
        #[must_use]
        pub fn five_star(&self) -> f64 {
            self.limited5 + self.standard5
        }

        /// 期望大珊瑚总量。
        #[must_use]
        pub fn coral(&self) -> f64 {
            self.limited5 * f64::from(CORAL_5STAR_LIMITED)
                + self.standard5 * f64::from(CORAL_5STAR_STANDARD)
                + self.four * f64::from(CORAL_4STAR)
        }

        /// 折算出的 5★ 出率。
        #[must_use]
        pub fn five_star_rate(&self, pulls: u64) -> f64 {
            per_pull(self.five_star(), pulls)
        }

        /// 折算出的 4★ 出率。
        #[must_use]
        pub fn four_rate(&self, pulls: u64) -> f64 {
            per_pull(self.four, pulls)
        }

        /// 折算出的平均每抽大珊瑚。
        #[must_use]
        pub fn coral_per_pull(&self, pulls: u64) -> f64 {
            per_pull(self.coral(), pulls)
        }

        /// 折算出的平均出金抽数。
        #[must_use]
        pub fn mean_pulls_per_5star(&self, pulls: u64) -> f64 {
            reciprocal_mean(self.five_star(), pulls)
        }

        /// 折算出的平均每 4★ 抽数。
        #[must_use]
        pub fn mean_pulls_per_4star(&self, pulls: u64) -> f64 {
            reciprocal_mean(self.four, pulls)
        }
    }

    fn per_pull(count: f64, pulls: u64) -> f64 {
        if pulls == 0 {
            0.0
        } else {
            count / pulls as f64
        }
    }

    fn reciprocal_mean(count: f64, pulls: u64) -> f64 {
        if count > 0.0 {
            pulls as f64 / count
        } else {
            0.0
        }
    }

    /// 从全新状态起抽 `pulls` 次的精确期望。
    ///
    /// `pulls > 2000` 时只精确算 2000 步，其余部分用平稳期每抽增量外推
    /// （多算 100 步来估计该增量）。
    #[must_use]
    pub fn expectations(pulls: u64) -> Expectations {
        if pulls <= EXACT_LIMIT {
            return advance(pulls);
        }
        let base = advance(EXACT_LIMIT);
        let ahead = advance(EXACT_LIMIT + PROBE);
        let extra = (pulls - EXACT_LIMIT) as f64;
        let per_step = 1.0 / PROBE as f64;
        Expectations {
            limited5: base.limited5 + extra * (ahead.limited5 - base.limited5) * per_step,
            standard5: base.standard5 + extra * (ahead.standard5 - base.standard5) * per_step,
            four: base.four + extra * (ahead.four - base.four) * per_step,
        }
    }

    #[inline]
    fn lane_index(pity5: usize, guaranteed: usize, pity4: usize) -> usize {
        ((pity5 * G_STATES + guaranteed) * P4_STATES + pity4) * LANES
    }

    /// 走 `steps` 步 DP，返回起点状态 `(pity5=0, 非大保底, pity4=0)` 的期望值。
    fn advance(steps: u64) -> Expectations {
        let mut cur = vec![0.0f64; P5_STATES * G_STATES * P4_STATES * LANES];
        let mut next = vec![0.0f64; cur.len()];

        // 出 5★ 后的落点：中限定回到非大保底，歪则转入大保底，两者 pity5 与 pity4 都归零。
        let limited_landing = lane_index(0, 0, 0);
        let standard_landing = lane_index(0, 1, 0);

        for _ in 0..steps {
            for pity5 in 0..P5_STATES {
                let chance5 = p5(pity5 as u32 + 1);
                let next_pity5 = (pity5 + 1).min(P5_STATES - 1);

                for guaranteed in 0..G_STATES {
                    // 出 5★ 这一支与 pity4 无关，先算好，避免在 p4 循环里重复计算。
                    let mut five_star = [0.0f64; LANES];
                    for (lane, slot) in five_star.iter_mut().enumerate() {
                        let limited_hit =
                            cur[limited_landing + lane] + if lane == 0 { 1.0 } else { 0.0 };
                        *slot = if guaranteed == 1 {
                            limited_hit
                        } else {
                            let standard_hit =
                                cur[standard_landing + lane] + if lane == 1 { 1.0 } else { 0.0 };
                            0.5 * limited_hit + 0.5 * standard_hit
                        };
                    }

                    for pity4 in 0..P4_STATES {
                        let chance4 = p4(pity4 as u32 + 1);
                        let hit_four = lane_index(next_pity5, guaranteed, 0);
                        let miss = lane_index(next_pity5, guaranteed, (pity4 + 1).min(P4_STATES - 1));
                        let base = lane_index(pity5, guaranteed, pity4);

                        for lane in 0..LANES {
                            let after_four =
                                cur[hit_four + lane] + if lane == 2 { 1.0 } else { 0.0 };
                            let after_miss = cur[miss + lane];
                            next[base + lane] = chance5 * five_star[lane]
                                + (1.0 - chance5)
                                    * (chance4 * after_four + (1.0 - chance4) * after_miss);
                        }
                    }
                }
            }
            std::mem::swap(&mut cur, &mut next);
        }

        let start = lane_index(0, 0, 0);
        Expectations {
            limited5: cur[start],
            standard5: cur[start + 1],
            four: cur[start + 2],
        }
    }
}

// ──────────────────────────── 模型 ────────────────────────────

/// 单抽出 5★ 的概率。
///
/// `n` = 距上次出 5★ 已抽的抽数 + 1，即本次是第 `n` 抽。
///
/// | n | 概率 |
/// | --- | --- |
/// | 1..=65 | 0.8% |
/// | 66 | 6.8% |
/// | 79 | 84.8% |
/// | 80 | 100% |
#[must_use]
pub fn p5(n: u32) -> f64 {
    if n >= P5_HARD_PITY {
        1.0
    } else if n >= P5_SOFT_START {
        (P5_BASE + P5_SOFT_STEP * f64::from(n - (P5_SOFT_START - 1))).min(1.0)
    } else {
        P5_BASE
    }
}

/// 单抽出 4★ 的概率。
///
/// 本模拟器不实现 4★ 的 50/50，也不拆分 UP / 非 UP，4★ 只按概率产出并计数。
#[must_use]
pub fn p4(n: u32) -> f64 {
    if n >= P4_HARD_PITY { 1.0 } else { P4_BASE }
}

/// 一次唤取的产出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// UP 限定 5★ 角色。
    Limited5,
    /// 常驻 5★ 角色，玩家口中的「歪」。
    Standard5,
    /// 4★ 内容（角色或武器）。
    Four,
    /// 3★ 武器。
    Three,
}

impl Item {
    /// 本次产出结算的大珊瑚数量。
    #[must_use]
    pub const fn coral(self) -> u32 {
        match self {
            Item::Limited5 => CORAL_5STAR_LIMITED,
            Item::Standard5 => CORAL_5STAR_STANDARD,
            Item::Four => CORAL_4STAR,
            Item::Three => 0,
        }
    }

    /// 是否为 5★。
    #[must_use]
    pub const fn is_five_star(self) -> bool {
        matches!(self, Item::Limited5 | Item::Standard5)
    }

    /// 明细行中显示的稀有度标签。
    #[must_use]
    pub const fn rarity_label(self) -> &'static str {
        match self {
            Item::Limited5 => "5★  限定",
            Item::Standard5 => "5★  常驻",
            Item::Four => "4★  内容",
            Item::Three => "3★  武器",
        }
    }
}

/// 一次唤取的结果。
#[derive(Clone, Copy, Debug)]
pub struct PullOutcome {
    /// 产出的物品。
    pub item: Item,
    /// 本次出金是否动用了大保底（仅对 5★ 有意义）。
    pub used_guarantee: bool,
}

/// 卡池状态机。
///
/// 每次运行都从全新状态开始（5★/4★ 保底均为 0 抽、非大保底）。
/// 本模拟器**不支持**任何跨运行的续跑或状态持久化。
pub struct Banner<R: Rng> {
    rng: R,
    /// 已累计未出 5★ 的抽数，取值 `0..=79`。
    pity5: u32,
    /// 已累计未出「4★ 或以上」的抽数，取值 `0..=9`。
    pity4: u32,
    /// 5★ 大保底：`true` 表示下次出金必为 UP 限定角色。
    guaranteed5: bool,
}

impl<R: Rng> Banner<R> {
    /// 用给定随机源构造一个全新状态的卡池。
    pub fn new(rng: R) -> Self {
        Self {
            rng,
            pity5: 0,
            pity4: 0,
            guaranteed5: false,
        }
    }

    /// 当前的 5★ 保底计数（已累计未出 5★ 的抽数）。
    #[must_use]
    pub fn pity5(&self) -> u32 {
        self.pity5
    }

    /// 当前的 4★ 保底计数（已累计未出「4★ 或以上」的抽数）。
    #[must_use]
    pub fn pity4(&self) -> u32 {
        self.pity4
    }

    /// 当前是否处于 5★ 大保底。
    #[must_use]
    pub fn guaranteed5(&self) -> bool {
        self.guaranteed5
    }

    /// 执行一次唤取。
    pub fn pull(&mut self) -> PullOutcome {
        if self.rng.random::<f64>() < p5(self.pity5 + 1) {
            self.pity5 = 0;
            // 5★ 属于「4★ 或以上」，因此一并重置 4★ 保底计数。
            self.pity4 = 0;

            let used_guarantee = self.guaranteed5;
            let limited = used_guarantee || self.rng.random::<f64>() < P5_UP_CHANCE;
            self.guaranteed5 = !limited;

            PullOutcome {
                item: if limited {
                    Item::Limited5
                } else {
                    Item::Standard5
                },
                used_guarantee,
            }
        } else {
            self.pity5 += 1;
            if self.rng.random::<f64>() < p4(self.pity4 + 1) {
                self.pity4 = 0;
                PullOutcome {
                    item: Item::Four,
                    used_guarantee: false,
                }
            } else {
                self.pity4 += 1;
                PullOutcome {
                    item: Item::Three,
                    used_guarantee: false,
                }
            }
        }
    }
}
