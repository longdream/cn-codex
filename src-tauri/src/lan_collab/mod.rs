//! 局域网协作（弱中心 Owner + P2P 业务面）
//!
//! 控制面：Group Owner 作为组元数据 / 成员 / 权限权威。
//! 业务面：聊天、模型代理、知识拉取保持节点间直连。
//!
//! 首期：节点身份 + 本地状态 + 启停骨架；发现 / 传输 / 共享后续迭代。
//! Phase 1.5：TCP 监听、手动 IP 连接、远端 Owner 入组、跨节点聊天。

pub mod commands;
pub mod discovery;
pub mod identity;
pub mod knowledge_share;
pub mod model_share;
pub mod skill_share;
pub mod protocol;
pub mod runtime;
pub mod store;
pub mod types;

pub use commands::*;
pub use runtime::LanCollabRuntime;
pub use types::*;
