use anyhow::{Context, Result, bail};
use chrono::{TimeZone, Utc};
use hmac::{Hmac, Mac};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::system::models::LighthouseState;

type HmacSha256 = Hmac<Sha256>;

const ACTION: &str = "DescribeInstancesTrafficPackages";
const ALGORITHM: &str = "TC3-HMAC-SHA256";
const SERVICE: &str = "lighthouse";
const VERSION: &str = "2020-03-24";
const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";

#[derive(Debug, Clone)]
pub struct LighthouseReader {
    client: Client,
    secret_id: String,
    secret_key: String,
    session_token: Option<String>,
    endpoint: String,
    region: String,
    instance_id: String,
}

impl LighthouseReader {
    pub fn new(config: &Config) -> Result<Option<Self>> {
        if !config.lighthouse_enabled {
            return Ok(None);
        }

        let client = Client::builder()
            .user_agent(format!(
                "{}/{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .context("failed to build Tencent Cloud Lighthouse HTTP client")?;

        let secret_id =
            required_config_value(config.lighthouse_secret_id.as_ref(), "lighthouse.secret_id")?;
        let secret_key = required_config_value(
            config.lighthouse_secret_key.as_ref(),
            "lighthouse.secret_key",
        )?;
        let region = required_config_value(config.lighthouse_region.as_ref(), "lighthouse.region")?;
        let instance_id = required_config_value(
            config.lighthouse_instance_id.as_ref(),
            "lighthouse.instance_id",
        )?;

        Ok(Some(Self {
            client,
            secret_id,
            secret_key,
            session_token: config.lighthouse_session_token.clone(),
            endpoint: config.lighthouse_endpoint.clone(),
            region,
            instance_id,
        }))
    }

    pub async fn read(&self) -> Result<Option<LighthouseState>> {
        let payload = build_payload(&self.instance_id);
        let payload_text =
            serde_json::to_string(&payload).context("failed to serialize lighthouse payload")?;

        let timestamp = Utc::now().timestamp();
        let date = Utc
            .timestamp_opt(timestamp, 0)
            .single()
            .context("failed to construct UTC timestamp")?
            .format("%Y-%m-%d")
            .to_string();

        let authorization = build_authorization(
            &self.secret_id,
            &self.secret_key,
            &self.endpoint,
            &payload_text,
            timestamp,
            &date,
        )?;

        let url = format!("https://{}/", self.endpoint);
        let mut request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            .header(HOST, &self.endpoint)
            .header(AUTHORIZATION, authorization)
            .header("X-TC-Action", ACTION)
            .header("X-TC-Version", VERSION)
            .header("X-TC-Timestamp", timestamp.to_string())
            .header("X-TC-Region", &self.region)
            .body(payload_text);

        if let Some(token) = &self.session_token {
            request = request.header("X-TC-Token", token);
        }

        let response = request
            .send()
            .await
            .context("failed to send Tencent Cloud Lighthouse request")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read Tencent Cloud Lighthouse response body")?;

        if !status.is_success() {
            bail!("Tencent Cloud Lighthouse request failed with status {status}: {body}");
        }

        let envelope: RawApiEnvelope = serde_json::from_str(&body)
            .with_context(|| format!("failed to parse Tencent Cloud Lighthouse JSON: {body}"))?;

        if let Some(error) = envelope
            .response
            .get("Error")
            .cloned()
            .map(serde_json::from_value::<TencentCloudError>)
            .transpose()
            .context("failed to parse Tencent Cloud Lighthouse error payload")?
        {
            let request_id = envelope
                .response
                .get("RequestId")
                .and_then(Value::as_str)
                .unwrap_or("unknown-request-id");

            bail!(
                "{}: {} (RequestId={})",
                error.code,
                error.message,
                request_id
            );
        }

        let response: DescribeInstancesTrafficPackagesResponse =
            serde_json::from_value(envelope.response)
                .context("failed to parse DescribeInstancesTrafficPackages response")?;

        let instance = response
            .instance_traffic_package_set
            .into_iter()
            .find(|instance| instance.instance_id == self.instance_id);

        let Some(instance) = instance else {
            return Ok(None);
        };

        let package = pick_preferred_package(instance.traffic_package_set);
        let Some(package) = package else {
            return Ok(None);
        };

        let used_pct = if package.traffic_package_total == 0 {
            0.0
        } else {
            (package.traffic_used as f64 / package.traffic_package_total as f64) * 100.0
        };

        Ok(Some(LighthouseState {
            timestamp: Utc::now().to_rfc3339(),
            lighthouse_instance_id: instance.instance_id,
            lighthouse_package_id: package.traffic_package_id,
            lighthouse_used: package.traffic_used,
            lighthouse_total: package.traffic_package_total,
            lighthouse_remaining: package.traffic_package_remaining,
            lighthouse_overflow: package.traffic_overflow,
            lighthouse_usage: used_pct,
            lighthouse_status: package.status,
            lighthouse_cycle_start: package.start_time,
            lighthouse_cycle_end: package.end_time,
            lighthouse_deadline: package.deadline,
        }))
    }
}

fn required_config_value(value: Option<&String>, key: &str) -> Result<String> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value.trim().to_string()),
        _ => bail!("missing required configuration value `{key}`"),
    }
}

fn build_payload(instance_id: &str) -> Value {
    let mut payload = Map::new();
    payload.insert("Offset".to_string(), json!(0));
    payload.insert("Limit".to_string(), json!(1));
    payload.insert("InstanceIds".to_string(), json!([instance_id]));
    Value::Object(payload)
}

fn build_authorization(
    secret_id: &str,
    secret_key: &str,
    endpoint: &str,
    payload: &str,
    timestamp: i64,
    date: &str,
) -> Result<String> {
    let hashed_request_payload = sha256_hex(payload.as_bytes());
    let canonical_request = format!(
        "POST\n/\n\ncontent-type:{JSON_CONTENT_TYPE}\nhost:{endpoint}\n\ncontent-type;host\n{hashed_request_payload}"
    );

    let credential_scope = format!("{date}/{SERVICE}/tc3_request");
    let hashed_canonical_request = sha256_hex(canonical_request.as_bytes());
    let string_to_sign =
        format!("{ALGORITHM}\n{timestamp}\n{credential_scope}\n{hashed_canonical_request}");

    let secret_date = hmac_sha256(format!("TC3{secret_key}").as_bytes(), date)?;
    let secret_service = hmac_sha256(&secret_date, SERVICE)?;
    let secret_signing = hmac_sha256(&secret_service, "tc3_request")?;
    let signature = hex::encode(hmac_sha256(&secret_signing, &string_to_sign)?);

    Ok(format!(
        "{ALGORITHM} Credential={secret_id}/{credential_scope}, SignedHeaders=content-type;host, Signature={signature}"
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &str) -> Result<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key).context("failed to initialize HMAC")?;
    mac.update(data.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

fn pick_preferred_package(packages: Vec<TrafficPackage>) -> Option<TrafficPackage> {
    packages.into_iter().max_by(|left, right| {
        let left_rank = package_rank(left);
        let right_rank = package_rank(right);
        left_rank
            .cmp(&right_rank)
            .then_with(|| left.end_time.cmp(&right.end_time))
            .then_with(|| left.deadline.cmp(&right.deadline))
    })
}

fn package_rank(package: &TrafficPackage) -> u8 {
    if package.status == "NETWORK_NORMAL" {
        1
    } else {
        0
    }
}

#[derive(Debug, Deserialize)]
struct RawApiEnvelope {
    #[serde(rename = "Response")]
    response: Value,
}

#[derive(Debug, Deserialize)]
struct TencentCloudError {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
}

#[derive(Debug, Deserialize)]
struct DescribeInstancesTrafficPackagesResponse {
    #[serde(rename = "InstanceTrafficPackageSet", default)]
    instance_traffic_package_set: Vec<InstanceTrafficPackage>,
}

#[derive(Debug, Deserialize)]
struct InstanceTrafficPackage {
    #[serde(rename = "InstanceId")]
    instance_id: String,
    #[serde(rename = "TrafficPackageSet", default)]
    traffic_package_set: Vec<TrafficPackage>,
}

#[derive(Debug, Deserialize)]
struct TrafficPackage {
    #[serde(rename = "TrafficPackageId")]
    traffic_package_id: String,
    #[serde(rename = "TrafficUsed")]
    traffic_used: u64,
    #[serde(rename = "TrafficPackageTotal")]
    traffic_package_total: u64,
    #[serde(rename = "TrafficPackageRemaining")]
    traffic_package_remaining: u64,
    #[serde(rename = "TrafficOverflow")]
    traffic_overflow: u64,
    #[serde(rename = "StartTime")]
    start_time: String,
    #[serde(rename = "EndTime")]
    end_time: String,
    #[serde(rename = "Deadline")]
    deadline: String,
    #[serde(rename = "Status")]
    status: String,
}
