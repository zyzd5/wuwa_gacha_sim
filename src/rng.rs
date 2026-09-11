//! 可复现随机源。
//!
//! 使用 [`ChaCha8Rng`] 而**不是** `rand::rngs::StdRng`：后者的算法不保证跨 rand
//! 版本稳定，一旦升级依赖，同一个种子就会给出不同结果，破坏「同 seed 同结果」的承诺。
//! ChaCha8 的输出由算法规范完全确定，与平台、版本无关。

use std::time::{SystemTime, UNIX_EPOCH};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// 用给定种子构造随机源。
///
/// 同一 seed 必然产生同一串随机数，因此在同一编译产物上结果完全可复现。
#[must_use]
pub fn seeded(seed: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed)
}

/// 未指定 `--seed` 时使用的种子：当前系统时间的纳秒数。
///
/// 程序会把实际使用的种子打印出来，方便之后复现这一次模拟。
#[must_use]
pub fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
}
