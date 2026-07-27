/*
 * ============================================================================
 * MODULE: p2p.rs — Rete P2P Mesh Cifrata (ChaCha20-Poly1305)
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Encrypted UDP Socket: Scambio pacchetti P2P su UDP con cifratura simmetrica.
 * 2. ChaCha20-Poly1305: Algoritmo di cifratura autenticata di grado militare.
 * 3. Arc & Mutex (`Arc<Mutex<T>>`): Condivisione thread-safe dello stato della flotta.
 * 4. Automatic Peer Pruning: Rimozione dei nodi inattivi da oltre 30 secondi.
 */

use std::collections::HashMap;
use std::net::SocketAddr;
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

/// Pacchetto di telemetria scambiato tra i nodi P2P
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTelemetryPacket {
    pub host: String,
    pub total_watts: f64,
    pub today_kwh: f64,
    pub today_cost: f64,
    pub alltime_kwh: f64,
    pub alltime_cost: f64,
    pub timestamp: u64,
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
}

/// Stato condiviso dell'intera flotta/cluster P2P
#[derive(Debug, Default)]
pub struct FleetClusterState {
    pub nodes: HashMap<String, RemoteNodeState>,
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

        let peers: Vec<String> = config.p2p_peers.clone();
        let p2p_port = config.p2p_port;

        let recv_socket = socket.clone();
        let recv_cipher = cipher.clone();
        let recv_state = cluster_state.clone();
        let local_host = config.host_label.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                if let Ok((len, _src)) = recv_socket.recv_from(&mut buf).await {
                    if len > 12 {
                        let (nonce_bytes, ciphertext) = buf[..len].split_at(12);
                        let nonce = Nonce::from_slice(nonce_bytes);

                        if let Ok(decrypted_bytes) = recv_cipher.decrypt(nonce, ciphertext) {
                            if let Ok(packet) = bincode::deserialize::<NodeTelemetryPacket>(&decrypted_bytes) {
                                if packet.host != local_host {
                                    let now_ts = chrono::Local::now().timestamp() as u64;
                                    let mut state = recv_state.lock().unwrap();
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

        let send_socket = socket.clone();
        let send_cipher = cipher;

        tokio::spawn(async move {
            while let Some(packet) = rx.recv().await {
                if let Ok(encoded_bytes) = bincode::serialize(&packet) {
                    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
                    if let Ok(ciphertext) = send_cipher.encrypt(&nonce, encoded_bytes.as_ref()) {
                        let mut payload = Vec::with_capacity(12 + ciphertext.len());
                        payload.extend_from_slice(nonce.as_slice());
                        payload.extend_from_slice(&ciphertext);

                        let broadcast_addr = format!("255.255.255.255:{}", p2p_port);
                        let _ = send_socket.send_to(&payload, &broadcast_addr).await;

                        for peer_addr_str in &peers {
                            let target = if peer_addr_str.contains(':') {
                                peer_addr_str.clone()
                            } else {
                                format!("{}:{}", peer_addr_str, p2p_port)
                            };
                            if let Ok(addr) = target.parse::<SocketAddr>() {
                                let _ = send_socket.send_to(&payload, addr).await;
                            }
                        }
                    }
                }
            }
        });

        Ok((tx, cluster_state))
    }
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
        };

        let bytes = bincode::serialize(&packet).expect("Serialization failed");
        let decoded: NodeTelemetryPacket = bincode::deserialize(&bytes).expect("Deserialization failed");

        assert_eq!(decoded.host, "TestHost");
        assert_eq!(decoded.total_watts, 45.5);
        assert_eq!(decoded.today_kwh, 1.23);
    }

    #[test]
    fn test_p2p_encryption_decryption_cycle() {
        let secret = "my_p2p_secret_key";
        let key_bytes = derive_256bit_key(secret);
        let cipher = ChaCha20Poly1305::new(&key_bytes.into());

        let packet = NodeTelemetryPacket {
            host: "RemoteServer".to_string(),
            total_watts: 120.0,
            today_kwh: 2.50,
            today_cost: 0.75,
            alltime_kwh: 50.0,
            alltime_cost: 15.0,
            timestamp: 1700000000,
        };

        let encoded_bytes = bincode::serialize(&packet).unwrap();
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, encoded_bytes.as_ref()).unwrap();

        let decrypted_bytes = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();
        let decoded_packet: NodeTelemetryPacket = bincode::deserialize(&decrypted_bytes).unwrap();

        assert_eq!(decoded_packet.host, "RemoteServer");
        assert_eq!(decoded_packet.total_watts, 120.0);
    }

    #[test]
    fn test_fleet_cluster_state_totals_and_pruning() {
        let mut state = FleetClusterState::default();
        let now_ts = 1000u64;

        state.nodes.insert(
            "NodeA".to_string(),
            RemoteNodeState {
                host: "NodeA".to_string(),
                total_watts: 30.0,
                today_kwh: 0.5,
                today_cost: 0.15,
                alltime_kwh: 10.0,
                alltime_cost: 3.0,
                last_seen: now_ts,
            },
        );

        state.nodes.insert(
            "OldNode".to_string(),
            RemoteNodeState {
                host: "OldNode".to_string(),
                total_watts: 100.0,
                today_kwh: 5.0,
                today_cost: 1.5,
                alltime_kwh: 200.0,
                alltime_cost: 60.0,
                last_seen: now_ts - 40, // 40s ago (inactive)
            },
        );

        let (total_w, _total_kwh, active_count) = state.compute_cluster_totals("LocalNode", 50.0, 1.0);
        assert_eq!(active_count, 3);
        assert_eq!(total_w, 180.0);

        state.prune_inactive_nodes(now_ts);
        assert_eq!(state.nodes.len(), 1);
        assert!(state.nodes.contains_key("NodeA"));
        assert!(!state.nodes.contains_key("OldNode"));
    }
}
