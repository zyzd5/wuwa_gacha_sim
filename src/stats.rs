//! 统计聚合。
//!
//! [`Stats::record`] 是唯一的入口，它同时负责累加大珊瑚。
//! 由于珊瑚只在 `record` 里结算，恒等式
//! `大珊瑚总量 == 限定5★数 × 15 + 常驻5★数 × 45 + 4★数 × 8`
//! 是结构性成立的，而不是靠调用方自觉。

use crate::gacha::{CORAL_PER_PULL_EXCHANGE, Item, PullOutcome};

/// 一条 5★ / 4★ 的明细记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    /// 事件序号，5★ 与 4★ 混排，从 1 开始连续编号。
    pub seq: u64,
    /// 这是整个模拟中的第几抽。
    pub pull_index: u64,
    /// 产出的物品。
    pub item: Item,
    /// 本次出金是否动用了大保底。
    pub used_guarantee: bool,
}

/// 一次模拟的统计结果。
#[derive(Debug, Default)]
pub struct Stats {
    /// 总抽数。
    pub pulls: u64,
    /// 限定 5★ 数量。
    pub limited5: u64,
    /// 常驻 5★ 数量（等于「歪」的次数）。
    pub standard5: u64,
    /// 4★ 内容数量。
    pub four: u64,
    /// 3★ 武器数量。
    pub three: u64,
    /// 累计获得的大珊瑚。
    pub coral: u64,
    /// 明细，仅在需要时记录。
    pub details: Vec<Detail>,
    /// 已完成的 5★ 间隔，用于统计「最欧 / 最非」。
    gaps: Vec<u64>,
    /// 当前这一段尚未出金的抽数。
    since5: u64,
}

impl Stats {
    /// 新建一份空统计。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次唤取。
    ///
    /// `pull_index` 是本次唤取在整个模拟中的序号（从 1 开始）。
    /// `record_detail` 为 `false` 时不保存明细，避免大样本下占用过多内存。
    pub fn record(&mut self, pull_index: u64, outcome: PullOutcome, record_detail: bool) {
        self.pulls = pull_index;
        self.since5 += 1;
        self.coral += u64::from(outcome.item.coral());

        match outcome.item {
            Item::Limited5 | Item::Standard5 => {
                if outcome.item == Item::Limited5 {
                    self.limited5 += 1;
                } else {
                    self.standard5 += 1;
                }
                self.gaps.push(self.since5);
                self.since5 = 0;

                if record_detail {
                    let seq = self.details.len() as u64 + 1;
                    self.details.push(Detail {
                        seq,
                        pull_index,
                        item: outcome.item,
                        used_guarantee: outcome.used_guarantee,
                    });
                }
            }
            Item::Four => {
                self.four += 1;
                if record_detail {
                    let seq = self.details.len() as u64 + 1;
                    self.details.push(Detail {
                        seq,
                        pull_index,
                        item: outcome.item,
                        used_guarantee: false,
                    });
                }
            }
            Item::Three => self.three += 1,
        }
    }

    /// 5★ 总数。
    #[must_use]
    pub fn five_star_total(&self) -> u64 {
        self.limited5 + self.standard5
    }

    /// 5★ 综合出率。
    #[must_use]
    pub fn five_star_rate(&self) -> Option<f64> {
        ratio(self.five_star_total(), self.pulls)
    }

    /// 限定 5★ 出率。
    #[must_use]
    pub fn limited5_rate(&self) -> Option<f64> {
        ratio(self.limited5, self.pulls)
    }

    /// 平均出金抽数。
    #[must_use]
    pub fn mean_pulls_per_5star(&self) -> Option<f64> {
        ratio(self.pulls, self.five_star_total())
    }

    /// 4★ 出率。
    #[must_use]
    pub fn four_star_rate(&self) -> Option<f64> {
        ratio(self.four, self.pulls)
    }

    /// 平均每 4★ 抽数。
    #[must_use]
    pub fn mean_pulls_per_4star(&self) -> Option<f64> {
        ratio(self.pulls, self.four)
    }

    /// 3★ 武器出率。
    #[must_use]
    pub fn three_star_rate(&self) -> Option<f64> {
        ratio(self.three, self.pulls)
    }

    /// 最短的一次出金间隔（最欧）。没有任何 5★ 时为 `None`。
    #[must_use]
    pub fn min_gap(&self) -> Option<u64> {
        self.gaps.iter().copied().min()
    }

    /// 最长的一次出金间隔（最非）。没有任何 5★ 时为 `None`。
    #[must_use]
    pub fn max_gap(&self) -> Option<u64> {
        self.gaps.iter().copied().max()
    }

    /// 平均每抽大珊瑚。
    #[must_use]
    pub fn coral_per_pull(&self) -> Option<f64> {
        ratio(self.coral, self.pulls)
    }

    /// 大珊瑚可兑换的抽数（向下取整）。
    #[must_use]
    pub fn exchangeable_pulls(&self) -> u64 {
        self.coral / u64::from(CORAL_PER_PULL_EXCHANGE)
    }
}

/// `num / den`，`den == 0` 时返回 `None`（而不是 `NaN`）。
fn ratio(num: u64, den: u64) -> Option<f64> {
    if den == 0 {
        None
    } else {
        Some(num as f64 / den as f64)
    }
}
