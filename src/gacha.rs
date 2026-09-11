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
    use super::{CORAL_4STAR, CORAL_5STAR_LIMITED, CORAL_5STAR_STANDARD};

    /// 5★ 综合出率（含保底）。
    pub const P5: f64 = 0.018_648;
    /// 期望出金抽数。
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
    /// 平均每抽大珊瑚。
    pub const CORAL_PER_PULL: f64 = P5_LIMITED * CORAL_5STAR_LIMITED as f64
        + P5_STANDARD * CORAL_5STAR_STANDARD as f64
        + P4 * CORAL_4STAR as f64;
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
