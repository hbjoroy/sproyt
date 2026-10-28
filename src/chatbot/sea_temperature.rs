//! Bounded public marine forecasts. Coordinates, providers and prose are server-owned.
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use reqwest::{Url, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};

use crate::domain::RepositoryError;

type Result<T> = std::result::Result<T, RepositoryError>;
const MAX_BODY: usize = 256 * 1024;
const CACHE_SECONDS: i64 = 3600;
const SAMPLE_SECONDS: i64 = 90 * 60;
const UPDATE_SECONDS: i64 = 36 * 3600;
const USER_AGENT: &str = "Sproyt/sea-temperature (https://github.com/hbjoroy/sproyt)";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeaTemperatureConfig {
    pub default_location: String,
    pub locations: Vec<String>,
}

impl SeaTemperatureConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        let unique: HashSet<_> = self.locations.iter().collect();
        if self.locations.is_empty()
            || self.locations.len() > 8
            || unique.len() != self.locations.len()
            || !self.locations.contains(&self.default_location)
            || self.locations.iter().any(|id| place(id).is_none())
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Provider {
    OpenMeteo,
    Met,
}

#[derive(Clone, Copy)]
struct Place {
    id: &'static str,
    name: &'static str,
    lat: f64,
    lon: f64,
    timezone: &'static str,
    provider: Provider,
    max_distance_km: f64,
}

const PLACES: [Place; 4] = [
    Place {
        id: "parikia",
        name: "Parikia, Paros",
        lat: 37.085,
        lon: 25.148,
        timezone: "Europe/Athens",
        provider: Provider::OpenMeteo,
        max_distance_km: 6.0,
    },
    Place {
        id: "aliki",
        name: "Aliki, Paros",
        lat: 36.997,
        lon: 25.137,
        timezone: "Europe/Athens",
        provider: Provider::OpenMeteo,
        max_distance_km: 6.0,
    },
    Place {
        id: "bjoroy",
        name: "Bjorøy",
        lat: 60.33,
        lon: 5.17,
        timezone: "Europe/Oslo",
        provider: Provider::Met,
        max_distance_km: 2.0,
    },
    Place {
        id: "nordnes",
        name: "Nordnes, Bergen",
        lat: 60.399,
        lon: 5.304,
        timezone: "Europe/Oslo",
        provider: Provider::Met,
        max_distance_km: 1.5,
    },
];

fn place(id: &str) -> Option<&'static Place> {
    PLACES.iter().find(|p| p.id == id)
}

pub(crate) fn locations() -> Vec<Value> {
    PLACES
        .iter()
        .map(|p| json!({"id":p.id,"name":p.name}))
        .collect()
}

impl Provider {
    fn id(self) -> &'static str {
        match self {
            Self::OpenMeteo => "open_meteo",
            Self::Met => "met_oceanforecast",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::OpenMeteo => "Open-Meteo Marine",
            Self::Met => "MET Oceanforecast",
        }
    }
    fn url(self) -> &'static str {
        match self {
            Self::OpenMeteo => "https://open-meteo.com/en/docs/marine-weather-api",
            Self::Met => "https://api.met.no/weatherapi/oceanforecast/2.0/documentation",
        }
    }
    fn attribution(self) -> &'static str {
        match self {
            Self::OpenMeteo => "Open-Meteo; Meteo-France / Copernicus Marine; DWD (CC BY 4.0)",
            Self::Met => "Meteorologisk institutt (MET Norway, CC BY 4.0)",
        }
    }
}

#[derive(Clone)]
struct RawSeries {
    body: Value,
    fetched_at: i64,
    cache_until: i64,
}

#[derive(Default)]
struct Entry {
    raw: Option<RawSeries>,
    next_fetch: i64,
    failure: Option<&'static str>,
}

pub(crate) struct SeaTemperatureService {
    http: reqwest::Client,
    entries: Vec<Mutex<Entry>>,
    permits: Arc<Semaphore>,
}

impl SeaTemperatureService {
    pub(crate) fn from_env()
    -> std::result::Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        if std::env::var("SPROYT_SEA_TEMPERATURE_AGENTS_ENABLED").as_deref() != Ok("true") {
            return Ok(None);
        }
        Self::new().map(Some)
    }

    fn new() -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(USER_AGENT)
                .build()?,
            entries: PLACES
                .iter()
                .map(|_| Mutex::new(Entry::default()))
                .collect(),
            permits: PERMITS.get_or_init(|| Arc::new(Semaphore::new(2))).clone(),
        })
    }

    pub(crate) async fn lookup(
        &self,
        config: &SeaTemperatureConfig,
        location_ids: Option<&[String]>,
    ) -> Value {
        if std::env::var("SPROYT_SEA_TEMPERATURE_AGENTS_ENABLED").as_deref() != Ok("true") {
            return empty_snapshot("disabled");
        }
        if config.validate().is_err() {
            return empty_snapshot("invalid_configuration");
        }
        let default = [config.default_location.clone()];
        let ids = location_ids.unwrap_or(&default);
        if ids.is_empty()
            || ids.len() > 2
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
            || ids.iter().any(|id| !config.locations.contains(id))
        {
            return empty_snapshot("invalid_request");
        }
        let first = self.for_place(place(&ids[0]).unwrap());
        let results = if ids.len() == 2 {
            let (a, b) = tokio::join!(first, self.for_place(place(&ids[1]).unwrap()));
            vec![a, b]
        } else {
            vec![first.await]
        };
        snapshot(results, Utc::now().timestamp())
    }

    async fn for_place(&self, p: &Place) -> Value {
        let index = PLACES.iter().position(|x| x.id == p.id).unwrap();
        // Keep the per-place lock through fetching: identical concurrent requests
        // share the same raw series (and original fetch time), including failures.
        let mut entry = self.entries[index].lock().await;
        let now = Utc::now().timestamp();
        if let Some(raw) = &entry.raw
            && raw.cache_until > now
        {
            return normalize(p, raw, now).unwrap_or_else(|reason| unavailable(p, reason, now));
        }
        if entry.next_fetch > now {
            return unavailable(p, entry.failure.unwrap_or("source_backoff"), now);
        }
        entry.raw = None; // An expired result is never a fallback after a failed fetch.
        let fetched = self.fetch(p, &mut entry).await;
        let now = Utc::now().timestamp();
        match fetched {
            Ok(raw) => {
                let result =
                    normalize(p, &raw, now).unwrap_or_else(|reason| unavailable(p, reason, now));
                if raw.cache_until > now {
                    entry.raw = Some(raw);
                }
                entry.failure = None;
                result
            }
            Err(reason) => {
                entry.failure = Some(reason);
                entry.next_fetch = entry.next_fetch.max(now + 60);
                tracing::warn!(
                    source = p.provider.id(),
                    category = reason,
                    "marine source unavailable"
                );
                unavailable(p, reason, now)
            }
        }
    }

    async fn fetch(
        &self,
        p: &Place,
        entry: &mut Entry,
    ) -> std::result::Result<RawSeries, &'static str> {
        let _permit = tokio::time::timeout(Duration::from_secs(8), self.permits.acquire())
            .await
            .map_err(|_| "source_busy")?
            .map_err(|_| "source_busy")?;
        let mut url = Url::parse(match p.provider {
            Provider::OpenMeteo => "https://marine-api.open-meteo.com/v1/marine",
            Provider::Met => "https://api.met.no/weatherapi/oceanforecast/2.0/complete",
        })
        .map_err(|_| "source_configuration")?;
        match p.provider {
            Provider::OpenMeteo => {
                url.query_pairs_mut()
                    .append_pair("latitude", &p.lat.to_string())
                    .append_pair("longitude", &p.lon.to_string())
                    .append_pair("hourly", "sea_surface_temperature")
                    .append_pair("timezone", "GMT")
                    .append_pair("timeformat", "unixtime")
                    .append_pair("forecast_days", "2")
                    .append_pair("cell_selection", "sea");
            }
            Provider::Met => {
                url.query_pairs_mut()
                    .append_pair("lat", &p.lat.to_string())
                    .append_pair("lon", &p.lon.to_string());
            }
        }
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| "source_transport")?;
        let fetched_at = Utc::now().timestamp();
        let policy = cache_policy(response.headers(), fetched_at);
        entry.next_fetch = policy.next_fetch;
        if !response.status().is_success() {
            return Err(if response.status().as_u16() == 429 {
                "source_rate_limited"
            } else {
                "source_status"
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "source_transport")? {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err("source_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice(&bytes).map_err(|_| "source_invalid_json")?;
        Ok(RawSeries {
            body,
            fetched_at,
            cache_until: policy.cache_until,
        })
    }
}

struct CachePolicy {
    cache_until: i64,
    next_fetch: i64,
}

fn http_date(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|d| d.timestamp())
}

fn cache_policy(headers: &HeaderMap, now: i64) -> CachePolicy {
    let value = |key: &str| headers.get(key).and_then(|v| v.to_str().ok());
    let control = value("cache-control").unwrap_or("").to_ascii_lowercase();
    let age = value("age")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    let expires = value("expires").and_then(http_date);
    let max_age = control
        .split(',')
        .map(str::trim)
        .find_map(|v| {
            v.strip_prefix("max-age=")
                .and_then(|n| n.trim_matches('"').parse::<i64>().ok())
        })
        .map(|n| now.saturating_add(n.saturating_sub(age).max(0)));
    let provider_until = max_age.or(expires);
    let no_reuse = control
        .split(',')
        .any(|v| matches!(v.trim().split('=').next(), Some("no-store" | "no-cache")));
    let cache_until = if no_reuse {
        now
    } else {
        (now + CACHE_SECONDS).min(provider_until.unwrap_or(now + CACHE_SECONDS))
    };
    let retry_after = value("retry-after").and_then(|v| {
        v.parse::<i64>()
            .ok()
            .map(|n| now.saturating_add(n.max(0)))
            .or_else(|| http_date(v))
    });
    // Provider expiry may exceed the local one-hour cap. Keep that request embargo
    // even after local data expiry; an unavailable answer is preferable to refetching.
    let next_fetch = provider_until
        .unwrap_or(cache_until)
        .max(retry_after.unwrap_or(now))
        .max(now);
    CachePolicy {
        cache_until,
        next_fetch,
    }
}

fn coordinates(lat: f64, lon: f64) -> Value {
    json!({"latitude":lat,"longitude":lon})
}

fn valid_coordinates(lat: f64, lon: f64) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
}

fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let lat = (b.0 - a.0).to_radians();
    let lon = (b.1 - a.1).to_radians();
    let h = (lat / 2.0).sin().powi(2)
        + a.0.to_radians().cos() * b.0.to_radians().cos() * (lon / 2.0).sin().powi(2);
    6371.0 * 2.0 * h.clamp(0.0, 1.0).sqrt().asin()
}

fn temperature(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && (-3.0..=45.0).contains(v))
}

fn rfc3339(epoch: i64) -> Option<String> {
    DateTime::from_timestamp(epoch, 0).map(|d| d.to_rfc3339())
}

fn normalize(p: &Place, raw: &RawSeries, now: i64) -> std::result::Result<Value, &'static str> {
    let d = &raw.body;
    let mut updated = None;
    let (lat, lon, times, temps, grid) = match p.provider {
        Provider::OpenMeteo => {
            if d["timezone"].as_str() != Some("GMT")
                || d["utc_offset_seconds"].as_i64() != Some(0)
                || d["hourly_units"]["time"].as_str() != Some("unixtime")
                || d["hourly_units"]["sea_surface_temperature"].as_str() != Some("°C")
            {
                return Err("source_units_or_timezone");
            }
            let lat = d["latitude"].as_f64().ok_or("source_coordinates")?;
            let lon = d["longitude"].as_f64().ok_or("source_coordinates")?;
            let times = d["hourly"]["time"]
                .as_array()
                .ok_or("source_missing_series")?
                .iter()
                .map(Value::as_i64)
                .collect::<Option<Vec<_>>>()
                .ok_or("source_time")?;
            let temps = d["hourly"]["sea_surface_temperature"]
                .as_array()
                .ok_or("source_missing_series")?
                .clone();
            (lat, lon, times, temps, true)
        }
        Provider::Met => {
            if d["type"].as_str() != Some("Feature")
                || d["geometry"]["type"].as_str() != Some("Point")
            {
                return Err("source_coordinates");
            }
            if d["properties"]["meta"]["units"]["sea_water_temperature"].as_str() != Some("celsius")
            {
                return Err("source_units_or_timezone");
            }
            if let Some(s) = d["properties"]["meta"]["updated_at"].as_str() {
                updated = Some(
                    DateTime::parse_from_rfc3339(s)
                        .map_err(|_| "source_time")?
                        .timestamp(),
                );
            } else if !d["properties"]["meta"]["updated_at"].is_null() {
                return Err("source_time");
            }
            let lat = d["geometry"]["coordinates"][1]
                .as_f64()
                .ok_or("source_coordinates")?;
            let lon = d["geometry"]["coordinates"][0]
                .as_f64()
                .ok_or("source_coordinates")?;
            let series = d["properties"]["timeseries"]
                .as_array()
                .ok_or("source_missing_series")?;
            let times = series
                .iter()
                .map(|row| {
                    row["time"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|d| d.timestamp())
                })
                .collect::<Option<Vec<_>>>()
                .ok_or("source_time")?;
            let temps = series
                .iter()
                .map(|row| row["data"]["instant"]["details"]["sea_water_temperature"].clone())
                .collect();
            (lat, lon, times, temps, false)
        }
    };
    if !valid_coordinates(lat, lon) {
        return Err("source_coordinates");
    }
    let distance = distance_km((p.lat, p.lon), (lat, lon));
    if distance > p.max_distance_km || distance > 10.0 {
        return Err("source_position_too_far");
    }
    if updated.is_some_and(|t| now - t > UPDATE_SECONDS || t > now + 300) {
        return Err("source_update_stale");
    }
    if times.is_empty()
        || times.len() != temps.len()
        || times.len() > 512
        || times.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("source_invalid_series");
    }
    let coverage_end = *times.last().ok_or("source_missing_series")?;
    if now < times[0] || now >= coverage_end {
        return Err("source_outside_coverage");
    }
    let (index, &valid_at) = times
        .iter()
        .enumerate()
        .min_by_key(|(_, t)| t.abs_diff(now))
        .ok_or("source_missing_series")?;
    if valid_at.abs_diff(now) > SAMPLE_SECONDS as u64 {
        return Err("source_sample_stale");
    }
    let temp = temperature(&temps[index]).ok_or("source_temperature_missing_or_invalid")?;
    let valid_until = raw
        .cache_until
        .min(coverage_end)
        .min(valid_at.saturating_add(SAMPLE_SECONDS))
        .min(now + 300)
        .min(updated.map(|t| t + UPDATE_SECONDS).unwrap_or(i64::MAX));
    if valid_until <= now {
        return Err("source_cache_expired");
    }
    Ok(json!({
        "status":"available", "id":p.id, "name":p.name,"source":p.provider.id(),
        "source_name":p.provider.name(), "source_url":p.provider.url(), "attribution":p.provider.attribution(),
        "type":"model_forecast", "timezone":p.timezone, "temperature_c":temp,
        "valid_at":rfc3339(valid_at), "valid_at_epoch":valid_at,
        "fetched_at":rfc3339(raw.fetched_at), "fetched_at_epoch":raw.fetched_at,
        "source_updated_at":updated.and_then(rfc3339),"source_updated_at_epoch":updated,
        "model_age":"unknown", "requested_coordinates":coordinates(p.lat,p.lon),
        "reported_coordinates":coordinates(lat,lon),
        "model_grid_coordinates":if grid { coordinates(lat,lon) } else { Value::Null },
        "distance_km":distance,"distance_basis":if grid {"documented_model_grid"} else {"reported_position"},
        "cache_valid_until_epoch":raw.cache_until,"valid_until_epoch":valid_until,"coverage_until_epoch":coverage_end,
        "note":"Regionalt modellestimat; lokal temperatur og badevasskvalitet er ikkje målt."
    }))
}

fn unavailable(p: &Place, reason: &'static str, now: i64) -> Value {
    json!({"status":"unavailable","id":p.id,"name":p.name,"source":p.provider.id(),"source_name":p.provider.name(),
        "source_url":p.provider.url(),"attribution":p.provider.attribution(),"type":"model_forecast","timezone":p.timezone,
        "reason":reason,"valid_until_epoch":now+60})
}

fn empty_snapshot(reason: &'static str) -> Value {
    json!({"status":"unavailable","locations":[],"reason":reason,"valid_until_epoch":Utc::now().timestamp()+60})
}

fn snapshot(results: Vec<Value>, now: i64) -> Value {
    let available = results
        .iter()
        .filter(|v| v["status"] == "available")
        .count();
    let until = results
        .iter()
        .filter_map(|v| v["valid_until_epoch"].as_i64())
        .min()
        .unwrap_or(now + 60)
        .min(now + 300);
    json!({"status":if available==results.len() && available>0 {"available"} else if available>0 {"partial"} else {"unavailable"},
        "locations":results,"valid_until_epoch":until})
}

/// Render only fixed catalogue names/links and validated snapshot scalar fields.
/// Model prose, source notes and agent response phrases cannot enter the answer.
pub(crate) fn render(snapshot: &Value) -> Result<String> {
    let rows = snapshot["locations"]
        .as_array()
        .ok_or(RepositoryError::Conflict)?;
    if rows.len() > 2 {
        return Err(RepositoryError::Conflict);
    }
    if rows.is_empty() {
        return Ok("Eg får ikkje henta sjøtemperatur for ein godkjend stad no.".to_owned());
    }
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for row in rows {
        let p = place(row["id"].as_str().ok_or(RepositoryError::Conflict)?)
            .ok_or(RepositoryError::Conflict)?;
        if !seen.insert(p.id) || row["source"].as_str() != Some(p.provider.id()) {
            return Err(RepositoryError::Conflict);
        }
        match row["status"].as_str() {
            Some("available") => {
                if row["type"] != "model_forecast" || row["timezone"].as_str() != Some(p.timezone) {
                    return Err(RepositoryError::Conflict);
                }
                let temp = temperature(&row["temperature_c"]).ok_or(RepositoryError::Conflict)?;
                let valid = row["valid_at_epoch"]
                    .as_i64()
                    .and_then(|t| DateTime::from_timestamp(t, 0))
                    .ok_or(RepositoryError::Conflict)?;
                let zone = p
                    .timezone
                    .parse::<chrono_tz::Tz>()
                    .map_err(|_| RepositoryError::Conflict)?;
                let time = valid.with_timezone(&zone).format("%d.%m.%Y kl. %H:%M %Z");
                let value = format!("{temp:.1}").replace('.', ",");
                lines.push(format!(
                    "{}: {} °C ({}). Kjelde: [{}]({}).",
                    p.name,
                    value,
                    time,
                    p.provider.name(),
                    p.provider.url()
                ));
            }
            Some("unavailable") => lines.push(format!(
                "{}: sjøtemperaturen er utilgjengeleg no. Kjelde: [{}]({}).",
                p.name,
                p.provider.name(),
                p.provider.url()
            )),
            _ => return Err(RepositoryError::Conflict),
        }
    }
    lines.push("Regionalt modellestimat, ikkje ei lokal bademåling. Modellalderen er ukjend. Dette svaret gjeld berre sjøtemperatur.".to_owned());
    let providers: HashSet<_> = rows.iter().filter_map(|r| r["source"].as_str()).collect();
    if providers.contains("open_meteo") {
        lines.push(format!(
            "Kreditering: {}.",
            Provider::OpenMeteo.attribution()
        ));
    }
    if providers.contains("met_oceanforecast") {
        lines.push(format!("Kreditering: {}.", Provider::Met.attribution()));
    }
    Ok(lines.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_838_800; // 2026-10-12 21:00 UTC, not midnight.

    fn om(temp: Value) -> RawSeries {
        RawSeries {
            fetched_at: NOW,
            cache_until: NOW + 3600,
            body: json!({
                "latitude":37.125,"longitude":25.125015,"timezone":"GMT","utc_offset_seconds":0,
                "hourly_units":{"time":"unixtime","sea_surface_temperature":"°C"},
                "hourly":{"time":[NOW-21*3600,NOW,NOW+3600],"sea_surface_temperature":[30.0,temp,21.0]},
                "generationtime_ms":0.04
            }),
        }
    }

    fn met() -> RawSeries {
        RawSeries {
            fetched_at: NOW,
            cache_until: NOW + 1800,
            body: json!({
                "type":"Feature","geometry":{"type":"Point","coordinates":[5.1571,60.3359]},
                "properties":{"meta":{"updated_at":rfc3339(NOW-3600),"units":{"sea_water_temperature":"celsius"}},
                    "timeseries":[
                        {"time":rfc3339(NOW),"data":{"instant":{"details":{"sea_water_temperature":12.1}}}},
                        {"time":rfc3339(NOW+3600),"data":{"instant":{"details":{"sea_water_temperature":12.0}}}}
                    ]}
            }),
        }
    }

    #[test]
    fn strict_config_and_catalogue() {
        let config = SeaTemperatureConfig {
            default_location: "parikia".into(),
            locations: vec!["parikia".into(), "bjoroy".into()],
        };
        assert!(config.validate().is_ok());
        let mut invalid = config.clone();
        invalid.locations.push("parikia".into());
        assert!(invalid.validate().is_err());
        invalid.locations = vec!["unknown".into()];
        assert!(invalid.validate().is_err());
        assert!(serde_json::from_value::<SeaTemperatureConfig>(json!({"default_location":"parikia","locations":["parikia"],"url":"https://evil.example"})).is_err());
        assert_eq!(locations().len(), 4);
    }

    #[test]
    fn null_is_missing_but_zero_is_valid_and_nearest_is_not_midnight() {
        assert!(normalize(&PLACES[0], &om(Value::Null), NOW).is_err());
        let zero = normalize(&PLACES[0], &om(json!(0.0)), NOW).unwrap();
        assert_eq!(zero["temperature_c"], 0.0);
        assert_eq!(zero["valid_at_epoch"], NOW);
        assert!(zero["source_updated_at"].is_null());
        assert_eq!(zero["model_age"], "unknown");
        assert_eq!(zero["distance_basis"], "documented_model_grid");
        assert_eq!(zero["valid_until_epoch"], NOW + 300);
        let next = normalize(&PLACES[0], &om(json!(0.0)), NOW + 1900).unwrap();
        assert_eq!(next["temperature_c"], 21.0);
        assert_eq!(next["fetched_at_epoch"], NOW);
        let mut ending = om(json!(22.4));
        ending.body["hourly"]["time"] = json!([NOW - 3600, NOW, NOW + 100]);
        assert_eq!(
            normalize(&PLACES[0], &ending, NOW).unwrap()["valid_until_epoch"],
            NOW + 100
        );
        assert!(normalize(&PLACES[0], &ending, NOW + 100).is_err());
    }

    #[test]
    fn rejects_units_coordinates_temperature_and_stale_bounds() {
        let mut raw = om(json!(22.4));
        raw.body["hourly_units"]["sea_surface_temperature"] = json!("F");
        assert!(normalize(&PLACES[0], &raw, NOW).is_err());
        raw = om(json!(46.0));
        assert!(normalize(&PLACES[0], &raw, NOW).is_err());
        raw = om(json!(22.4));
        raw.body["latitude"] = json!(60.0);
        assert!(normalize(&PLACES[0], &raw, NOW).is_err());
        raw = om(json!(22.4));
        raw.body["longitude"] = Value::Null;
        assert!(normalize(&PLACES[0], &raw, NOW).is_err());
        raw = om(json!(22.4));
        raw.body["timezone"] = json!("Europe/Athens");
        assert!(normalize(&PLACES[0], &raw, NOW).is_err());
        raw = om(json!(22.4));
        raw.cache_until = NOW + 20000;
        assert!(normalize(&PLACES[0], &raw, NOW + 3600 + SAMPLE_SECONDS + 1).is_err());
        let mut raw = met();
        raw.body["properties"]["meta"]["updated_at"] = json!(rfc3339(NOW - UPDATE_SECONDS - 1));
        assert!(normalize(&PLACES[2], &raw, NOW).is_err());
        let normal = normalize(&PLACES[2], &met(), NOW).unwrap();
        assert!(normal["model_grid_coordinates"].is_null());
        assert_eq!(normal["distance_basis"], "reported_position");
    }

    #[test]
    fn cache_caps_expiry_retry_after_and_age() {
        let mut headers = HeaderMap::new();
        headers.insert("expires", "Tue, 13 Oct 2026 00:00:00 GMT".parse().unwrap());
        let policy = cache_policy(&headers, NOW);
        assert_eq!(policy.cache_until, NOW + 3600);
        assert!(policy.next_fetch > policy.cache_until);
        headers.insert("cache-control", "max-age=1200".parse().unwrap());
        headers.insert("age", "100".parse().unwrap());
        headers.insert("retry-after", "7200".parse().unwrap());
        let policy = cache_policy(&headers, NOW);
        assert_eq!(policy.cache_until, NOW + 1100);
        assert_eq!(policy.next_fetch, NOW + 7200);
        headers.insert("cache-control", "no-store".parse().unwrap());
        assert_eq!(cache_policy(&headers, NOW).cache_until, NOW);
    }

    #[tokio::test]
    async fn concurrent_identity_reuses_original_fetch_and_expired_failure_never_falls_back() {
        let service = SeaTemperatureService::new().unwrap();
        let now = Utc::now().timestamp();
        let mut raw = om(json!(22.4));
        raw.body["hourly"]["time"] = json!([now - 3600, now, now + 3600]);
        raw.fetched_at = now - 20;
        raw.cache_until = now + 60;
        service.entries[0].lock().await.raw = Some(raw.clone());
        let (a, b) = tokio::join!(service.for_place(&PLACES[0]), service.for_place(&PLACES[0]));
        assert_eq!(a, b);
        assert_eq!(a["fetched_at_epoch"], now - 20);
        raw.cache_until = now - 1;
        let mut entry = service.entries[0].lock().await;
        entry.raw = Some(raw);
        entry.next_fetch = now + 60;
        entry.failure = Some("source_transport");
        drop(entry);
        let failed = service.for_place(&PLACES[0]).await;
        assert_eq!(failed["status"], "unavailable");
        assert!(failed["temperature_c"].is_null());
    }

    #[test]
    fn renderer_uses_fixed_fields_and_dst_timezone_and_partial_results() {
        let mut good = normalize(&PLACES[0], &om(json!(22.4)), NOW).unwrap();
        good["note"] = json!("Målt på stranda: 45 °C; varmast i verda!");
        good["name"] = json!("Evil");
        good["source_url"] = json!("https://evil.example");
        let partial = snapshot(
            vec![good, unavailable(&PLACES[2], "source_transport", NOW)],
            NOW,
        );
        let text = render(&partial).unwrap();
        assert!(text.contains("22,4 °C"));
        assert!(text.contains("Bjorøy: sjøtemperaturen er utilgjengeleg"));
        assert!(!text.contains("45 °C"));
        assert!(!text.contains("evil.example"));
        assert!(!text.contains("varmast"));
        // UTC epoch conversion across Oslo's autumn DST boundary is unambiguous.
        let a = DateTime::parse_from_rfc3339("2026-10-25T00:30:00Z")
            .unwrap()
            .with_timezone(&chrono_tz::Europe::Oslo);
        let b = DateTime::parse_from_rfc3339("2026-10-25T01:30:00Z")
            .unwrap()
            .with_timezone(&chrono_tz::Europe::Oslo);
        assert_eq!(a.format("%H:%M %Z").to_string(), "02:30 CEST");
        assert_eq!(b.format("%H:%M %Z").to_string(), "02:30 CET");
        let mut invalid = partial;
        invalid["locations"][0]["temperature_c"] = json!(100.0);
        assert!(render(&invalid).is_err());
    }

    /// Operator-only, read-only integration probe. No database, messages or edits.
    /// Requires SPROYT_VLLM_URL (+ API key if protected), the marine gate and
    /// outbound HTTPS access to the two fixed public providers.
    #[tokio::test]
    #[ignore = "requires explicitly configured vLLM access and marine gate"]
    async fn live_qwen_and_marine_server_rendering() {
        use crate::chatbot::{ContextMessage, VllmChat, tools::ReadTools};

        let model = VllmChat::from_env()
            .unwrap()
            .expect("SPROYT_VLLM_URL required");
        let service = SeaTemperatureService::from_env()
            .unwrap()
            .expect("SPROYT_SEA_TEMPERATURE_AGENTS_ENABLED=true required");
        let config = SeaTemperatureConfig {
            default_location: "parikia".into(),
            locations: vec!["parikia".into(), "aliki".into(), "bjoroy".into()],
        };
        for (label, question, expected) in [
            (
                "default",
                "Kva er sjøtemperaturen ved standardstaden min no?",
                &["parikia"][..],
            ),
            (
                "alternate",
                "Kva er sjøtemperaturen i Aliki på Paros no?",
                &["aliki"][..],
            ),
            (
                "comparison",
                "Kan du samanlikne sjøtemperaturen ved Parikia på Paros og ved Bjorøy no?",
                &["parikia", "bjoroy"][..],
            ),
            (
                "conversation",
                "Hei Maria! Kan du foreslå eit hyggeleg brettspel for to personar?",
                &[][..],
            ),
        ] {
            let messages = [ContextMessage {
                source: None,
                id: "marine-operator-probe".into(),
                author: "Operator test".into(),
                body: format!("@Maria {question}"),
            }];
            let mut tools = ReadTools {
                place_lookup: None,
                weather_service: None,
                sea_temperature_service: Some(&service),
                sea_temperature_config: Some(config.clone()),
                sea_temperature: None,
                weather: None,
                ferry: None,
                observations: None,
            };
            let reply = model
                .reply_with_tools(
                    "Maria",
                    &[],
                    &[],
                    "marine-operator-probe",
                    &messages,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    Some(&mut tools),
                )
                .await
                .unwrap();
            if expected.is_empty() {
                assert!(
                    tools.sea_temperature.is_none(),
                    "unrelated conversation must not invoke marine lookup"
                );
                assert!(!reply.trim().is_empty());
            } else {
                let snapshot = tools
                    .sea_temperature
                    .as_ref()
                    .expect("model must invoke the native marine tool");
                assert_eq!(
                    snapshot["status"], "available",
                    "{label}: live provider coverage required"
                );
                let mut actual = snapshot["locations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| row["id"].as_str().unwrap())
                    .collect::<Vec<_>>();
                let mut expected = expected.to_vec();
                actual.sort_unstable();
                expected.sort_unstable();
                assert_eq!(
                    actual, expected,
                    "{label}: correct authorized place selection"
                );
                assert_eq!(
                    reply,
                    render(snapshot).unwrap(),
                    "marine reply must be the exact server rendering"
                );
                assert!(snapshot["valid_until_epoch"].as_i64().unwrap() > Utc::now().timestamp());
            }
            println!("Read-only marine/Qwen smoke ({label}): {reply}");
        }
    }
}
