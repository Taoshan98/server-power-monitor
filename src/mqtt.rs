/*
 * ============================================================================
 * MODULE: mqtt.rs — Client MQTT & Home Assistant Auto-Discovery
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Async Client MQTT (`rumqttc`): Client MQTT non bloccante in esecuzione su Tokio.
 * 2. Home Assistant Auto-Discovery: Pubblicazione di payload JSON di configurazione
 *    sul topic `homeassistant/sensor/.../config`.
 * 3. Serde JSON Serialization: Conversione automatica di struct Rust in JSON.
 */

use std::collections::HashMap;
use std::time::Duration;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::config::Config;

/// Struct per pubblicare aggiornamenti di stato su canale interno verso la task MQTT
#[derive(Debug, Clone, Serialize)]
pub struct MqttStatePayload {
    pub host: String,
    pub total_watts: f64,
    pub today_kwh: f64,
    pub today_cost: f64,
    pub alltime_kwh: f64,
    pub alltime_cost: f64,
    pub currency: String,
    pub sensors: HashMap<String, f64>,
}

pub struct MqttService {
    client: AsyncClient,
    topic_prefix: String,
    discovery_sent: bool,
}

impl MqttService {
    pub fn start(config: &Config) -> Result<mpsc::Sender<MqttStatePayload>> {
        let clean_host = config.host_label.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "_");
        let client_id = format!("spm_{}", clean_host);

        let mut mqttoptions = MqttOptions::new(client_id, &config.mqtt_host, config.mqtt_port);
        mqttoptions.set_keep_alive(Duration::from_secs(30));

        if !config.mqtt_username.is_empty() {
            mqttoptions.set_credentials(&config.mqtt_username, &config.mqtt_password);
        }

        let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);
        let (tx, mut rx) = mpsc::channel::<MqttStatePayload>(20);

        let mut service = MqttService {
            client,
            topic_prefix: config.mqtt_topic_prefix.clone(),
            discovery_sent: false,
        };

        let host_label = config.host_label.clone();
        tokio::spawn(async move {
            let event_task_client = service.client.clone();

            tokio::select! {
                _ = async {
                    loop {
                        if let Err(e) = eventloop.poll().await {
                            eprintln!("⚠️ Connessione MQTT disconnessa, riprovo: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                } => {}
                _ = async {
                    while let Some(payload) = rx.recv().await {
                        if !service.discovery_sent {
                            let _ = service.publish_home_assistant_discovery(&payload, &host_label).await;
                            service.discovery_sent = true;
                        }

                        let payload_host = payload.host.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "_");
                        let topic = format!("{}/{}/state", service.topic_prefix, payload_host);

                        if let Ok(json_str) = serde_json::to_string(&payload) {
                            let _ = event_task_client
                                .publish(&topic, QoS::AtLeastOnce, false, json_str)
                                .await;
                        }
                    }
                } => {}
            }
        });

        Ok(tx)
    }

    async fn publish_home_assistant_discovery(&self, payload: &MqttStatePayload, host_label: &str) -> Result<()> {
        let clean_host = host_label.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "_");
        let state_topic = format!("{}/{}/state", self.topic_prefix, clean_host);

        let device_info = serde_json::json!({
            "identifiers": [format!("spm_{}", clean_host)],
            "name": format!("Server Power Monitor ({})", host_label),
            "model": "Power Telemetry Agent (Rust)",
            "manufacturer": "Server Power Monitor"
        });

        let total_config = serde_json::json!({
            "name": format!("{} Total Power", host_label),
            "unique_id": format!("spm_{}_total_watts", clean_host),
            "state_topic": state_topic,
            "value_template": "{{ value_json.total_watts }}",
            "unit_of_measurement": "W",
            "device_class": "power",
            "state_class": "measurement",
            "device": device_info
        });
        self.publish_disc_sensor(&clean_host, "total_watts", &total_config).await?;

        let today_kwh_config = serde_json::json!({
            "name": format!("{} Energy Today", host_label),
            "unique_id": format!("spm_{}_today_kwh", clean_host),
            "state_topic": state_topic,
            "value_template": "{{ value_json.today_kwh }}",
            "unit_of_measurement": "kWh",
            "device_class": "energy",
            "state_class": "total_increasing",
            "device": device_info
        });
        self.publish_disc_sensor(&clean_host, "today_kwh", &today_kwh_config).await?;

        let lifetime_kwh_config = serde_json::json!({
            "name": format!("{} Lifetime Energy", host_label),
            "unique_id": format!("spm_{}_lifetime_kwh", clean_host),
            "state_topic": state_topic,
            "value_template": "{{ value_json.alltime_kwh }}",
            "unit_of_measurement": "kWh",
            "device_class": "energy",
            "state_class": "total_increasing",
            "device": device_info
        });
        self.publish_disc_sensor(&clean_host, "lifetime_kwh", &lifetime_kwh_config).await?;

        for (sensor_id, _) in &payload.sensors {
            let clean_sensor_id = sensor_id.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "_");
            let sensor_config = serde_json::json!({
                "name": format!("{} Power {}", host_label, sensor_id),
                "unique_id": format!("spm_{}_{}", clean_host, clean_sensor_id),
                "state_topic": state_topic,
                "value_template": format!("{{{{ value_json.sensors.{} }}}}", sensor_id),
                "unit_of_measurement": "W",
                "device_class": "power",
                "state_class": "measurement",
                "device": device_info
            });
            self.publish_disc_sensor(&clean_host, &clean_sensor_id, &sensor_config).await?;
        }

        println!("🏠 MQTT Home Assistant Auto-Discovery inviato con successo per {}.", host_label);
        Ok(())
    }

    async fn publish_disc_sensor(&self, clean_host: &str, clean_sensor_id: &str, payload_json: &serde_json::Value) -> Result<()> {
        let disc_topic = format!(
            "homeassistant/sensor/server-power-monitor_{}_{}/config",
            clean_host, clean_sensor_id
        );
        self.client
            .publish(&disc_topic, QoS::AtLeastOnce, true, payload_json.to_string())
            .await
            .context("Errore nella pubblicazione Auto-Discovery MQTT")?;
        Ok(())
    }
}
