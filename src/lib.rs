//! 鸣潮「角色活动唤取」限定 5★ 角色抽卡模拟器。
//!
//! 本 crate 同时提供库与命令行程序：
//!
//! - [`gacha`]：概率模型、保底状态机、大珊瑚结算。**所有概率与珊瑚常量都在这里集中定义。**
//! - [`rng`]：可复现随机源。
//! - [`stats`]：统计聚合。
//!
//! 概率模型与建模假设的完整说明见 `README.md`。

pub mod gacha;
pub mod rng;
pub mod stats;

#[cfg(test)]
mod tests;
