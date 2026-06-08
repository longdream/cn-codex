//! 用量追踪模块
//! 使用 SQLite 持久化存储每次 LLM 调用的 token 使用量和费用信息。
//! 提供查询接口：汇总统计、历史记录、按供应商/模型分组等。

pub mod db;
pub mod recorder;
pub mod pricing;

pub use db::UsageDb;
pub use recorder::UsageRecorder;
pub use pricing::PricingTable;
