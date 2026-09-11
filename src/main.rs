//! 命令行入口：参数解析与结果渲染。

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;

use wuwa_gacha_sim::gacha::{self, Banner, Item};
use wuwa_gacha_sim::rng;
use wuwa_gacha_sim::stats::Stats;

/// 抽卡次数不超过这个值时，默认打印逐条明细。
const DETAIL_THRESHOLD: u64 = 200;
/// 汇总区标签列的统一显示宽度。
const LABEL_WIDTH: usize = 17;

/// 鸣潮「角色活动唤取」限定 5★ 角色抽卡模拟器。
#[derive(Debug, Parser)]
#[command(name = "wuwa_gacha_sim", version, about, long_about = None)]
struct Cli {
    /// 抽卡次数
    #[arg(short = 'n', long = "pulls", value_name = "N")]
    pulls: u64,

    /// 随机种子；不给则取系统时间，并打印实际使用的种子
    #[arg(long, value_name = "U64")]
    seed: Option<u64>,

    /// 只打印汇总与结束状态，不打印逐条明细
    #[arg(long, conflicts_with = "detail")]
    summary_only: bool,

    /// 强制打印逐条明细（抽卡次数大于 200 时默认不打印）
    #[arg(long)]
    detail: bool,

    /// 关闭 ANSI 颜色
    #[arg(long)]
    no_color: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let seed = cli.seed.unwrap_or_else(rng::time_seed);
    let mut banner = Banner::new(rng::seeded(seed));

    let show_detail = if cli.summary_only {
        false
    } else if cli.detail {
        true
    } else {
        cli.pulls <= DETAIL_THRESHOLD
    };

    let mut stats = Stats::new();
    for index in 1..=cli.pulls {
        let outcome = banner.pull();
        stats.record(index, outcome, show_detail);
    }

    let painter = Painter::new(color_enabled(cli.no_color));
    let report = render(&RenderInput {
        pulls: cli.pulls,
        seed,
        stats: &stats,
        end: EndState {
            pity5: banner.pity5(),
            pity4: banner.pity4(),
            guaranteed5: banner.guaranteed5(),
        },
        show_detail,
        painter: &painter,
    });
    print!("{report}");

    ExitCode::SUCCESS
}

/// 模拟结束时的卡池状态，仅用于展示。
#[derive(Clone, Copy, Debug)]
struct EndState {
    pity5: u32,
    pity4: u32,
    guaranteed5: bool,
}

struct RenderInput<'a> {
    pulls: u64,
    seed: u64,
    stats: &'a Stats,
    end: EndState,
    show_detail: bool,
    painter: &'a Painter,
}

fn render(input: &RenderInput<'_>) -> String {
    let p = input.painter;
    let stats = input.stats;
    let mut out = String::new();

    // ── 头部 ───────────────────────────────────────────────
    out.push_str(&format!(
        "{}\n",
        p.bold("=== 鸣潮 · 角色活动唤取 模拟 ===")
    ));
    out.push_str(&format!("随机种子 : {}\n", input.seed));
    out.push_str(&format!("抽卡次数 : {}\n\n", input.pulls));

    // ── 汇总 ───────────────────────────────────────────────
    out.push_str(&p.cyan(&rule("汇总")));
    out.push('\n');

    row(&mut out, "5★ 总数", stats.five_star_total());
    row(&mut out, "  限定 5★", stats.limited5);
    row(&mut out, "  常驻 5★(歪)", stats.standard5);
    row(&mut out, "  歪(50/50 失败)", format!("{} 次", stats.standard5));
    row(
        &mut out,
        "  综合出金率",
        with_theory(
            pct(stats.five_star_rate()),
            &format!("{:.2}%", gacha::theory::P5 * 100.0),
        ),
    );
    row(
        &mut out,
        "  平均出金抽数",
        with_theory(
            num2(stats.mean_pulls_per_5star()),
            &format!("{:.2}", gacha::theory::MEAN_PULLS_PER_5STAR),
        ),
    );
    out.push('\n');

    row(&mut out, "4★ 总数", stats.four);
    row(
        &mut out,
        "  4★ 出率",
        with_theory(
            pct(stats.four_star_rate()),
            &format!("{:.2}%", gacha::theory::P4 * 100.0),
        ),
    );
    row(
        &mut out,
        "  平均每 4★ 抽数",
        with_theory(
            num2(stats.mean_pulls_per_4star()),
            &format!("{:.2}", gacha::theory::MEAN_PULLS_PER_4STAR),
        ),
    );
    out.push('\n');

    row(
        &mut out,
        "3★ 武器",
        format!("{:<6}({})", stats.three, pct(stats.three_star_rate())),
    );
    out.push('\n');

    row(
        &mut out,
        "5★ 最欧 / 最非",
        match (stats.min_gap(), stats.max_gap()) {
            (Some(min), Some(max)) => format!("{min} 抽 / {max} 抽"),
            _ => "-".to_string(),
        },
    );
    out.push('\n');

    // ── 大珊瑚明细 ─────────────────────────────────────────
    out.push_str(&p.cyan(&rule("大珊瑚（余波珊瑚）明细")));
    out.push('\n');

    row(&mut out, "总获取量", stats.coral);
    row(
        &mut out,
        "  限定 5★",
        format!(
            "{} × {} = {:>4}",
            stats.limited5,
            gacha::CORAL_5STAR_LIMITED,
            stats.limited5 * u64::from(gacha::CORAL_5STAR_LIMITED)
        ),
    );
    row(
        &mut out,
        "  常驻 5★(歪)",
        format!(
            "{} × {} = {:>4}    ({} 基础 + {} 歪补偿)",
            stats.standard5,
            gacha::CORAL_5STAR_STANDARD,
            stats.standard5 * u64::from(gacha::CORAL_5STAR_STANDARD),
            gacha::CORAL_5STAR_LIMITED,
            gacha::CORAL_STANDARD_5STAR_BONUS
        ),
    );
    row(
        &mut out,
        "  4★（按满链）",
        format!(
            "{} × {} = {:>4}    (3 基础 + {} 满链转化)",
            stats.four,
            gacha::CORAL_4STAR,
            stats.four * u64::from(gacha::CORAL_4STAR),
            gacha::CORAL_4STAR_MAXED_BONUS
        ),
    );
    row(
        &mut out,
        "平均每抽",
        with_theory(
            num2(stats.coral_per_pull()),
            &format!("{:.2}", gacha::theory::CORAL_PER_PULL),
        ),
    );
    row(
        &mut out,
        "平均每 100 抽",
        with_theory(
            stats
                .coral_per_pull()
                .map_or("-".to_string(), |v| format!("{:.1}", v * 100.0)),
            &format!("{:.1}", gacha::theory::CORAL_PER_PULL * 100.0),
        ),
    );
    row(
        &mut out,
        "可兑换限定抽数",
        format!(
            "{} 抽   ({} ÷ {}，向下取整)",
            stats.exchangeable_pulls(),
            stats.coral,
            gacha::CORAL_PER_PULL_EXCHANGE
        ),
    );
    out.push('\n');

    // ── 明细 ───────────────────────────────────────────────
    if input.show_detail {
        out.push_str(&p.cyan(&rule("明细")));
        out.push('\n');
        if stats.details.is_empty() {
            out.push_str("（本次没有 5★ 或 4★ 产出）\n");
        } else {
            for d in &stats.details {
                let note = match d.item {
                    Item::Limited5 if d.used_guarantee => "   (歪后大保底)",
                    Item::Limited5 => "   (50/50 成功)",
                    Item::Standard5 => "   (50/50 失败 → 下次必限定)",
                    Item::Four | Item::Three => "",
                };
                out.push_str(&format!(
                    "#{:<3}第 {:>4} 抽   {} {:<4}大珊瑚{}\n",
                    d.seq,
                    d.pull_index,
                    pad_display(d.item.rarity_label(), 10),
                    format!("+{}", d.item.coral()),
                    note
                ));
            }
        }
        out.push('\n');
    }

    // ── 结束状态 ───────────────────────────────────────────
    out.push_str(&p.cyan(&rule("结束状态")));
    out.push('\n');
    row(
        &mut out,
        "5★ 保底计数",
        format!(
            "{}/{}（距硬保底还差 {} 抽）",
            input.end.pity5,
            gacha::P5_HARD_PITY,
            gacha::P5_HARD_PITY - input.end.pity5
        ),
    );
    row(
        &mut out,
        "下次 5★",
        if input.end.guaranteed5 {
            "大保底（必为限定）"
        } else {
            "非大保底（50/50）"
        },
    );
    row(
        &mut out,
        "4★ 保底计数",
        format!(
            "{}/{}（距 4★ 保底还差 {} 抽）",
            input.end.pity4,
            gacha::P4_HARD_PITY,
            gacha::P4_HARD_PITY - input.end.pity4
        ),
    );
    out.push('\n');

    // ── 与理论值对比 ───────────────────────────────────────
    out.push_str(&p.cyan(&rule("与理论值对比")));
    out.push('\n');

    let n = input.pulls as f64;
    let expected5 = gacha::theory::P5 * n;
    let deviation = if expected5 > 0.0 {
        format!(
            "      (偏差 {:+.1}%)",
            (stats.five_star_total() as f64 - expected5) / expected5 * 100.0
        )
    } else {
        String::new()
    };
    row(
        &mut out,
        "期望 5★ 总数",
        format!(
            "{:.2}   实际 {}{}",
            expected5,
            stats.five_star_total(),
            deviation
        ),
    );
    row(
        &mut out,
        "期望限定 5★",
        format!(
            "{:.2}   实际 {}",
            gacha::theory::P5_LIMITED * n,
            stats.limited5
        ),
    );
    row(
        &mut out,
        "期望 4★ 总数",
        format!("{:.1}   实际 {}", gacha::theory::P4 * n, stats.four),
    );
    row(
        &mut out,
        "期望大珊瑚",
        format!("{:.1}   实际 {}", gacha::theory::CORAL_PER_PULL * n, stats.coral),
    );

    out
}

/// 追加一行 `标签 : 值`，标签按显示宽度补齐。
fn row(out: &mut String, label: &str, value: impl std::fmt::Display) {
    out.push_str(&pad_display(label, LABEL_WIDTH));
    out.push_str(": ");
    out.push_str(&value.to_string());
    out.push('\n');
}

/// 拼出 `值` + 对齐的 `[理论 …]` 后缀。
fn with_theory(value: String, theory: &str) -> String {
    format!("{value:<8}[理论 {theory}]")
}

/// 百分比，两位小数；除数为 0 时显示 `-`。
fn pct(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |v| format!("{:.2}%", v * 100.0))
}

/// 普通数值，两位小数；除数为 0 时显示 `-`。
fn num2(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |v| format!("{v:.2}"))
}

/// 生成一条分区横线，使所有分区的显示宽度一致。
fn rule(title: &str) -> String {
    const TOTAL_WIDTH: usize = 46;
    let head = format!("── {title} ");
    let pad = TOTAL_WIDTH.saturating_sub(display_width(&head));
    format!("{head}{}", "─".repeat(pad))
}

/// 按终端显示宽度右侧补空格。
fn pad_display(text: &str, width: usize) -> String {
    let w = display_width(text);
    if w >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - w))
    }
}

/// 终端显示宽度：CJK 全角字符按 2 计，其余按 1 计。
///
/// 注意 `★`（U+2605）等 East Asian Ambiguous 字符按 1 计，与 `unicode-width`
/// 的默认行为一致。若终端把它渲染成双宽，含 `★` 的标签列会整体偏移一个空格，
/// 但不影响任何数值。
fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    let cp = c as u32;
    let wide = matches!(cp,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD
    );
    if wide { 2 } else { 1 }
}

/// 是否输出 ANSI 颜色。
///
/// 三重条件：未指定 `--no-color`、环境变量 `NO_COLOR` 未设置、且 stdout 是终端。
fn color_enabled(no_color: bool) -> bool {
    !no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

/// 极简着色器：关闭时原样返回。
struct Painter {
    enabled: bool,
}

impl Painter {
    fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }

    fn cyan(&self, text: &str) -> String {
        self.paint("36", text)
    }
}
