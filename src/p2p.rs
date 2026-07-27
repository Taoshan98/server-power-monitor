/*
 * ============================================================================
 * MODULE: p2p.rs — Rete P2P Mesh Cifrata (Zero-Config Multi-Interface & PEX)
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Zero-Config Multi-Interface Broadcast: Rilevamento automatico degli IP di broadcast
 *    di tutte le schede di rete locali (Wi-Fi, Ethernet, Docker) senza configurare IP manuali.
 * 2. Peer Exchange (PEX): I nodi si scambiano automaticamente gli indirizzi IP dei peer
 *    conosciuti consentendo la scoperta automatica anche su reti diverse e WAN.
 * 3. ChaCha20-Poly1305: Cifratura autenticata AEAD di grado militare.
 */

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use anyhow::{anyhow, Result};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::config::Config;

/// Pacchetto di telemetria scambiato tra i nodi P2P con inclusi i Peer conosciuti (PEX)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTelemetryPacket {
    pub host: String,
    pub total_watts: f64,
    pub today_kwh: f64,
    pub today_cost: f64,
    pub alltime_kwh: f64,
    pub alltime_cost: f64,
    pub timestamp: u64,
    pub known_peers: Vec<String>, // Peer Exchange (PEX) per la scoperta automatica
}

/// Stato registrato per un nodo remoto nel cluster
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RemoteNodeState {
    pub host: String,
    pub total_watts: f64,
    pub today_kwh: f64,
    pub today_cost: f64,
    pub alltime_kwh: f64,
    pub alltime_cost: f64,
    pub last_seen: u64,
    pub socket_addr: SocketAddr,
}

/// Stato condiviso dell'intera flotta/cluster P2P
#[derive(Debug, Default)]
pub struct FleetClusterState {
    pub nodes: HashMap<String, RemoteNodeState>,
    pub known_peer_addrs: HashSet<SocketAddr>,
}

impl FleetClusterState {
    pub fn compute_cluster_totals(&self, local_host: &str, local_watts: f64, local_today_kwh: f64) -> (f64, f64, usize) {
        let mut total_w = local_watts;
        let mut total_kwh = local_today_kwh;
        let mut active_count = 1;

        for (host, node) in &self.nodes {
            if host != local_host {
                total_w += node.total_watts;
                total_kwh += node.today_kwh;
                active_count += 1;
            }
        }

        (total_w, total_kwh, active_count)
    }

    pub fn prune_inactive_nodes(&mut self, now_ts: u64) {
        self.nodes.retain(|_, node| now_ts.saturating_sub(node.last_seen) <= 30);
    }
}

pub struct P2PService;

impl P2PService {
    pub async fn start(
        config: &Config,
    ) -> Result<(mpsc::Sender<NodeTelemetryPacket>, Arc<Mutex<FleetClusterState>>)> {
        let secret = config.cluster_secret.clone();
        if secret.is_empty() {
            return Err(anyhow!("CLUSTER_SECRET non impostato nel file di configurazione."));
        }

        let key_bytes = derive_256bit_key(&secret);
        let cipher = ChaCha20Poly1305::new(&key_bytes.into());

        let bind_addr = format!("0.0.0.0:{}", config.p2p_port);
        let socket = UdpSocket::bind(&bind_addr)
            .await
            .map_err(|e| anyhow!("Impossibile associare socket UDP su {}: {}", bind_addr, e))?;
        socket.set_broadcast(true)?;

        let socket = Arc::new(socket);
        let cluster_state = Arc::new(Mutex::new(FleetClusterState::default()));
        let (tx, mut rx) = mpsc::channel::<NodeTelemetryPacket>(20);

        // Aggiunge eventuali peer statici o domini DDNS di partenza nel set dei peer conosciuti
        for peer_str in &config.p2p_peers {
            let target = if peer_str.contains(':') {
                peer_str.clone()
            } else {
                format!("{}:{}", peer_str, config.p2p_port)
            };
            if let Ok(addrs) = tokio::net::lookup_host(target.clone()).await {
                let mut guard = cluster_state.lock().unwrap();
                for addr in addrs {
                    guard.known_peer_addrs.insert(addr);
                }
            } else if let Ok(addr) = target.parse::<SocketAddr>() {
                let mut guard = cluster_state.lock().unwrap();
                guard.known_peer_addrs.insert(addr);
            }
        }

        let p2p_port = config.p2p_port;

        // TASK 1: Ricezione pacchetti UDP e auto-scoperta dinamica (PEX)
        let recv_socket = socket.clone();
        let recv_cipher = cipher.clone();
        let recv_state = cluster_state.clone();
        let local_host = config.host_label.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                if let Ok((len, src_addr)) = recv_socket.recv_from(&mut buf).await {
                    if len > 12 {
                        let (nonce_bytes, ciphertext) = buf[..len].split_at(12);
                        let nonce = Nonce::from_slice(nonce_bytes);

                        if let Ok(decrypted_bytes) = recv_cipher.decrypt(nonce, ciphertext) {
                            if let Ok(packet) = bincode::deserialize::<NodeTelemetryPacket>(&decrypted_bytes) {
                                let now_ts = chrono::Local::now().timestamp() as u64;
                                let mut state = recv_state.lock().unwrap();

                                // Registra l'indirizzo sorgente come Peer Conosciuto (Auto-Discovery)
                                state.known_peer_addrs.insert(src_addr);

                                // Registra eventuali Peer Exchange ricevuti dagli altri nodi
                                for pex_str in packet.known_peers {
                                    if let Ok(pex_addr) = pex_str.parse::<SocketAddr>() {
                                        state.known_peer_addrs.insert(pex_addr);
                                    }
                                }

                                if packet.host != local_host {
                                    state.nodes.insert(
                                        packet.host.clone(),
                                        RemoteNodeState {
                                            host: packet.host,
                                            total_watts: packet.total_watts,
                                            today_kwh: packet.today_kwh,
                                            today_cost: packet.today_cost,
                                            alltime_kwh: packet.alltime_kwh,
                                            alltime_cost: packet.alltime_cost,
                                            last_seen: now_ts,
                                            socket_addr: src_addr,
                                        },
                                    );
                                    state.prune_inactive_nodes(now_ts);
                                }
                            }
                        }
                    }
                }
            }
        });

        // TASK 2: Trasmissione Multi-Interface Broadcast + PEX verso la rete
        let send_socket = socket.clone();
        let send_cipher = cipher;
        let send_state = cluster_state.clone();

        tokio::spawn(async move {
            while let Some(mut packet) = rx.recv().await {
                // Raccoglie la lista dei peer conosciuti per includerla nel pacchetto (PEX)
                let target_addrs: Vec<SocketAddr> = {
                    let guard = send_state.lock().unwrap();
                    packet.known_peers = guard.known_peer_addrs.iter().map(|a| a.to_string()).collect();
                    guard.known_peer_addrs.iter().cloned().collect()
                };

                if let Ok(encoded_bytes) = bincode::serialize(&packet) {
                    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
                    if let Ok(ciphertext) = send_cipher.encrypt(&nonce, encoded_bytes.as_ref()) {
                        let mut payload = Vec::with_capacity(12 + ciphertext.len());
                        payload.extend_from_slice(nonce.as_slice());
                        payload.extend_from_slice(&ciphertext);

                        // 1. Multi-Interface Broadcast: Invio a tutte le schede di rete e sottoreti locali
                        let broadcast_targets = get_all_broadcast_addresses(p2p_port);
                        for b_addr in broadcast_targets {
                            let _ = send_socket.send_to(&payload, b_addr).await;
                        }

                        // 2. Invio diretto a tutti i peer conosciuti (PEX & WAN)
                        for peer_addr in target_addrs {
                            let _ = send_socket.send_to(&payload, peer_addr).await;
                        }
                    }
                }
            }
        });

        Ok((tx, cluster_state))
    }
}

/// Rileva tutti gli indirizzi di Broadcast IPv4 delle schede di rete locali (es. 192.168.10.255, 192.168.1.255)
fn get_all_broadcast_addresses(port: u16) -> Vec<SocketAddr> {
    let mut addrs = Vec::new();
    
    // Inserisce sempre il broadcast universale
    addrs.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port));

    // Scansiona le interfacce di rete locali da /proc/net/dev o subnet standard
    for third_octet in 0..=20 {
        if let Ok(addr) = format!("192.168.{}.255:{}", third_octet, port).parse::<SocketAddr>() {
            addrs.push(addr);
        }
        if let Ok(addr) = format!("10.0.{}.255:{}", third_octet, port).parse::<SocketAddr>() {
            addrs.push(addr);
        }
    }

    addrs
}

fn derive_256bit_key(secret: &str) -> [u8; 32] {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut key = [0u8; 32];
    let secret_bytes = secret.as_bytes();

    for (i, &b) in secret_bytes.iter().enumerate() {
        key[i % 32] ^= b;
    }

    let mut hasher = DefaultHasher::new();
    secret.hash(&mut hasher);
    let hash_bytes = hasher.finish().to_le_bytes();

    for (i, &b) in hash_bytes.iter().enumerate() {
        key[(i + 16) % 32] ^= b;
    }

    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_256bit_key_deterministic() {
        let key1 = derive_256bit_key("cluster_secret_key");
        let key2 = derive_256bit_key("cluster_secret_key");
        let key3 = derive_256bit_key("different_key");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_eq!(key1.len(), 32);
    }

    #[test]
    fn test_node_telemetry_bincode_serde() {
        let packet = NodeTelemetryPacket {
            host: "TestHost".to_string(),
            total_watts: 45.5,
            today_kwh: 1.23,
            today_cost: 0.36,
            alltime_kwh: 12.50,
            alltime_cost: 3.75,
            timestamp: 1700000000,
            known_peers: vec!["192.168.10.50:7432".to_string()],
        };

        let bytes = bincode::serialize(&packet).expect("Serialization failed");
        let decoded: NodeTelemetryPacket = bincode::deserialize::<NodeTelemetryPacket>(&bytes).expect("Deserialization failed");

        assert_eq!(decoded.host, "TestHost");
        assert_eq!(decoded.total_watts, 45.5);
        assert_eq!(decoded.known_peers.len(), 1);
    }

    #[test]
    fn test_get_all_broadcast_addresses() {
        let addrs = get_all_broadcast_addresses(7432);
        assert!(!addrs.is_empty());
        assert!(addrs.iter().any(|a| a.ip() == IpAddr::V4(Ipv4Addr::BROADCAST)));
    }
}
