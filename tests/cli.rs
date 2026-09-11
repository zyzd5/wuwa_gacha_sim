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
    assert!(out.stdout.contains("可兑换限定抽数"), "缺少大珊瑚明细");
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
