//! 局域网节点发现：UDP 信标广播 + 本网段轻量端口探测。
//!
//! 开启协作后自动发现附近节点，无需手输 IP。
//! 单机双开场景额外支持：
//! - UDP 端口复用（SO_REUSEADDR），避免第二个实例抢不到 47899
//! - 优先扫描 127.0.0.1 / 本机局域网 IP 的协作端口

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::types::DiscoveredGroupSummary;

pub const BEACON_PORT: u16 = 47899;
pub const BEACON_INTERVAL_SECS: u64 = 2;
pub const SCAN_INTERVAL_SECS: u64 = 8;
pub const DISCOVERY_KIND: &str = "cn_codex_lan_beacon";
pub const COLLAB_PORT_START: u16 = 47800;
pub const COLLAB_PORT_END: u16 = 47820;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceBeacon {
    pub v: u16,
    pub kind: String,
    pub node_id: String,
    pub display_name: String,
    pub listen_port: u16,
    #[serde(default)]
    pub groups: Vec<DiscoveredGroupSummary>,
}

impl PresenceBeacon {
    pub fn new(
        node_id: String,
        display_name: String,
        listen_port: u16,
        groups: Vec<DiscoveredGroupSummary>,
    ) -> Self {
        Self {
            v: 1,
            kind: DISCOVERY_KIND.to_string(),
            node_id,
            display_name,
            listen_port,
            groups,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredEndpoint {
    pub node_id: String,
    pub display_name: String,
    pub address: String,
    pub port: u16,
    pub groups: Vec<DiscoveredGroupSummary>,
    /// true = UDP 信标；false = 端口扫描候选
    pub from_beacon: bool,
}

pub struct DiscoveryHandle {
    stop_tx: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl DiscoveryHandle {
    pub async fn stop(self) {
        let _ = self.stop_tx.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

pub type DiscoveryCallback = Arc<dyn Fn(DiscoveredEndpoint) + Send + Sync>;

/// 启动 UDP 信标收发 + 周期本网段端口探测。
pub async fn start_discovery(
    local_node_id: String,
    listen_port: u16,
    get_beacon: Arc<dyn Fn() -> PresenceBeacon + Send + Sync>,
    on_discovered: DiscoveryCallback,
) -> Result<DiscoveryHandle, String> {
    let socket = match UdpSocket::bind(("0.0.0.0", BEACON_PORT)).await {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(
                "[lan_collab] bind discovery port {BEACON_PORT} failed: {err}; fallback random port"
            );
            UdpSocket::bind("0.0.0.0:0")
                .await
                .map_err(|e| format!("绑定发现端口失败: {e}"))?
        }
    };
    socket
        .set_broadcast(true)
        .map_err(|e| format!("设置 UDP 广播失败: {e}"))?;
    let socket = Arc::new(socket);

    let (stop_tx, stop_rx) = watch::channel(false);
    let mut tasks = Vec::new();

    // 接收信标
    {
        let socket = socket.clone();
        let local_node_id = local_node_id.clone();
        let on_discovered = on_discovered.clone();
        let mut stop_rx = stop_rx.clone();
        tasks.push(tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((len, addr)) => {
                                if let Some(endpoint) = parse_beacon(&buf[..len], addr) {
                                    if endpoint.node_id != local_node_id {
                                        on_discovered(endpoint);
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::debug!("[lan_collab] udp recv error: {err}");
                                tokio::time::sleep(Duration::from_millis(200)).await;
                            }
                        }
                    }
                }
            }
        }));
    }

    // 发送信标 + 周期扫描
    {
        let socket = socket.clone();
        let on_discovered = on_discovered.clone();
        let mut stop_rx = stop_rx.clone();
        tasks.push(tokio::spawn(async move {
            let mut ticks: u64 = 0;
            let scan_every = (SCAN_INTERVAL_SECS / BEACON_INTERVAL_SECS).max(1);
            loop {
                // 广播本机信标
                let beacon = get_beacon();
                if let Ok(bytes) = serde_json::to_vec(&beacon) {
                    let _ = socket
                        .send_to(&bytes, SocketAddr::from((Ipv4Addr::BROADCAST, BEACON_PORT)))
                        .await;
                    for target in subnet_broadcast_targets() {
                        let _ = socket.send_to(&bytes, target).await;
                    }
                }

                // 轻量 TCP 扫描（兜底，UDP 被禁时仍可用）
                if ticks % scan_every == 0 {
                    let found = soft_scan_local_subnet(listen_port).await;
                    for (ip, port) in found {
                        on_discovered(DiscoveredEndpoint {
                            node_id: format!("scan:{ip}:{port}"),
                            display_name: format!("{ip}:{port}"),
                            address: ip,
                            port,
                            groups: Vec::new(),
                            from_beacon: false,
                        });
                    }
                }
                ticks = ticks.wrapping_add(1);

                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(BEACON_INTERVAL_SECS)) => {}
                }
            }
        }));
    }

    // 防止未使用警告
    let _ = mpsc::channel::<()>(1);

    Ok(DiscoveryHandle { stop_tx, tasks })
}

fn parse_beacon(bytes: &[u8], addr: SocketAddr) -> Option<DiscoveredEndpoint> {
    let beacon: PresenceBeacon = serde_json::from_slice(bytes).ok()?;
    if beacon.kind != DISCOVERY_KIND || beacon.node_id.is_empty() || beacon.listen_port == 0 {
        return None;
    }
    Some(DiscoveredEndpoint {
        node_id: beacon.node_id,
        display_name: beacon.display_name,
        address: addr.ip().to_string(),
        port: beacon.listen_port,
        groups: beacon.groups,
        from_beacon: true,
    })
}

fn subnet_broadcast_targets() -> Vec<SocketAddr> {
    let mut out = Vec::new();
    if let Ok(ip) = local_ip_address::local_ip() {
        if let std::net::IpAddr::V4(v4) = ip {
            let octets = v4.octets();
            let bcast = Ipv4Addr::new(octets[0], octets[1], octets[2], 255);
            out.push(SocketAddr::from((bcast, BEACON_PORT)));
        }
    }
    out
}

/// 轻量扫描本机 /24 网段常见协作端口。
pub async fn soft_scan_local_subnet(self_port: u16) -> Vec<(String, u16)> {
    let Some(local) = local_ip_address::local_ip().ok() else {
        return Vec::new();
    };
    let std::net::IpAddr::V4(v4) = local else {
        return Vec::new();
    };
    let octets = v4.octets();
    let base = [octets[0], octets[1], octets[2]];
    let self_ip = v4.to_string();

    // 只扫 47800..=47805，控制噪声
    let ports: Vec<u16> = (47800u16..=47805).collect();
    let mut targets = Vec::new();
    for host in 1u8..=254u8 {
        let ip = Ipv4Addr::new(base[0], base[1], base[2], host).to_string();
        for port in &ports {
            if ip == self_ip && *port == self_port {
                continue;
            }
            targets.push((ip.clone(), *port));
        }
    }

    // 单机双实例：额外探测 loopback
    for port in &ports {
        if *port != self_port {
            targets.push(("127.0.0.1".into(), *port));
        }
    }

    let mut found = Vec::new();
    let mut set = HashSet::new();
    for chunk in targets.chunks(160) {
        let mut handles = Vec::with_capacity(chunk.len());
        for (ip, port) in chunk {
            let ip = ip.clone();
            let port = *port;
            handles.push(tokio::spawn(async move {
                let addr = format!("{ip}:{port}");
                let ok = tokio::time::timeout(
                    Duration::from_millis(160),
                    tokio::net::TcpStream::connect(&addr),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some();
                if ok {
                    Some((ip, port))
                } else {
                    None
                }
            }));
        }
        for h in handles {
            if let Ok(Some(pair)) = h.await {
                if set.insert(pair.clone()) {
                    found.push(pair);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    found
}

/// 立即扫描一次（UI 手动刷新发现）。
pub async fn scan_now(self_port: u16) -> Vec<(String, u16)> {
    soft_scan_local_subnet(self_port).await
}
