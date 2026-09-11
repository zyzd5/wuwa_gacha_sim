//! 命令行集成测试：直接拉起二进制，验证退出码与关键输出。
//!
//! 这里覆盖的是「程序行为」层面的要求（退出码、明细阈值、可复现性），
//! 概率模型本身由 `src/tests.rs` 的单元测试覆盖。

use std::process::Command;

/// Cargo 注入的二进制路径。
const BIN: &str = env!("CARGO_BIN_EXE_wuwa_gacha_sim");

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Output {
    let output = Command::new(BIN)
        .args(args)
        .output()
        .expect("无法启动 wuwa_gacha_sim");
    Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn zero_pulls_succeeds_and_prints_dashes() {
    let out = run(&["-n", "0"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("抽卡次数 : 0"));
    // 除数为 0 时必须是 '-'，不能出现 NaN / inf。
    assert!(!out.stdout.contains("NaN"), "输出里出现了 NaN");
    assert!(!out.stdout.contains("inf"), "输出里出现了 inf");
    assert!(out.stdout.contains("大珊瑚（余波珊瑚）明细"), "缺少大珊瑚明细");
}

#[test]
fn detail_section_is_printed_last() {
    let out = run(&["-n", "200", "--seed", "42", "--no-color"]);
    let detail = out.stdout.find("── 明细 ").expect("应打印明细");
    for earlier in [
        "── 汇总 ",
        "大珊瑚（余波珊瑚）明细",
        "── 结束状态 ",
        "── 与理论值对比 ",
    ] {
        let pos = out
            .stdout
            .find(earlier)
            .unwrap_or_else(|| panic!("缺少 {earlier}"));
        assert!(pos < detail, "{earlier} 应排在明细之前");
    }
}

#[test]
fn removed_rows_are_gone() {
    let out = run(&["-n", "200", "--seed", "42", "--no-color"]);
    // 「歪(50/50 失败)」与「常驻 5★(歪)」恒为同一个数，只保留后者。
    assert!(!out.stdout.contains("50/50 失败) :"), "冗余的歪行应已删除");
    // 3★ 武器是无用的抽卡副产物，不再展示。
    assert!(!out.stdout.contains("3★ 武器"), "3★ 武器行应已删除");
    assert!(!out.stdout.contains("最欧"), "最欧/最非行应已删除");
    assert!(
        !out.stdout.contains("可兑换限定抽数"),
        "可兑换限定抽数行应已删除"
    );
}

#[test]
fn negative_pulls_exits_with_code_2() {
    let out = run(&["-n", "-1"]);
    assert_eq!(out.code, 2, "非法抽卡数应退出码 2，实际 {}", out.code);
}

#[test]
fn non_numeric_pulls_exits_with_code_2() {
    let out = run(&["-n", "abc"]);
    assert_eq!(out.code, 2);
}

#[test]
fn missing_pulls_exits_with_code_2() {
    let out = run(&[]);
    assert_eq!(out.code, 2);
}

#[test]
fn unknown_flag_exits_with_code_2() {
    let out = run(&["-n", "10", "--not-a-flag"]);
    assert_eq!(out.code, 2);
}

#[test]
fn same_seed_produces_identical_stdout() {
    let a = run(&["-n", "200", "--seed", "42", "--no-color"]);
    let b = run(&["-n", "200", "--seed", "42", "--no-color"]);
    assert_eq!(a.code, 0);
    assert_eq!(a.stdout, b.stdout, "同 seed 两次运行输出必须完全一致");
}

#[test]
fn detail_is_printed_by_default_for_small_pulls() {
    let out = run(&["-n", "200", "--seed", "42", "--no-color"]);
    assert!(out.stdout.contains("── 明细 "));
    assert!(out.stdout.contains("大珊瑚（余波珊瑚）明细"));
}

#[test]
fn detail_is_suppressed_above_the_threshold() {
    let out = run(&["-n", "500", "--seed", "42", "--no-color"]);
    assert!(
        !out.stdout.contains("── 明细 "),
        "超过 200 抽时默认不应打印明细"
    );
    assert!(out.stdout.contains("── 汇总 "));
}

#[test]
fn detail_can_be_forced_and_suppressed() {
    let forced = run(&["-n", "500", "--seed", "42", "--detail", "--no-color"]);
    assert!(forced.stdout.contains("── 明细 "));

    let suppressed = run(&["-n", "20", "--seed", "42", "--summary-only", "--no-color"]);
    assert!(!suppressed.stdout.contains("── 明细 "));
}

#[test]
fn detail_and_summary_only_conflict() {
    let out = run(&["-n", "20", "--detail", "--summary-only"]);
    assert_eq!(out.code, 2);
}

#[test]
fn no_color_emits_no_escape_sequences() {
    let out = run(&["-n", "20", "--seed", "42", "--no-color"]);
    assert!(!out.stdout.contains('\u{1b}'), "--no-color 下不应有 ANSI 转义");
}

#[test]
fn start_of_run_state_is_always_fresh() {
    // 程序不接受任何初始状态参数，且每次运行都从全新状态开始。
    let out = run(&["-n", "200", "--seed", "42", "--no-color"]);
    assert!(out.stdout.contains("保底计数"), "缺少结束状态");
    // 不存在任何续跑参数
    for flag in ["--init-pity", "--init-guarantee", "--init-pity4", "--init-guarantee4", "--trials"] {
        let rejected = run(&["-n", "10", flag]);
        assert_eq!(rejected.code, 2, "{flag} 不应被接受");
    }
}

// ─────────────────────── --light 与 --repeat ───────────────────────

/// 从 `--light` 的一行里取出 (限定, 常驻)。
fn parse_light_line(line: &str) -> (u64, u64) {
    let after = line
        .split_once("限定 ")
        .unwrap_or_else(|| panic!("light 行缺少「限定」: {line:?}"))
        .1;
    let (limited, rest) = after
        .split_once(" 常驻 ")
        .unwrap_or_else(|| panic!("light 行缺少「常驻」: {line:?}"));
    let standard = rest.split_whitespace().next().expect("缺少常驻数量");
    (
        limited.trim().parse().expect("限定数应为整数"),
        standard.parse().expect("常驻数应为整数"),
    )
}

/// 从完整输出里取出某个汇总字段的数值（取第一次出现的那行）。
fn summary_field(stdout: &str, label: &str) -> u64 {
    stdout
        .lines()
        .find(|line| line.contains(label))
        .and_then(|line| line.rsplit_once(':'))
        .map(|(_, value)| value.trim().parse().expect("汇总字段应为整数"))
        .unwrap_or_else(|| panic!("找不到汇总字段 {label:?}"))
}

fn light_lines(stdout: &str) -> Vec<(u64, u64)> {
    stdout
        .lines()
        .filter(|line| line.contains("限定 ") && line.contains("seed="))
        .map(parse_light_line)
        .collect()
}

#[test]
fn light_prints_only_the_two_counts() {
    let out = run(&["-n", "200", "--seed", "42", "--light", "--no-color"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    // 只应有表头 + 一行结果
    let body: Vec<&str> = out
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(body.len(), 3, "light 单次运行应只有 3 行非空输出: {body:?}");

    // 完整模式里的那些区块都不该出现
    for absent in ["── 汇总 ", "大珊瑚", "结束状态", "与理论值对比", "── 明细 ", "4★"] {
        assert!(!out.stdout.contains(absent), "light 输出不应包含 {absent:?}");
    }

    let counts = light_lines(&out.stdout);
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0], (1, 1), "seed=42 的 200 抽应为 限定 1 / 常驻 1");
}

#[test]
fn light_agrees_with_full_output_for_the_same_seed() {
    // --light 只影响渲染，绝不能影响随机流消耗。
    for seed in ["42", "7", "1234", "999999"] {
        let full = run(&["-n", "200", "--seed", seed, "--no-color"]);
        let light = run(&["-n", "200", "--seed", seed, "--light", "--no-color"]);

        let expected = (
            summary_field(&full.stdout, "  限定 5★"),
            summary_field(&full.stdout, "  常驻 5★"),
        );
        assert_eq!(
            light_lines(&light.stdout),
            vec![expected],
            "seed={seed} 时 light 与完整模式结果不一致"
        );
    }
}

#[test]
fn repeat_runs_the_simulation_k_times() {
    let out = run(&["-n", "200", "--seed", "42", "--light", "--repeat", "8", "--no-color"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let lines = light_lines(&out.stdout);
    assert_eq!(lines.len(), 8, "应输出 8 行结果");

    // 每次都是独立模拟：种子依次递增，所以结果不应全都一样。
    assert!(
        lines.iter().any(|line| *line != lines[0]),
        "8 次独立模拟的结果不应完全相同: {lines:?}"
    );

    let result_lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|line| line.contains("seed="))
        .collect();
    for (i, line) in result_lines.iter().enumerate() {
        assert!(
            line.contains(&format!("seed={}", 42 + i)),
            "第 {} 行应使用 seed={}: {line:?}",
            i + 1,
            42 + i
        );
    }
}

#[test]
fn loop_is_an_alias_for_repeat() {
    let a = run(&["-n", "100", "--seed", "5", "--light", "--repeat", "3", "--no-color"]);
    let b = run(&["-n", "100", "--seed", "5", "--light", "--loop", "3", "--no-color"]);
    assert_eq!(a.code, 0);
    assert_eq!(a.stdout, b.stdout, "--loop 应与 --repeat 等价");
}

#[test]
fn repeat_zero_is_rejected() {
    let out = run(&["-n", "10", "--repeat", "0"]);
    assert_eq!(out.code, 2, "--repeat 0 应退出码 2");
}

#[test]
fn repeat_suppresses_detail_by_default_in_full_mode() {
    let out = run(&["-n", "200", "--seed", "42", "--repeat", "3", "--no-color"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.matches("── 第 ").count(),
        3,
        "完整模式下每次重复应有一个分节标题"
    );
    assert!(
        !out.stdout.contains("── 明细 "),
        "重复多次时默认不应打印明细"
    );

    let forced = run(&["-n", "200", "--seed", "42", "--repeat", "2", "--detail", "--no-color"]);
    assert!(forced.stdout.contains("── 明细 "), "--detail 应能强制打开明细");
}

#[test]
fn single_run_output_is_unchanged_by_the_new_options() {
    // 不带任何新参数时，输出必须与之前完全一致（含 随机种子 那一行）。
    let out = run(&["-n", "200", "--seed", "42", "--no-color"]);
    assert!(out.stdout.contains("随机种子 : 42"));
    assert!(out.stdout.contains("抽卡次数 : 200"));
    assert!(!out.stdout.contains("重复次数"));
    assert!(!out.stdout.contains("基准种子"));
}
