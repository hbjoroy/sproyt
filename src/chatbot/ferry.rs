//! Bounded scheduled port calls from Ferry-Schedule. No live tracking claims.
use super::*;
use chrono::{NaiveTime, TimeZone};
use chrono_tz::Europe::Athens;

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_CALLS: usize = 100;
const SOURCE_URL: &str = "https://www.gtp.gr/greekferries_searchresult.asp";

#[derive(Clone)]
pub(super) struct FerryService {
    base: Url,
    http: reqwest::Client,
}

impl FerryService {
    pub(super) fn from_env()
    -> std::result::Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let Ok(base) = std::env::var("SPROYT_FERRY_URL") else {
            return Ok(None);
        };
        if base.is_empty() {
            return Ok(None);
        }
        Self::new(&base).map(Some)
    }

    fn new(base: &str) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let base = Url::parse(base)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err("invalid SPROYT_FERRY_URL".into());
        }
        Ok(Self {
            base,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    pub(super) async fn snapshot(&self, port: &str) -> std::result::Result<Value, &'static str> {
        if port != "paros" {
            return Err("ferry_port");
        }
        let date = Utc::now()
            .with_timezone(&Athens)
            .format("%d/%m/%Y")
            .to_string();
        let mut url = self
            .base
            .join("api/v1/schedule/paros")
            .map_err(|_| "ferry_configuration")?;
        url.query_pairs_mut().append_pair("date", &date);
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| "ferry_transport")?;
        if !response.status().is_success() {
            return Err("ferry_status");
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err("ferry_contract");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "ferry_transport")? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err("ferry_contract");
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: Value = serde_json::from_slice(&bytes).map_err(|_| "ferry_contract")?;
        // A midnight rollover during HTTP must not return yesterday's schedule as today's.
        normalize(&response, &date, Utc::now())
    }
}

fn text(value: &Value, key: &str, max_chars: usize) -> std::result::Result<Value, &'static str> {
    let Some(value) = value.get(key).filter(|value| !value.is_null()) else {
        return Ok(Value::Null);
    };
    let value = value.as_str().ok_or("ferry_contract")?;
    if value.chars().count() > max_chars || value.chars().any(char::is_control) {
        return Err("ferry_contract");
    }
    let value = value.trim();
    Ok(if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    })
}

fn scheduled_time(value: &Value, key: &str) -> std::result::Result<Value, &'static str> {
    let value = text(value, key, 5)?;
    if let Some(time) = value.as_str()
        && (time.len() != 5
            || time.as_bytes()[2] != b':'
            || !time
                .bytes()
                .enumerate()
                .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
            || NaiveTime::parse_from_str(time, "%H:%M").is_err())
    {
        return Err("ferry_contract");
    }
    Ok(value)
}

fn normalize(
    response: &Value,
    requested_date: &str,
    now: DateTime<Utc>,
) -> std::result::Result<Value, &'static str> {
    let local_now = now.with_timezone(&Athens);
    let today = local_now.format("%d/%m/%Y").to_string();
    if response["port"].as_str() != Some("paros") {
        return Err("ferry_port");
    }
    if requested_date != today || response["date"].as_str() != Some(requested_date) {
        return Err("ferry_date");
    }
    let fetched_at = response["fetched_at"]
        .as_str()
        .filter(|value| value.len() <= 80)
        .ok_or("ferry_timestamp")?;
    let fetched = DateTime::parse_from_rfc3339(fetched_at).map_err(|_| "ferry_timestamp")?;
    if fetched > now + chrono::Duration::seconds(60) || fetched < now - chrono::Duration::hours(24)
    {
        return Err("ferry_stale");
    }
    let schedules = response["schedules"].as_array().ok_or("ferry_contract")?;
    if schedules.len() > MAX_CALLS || response["count"].as_u64() != Some(schedules.len() as u64) {
        return Err("ferry_contract");
    }
    let mut calls = Vec::with_capacity(schedules.len());
    for call in schedules {
        if call["date"].as_str() != Some(requested_date) {
            return Err("ferry_date");
        }
        let vessel = text(call, "vessel", 120)?;
        if vessel.is_null() {
            return Err("ferry_contract");
        }
        let mut normalized = json!({
            "date":requested_date, "vessel":vessel,
            "scheduled_arrival_local":scheduled_time(call,"arriving")?,
            "scheduled_departure_local":scheduled_time(call,"leaving")?,
            "from_port":text(call,"from_port",120)?, "to_port":text(call,"to_port",120)?
        });
        for (key, limit) in [
            ("company", 160),
            ("operator_code", 16),
            ("ship_type", 80),
            ("route_start", 120),
            ("route_end", 120),
        ] {
            normalized[key] = text(call, key, limit)?;
        }
        if let Some(route) = call.get("full_route").filter(|value| !value.is_null()) {
            let route = route
                .as_array()
                .filter(|route| route.len() <= 30)
                .ok_or("ferry_contract")?;
            let mut ports = Vec::with_capacity(route.len());
            for port in route {
                let port = text(&json!({"port":port}), "port", 120)?;
                if port.is_null() {
                    return Err("ferry_contract");
                }
                ports.push(port);
            }
            normalized["full_route"] = json!(ports);
        }
        if let Some(stops) = call.get("stops").filter(|value| !value.is_null()) {
            normalized["stops"] = json!(
                stops
                    .as_u64()
                    .filter(|stops| *stops <= 100)
                    .ok_or("ferry_contract")?
            );
        }
        calls.push(normalized);
    }
    let midnight = local_now
        .date_naive()
        .succ_opt()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|time| Athens.from_local_datetime(&time).single())
        .ok_or("ferry_timestamp")?;
    let valid_until = (now.timestamp() + 300)
        .min(midnight.timestamp())
        .min(fetched.timestamp() + 24 * 3600);
    if valid_until <= now.timestamp() {
        return Err("ferry_stale");
    }
    Ok(json!({
        "source":"GTP ferry schedules", "source_url":SOURCE_URL,
        "data_kind":"scheduled", "scheduled_only":true, "live_tracking":false,
        "notice":"Scheduled port calls only. Actual arrival, departure, delays and vessel positions are unknown.",
        "port":"paros", "date":requested_date, "timezone":"Europe/Athens",
        "fetched_at":fetched_at, "fetched_at_epoch":fetched.timestamp(),
        "local_now":local_now.to_rfc3339(), "valid_until_epoch":valid_until,
        "count":calls.len(), "calls":calls
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-04T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
    fn fixture(now: DateTime<Utc>) -> Value {
        let date = now.with_timezone(&Athens).format("%d/%m/%Y").to_string();
        json!({"port":"paros","date":date,"fetched_at":(now-chrono::Duration::hours(2)).to_rfc3339(),
            "count":1,"schedules":[{"date":date,"vessel":"Blue Star Naxos","arriving":"12:15",
                "from_port":"Syros","to_port":"Naxos","full_route":["Syros","Paros","Naxos"]}]})
    }

    #[test]
    fn scheduled_snapshot_preserves_source_time_and_unknown_departure() {
        let response = fixture(now());
        let snapshot = normalize(&response, "04/10/2026", now()).unwrap();
        assert_eq!(snapshot["fetched_at"], response["fetched_at"]);
        assert_eq!(snapshot["fetched_at_epoch"], now().timestamp() - 7200);
        assert_eq!(snapshot["local_now"], "2026-10-04T12:00:00+03:00");
        assert_eq!(snapshot["valid_until_epoch"], now().timestamp() + 300);
        assert_eq!(snapshot["scheduled_only"], true);
        assert_eq!(snapshot["live_tracking"], false);
        assert_eq!(snapshot["calls"][0]["scheduled_arrival_local"], "12:15");
        assert!(snapshot["calls"][0]["scheduled_departure_local"].is_null());
        assert_eq!(snapshot["source_url"], SOURCE_URL);
    }

    #[test]
    fn validity_uses_athens_midnight_and_source_expiry() {
        let midnight = DateTime::parse_from_rfc3339("2026-10-04T23:59:30+03:00")
            .unwrap()
            .with_timezone(&Utc);
        let snapshot = normalize(&fixture(midnight), "04/10/2026", midnight).unwrap();
        assert_eq!(snapshot["valid_until_epoch"], midnight.timestamp() + 30);
        let mut expiring = fixture(now());
        expiring["fetched_at"] = json!(
            (now() - chrono::Duration::hours(24) + chrono::Duration::seconds(20)).to_rfc3339()
        );
        assert_eq!(
            normalize(&expiring, "04/10/2026", now()).unwrap()["valid_until_epoch"],
            now().timestamp() + 20
        );
        let winter = DateTime::parse_from_rfc3339("2026-12-04T22:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(normalize(&fixture(winter), "05/12/2026", winter).is_ok());
        assert_eq!(
            normalize(&fixture(winter), "04/12/2026", winter).unwrap_err(),
            "ferry_date"
        );
    }

    #[test]
    fn contract_rejects_wrong_dates_freshness_and_unbounded_data() {
        for (key, value, error) in [
            ("port", json!("naxos"), "ferry_port"),
            ("date", json!("03/10/2026"), "ferry_date"),
            ("fetched_at", json!("unknown"), "ferry_timestamp"),
            (
                "fetched_at",
                json!((now() + chrono::Duration::seconds(61)).to_rfc3339()),
                "ferry_stale",
            ),
            (
                "fetched_at",
                json!(
                    (now() - chrono::Duration::hours(24) - chrono::Duration::seconds(1))
                        .to_rfc3339()
                ),
                "ferry_stale",
            ),
            ("count", json!(2), "ferry_contract"),
        ] {
            let mut response = fixture(now());
            response[key] = value;
            assert_eq!(
                normalize(&response, "04/10/2026", now()).unwrap_err(),
                error
            );
        }
        for (key, value, error) in [
            ("date", json!("05/10/2026"), "ferry_date"),
            ("vessel", json!("x".repeat(121)), "ferry_contract"),
            ("arriving", json!("24:00"), "ferry_contract"),
            ("arriving", json!("9:30"), "ferry_contract"),
            ("from_port", json!("bad\nport"), "ferry_contract"),
        ] {
            let mut response = fixture(now());
            response["schedules"][0][key] = value;
            assert_eq!(
                normalize(&response, "04/10/2026", now()).unwrap_err(),
                error
            );
        }
        let mut too_many = fixture(now());
        too_many["schedules"] = json!(vec![too_many["schedules"][0].clone(); 101]);
        too_many["count"] = json!(101);
        assert_eq!(
            normalize(&too_many, "04/10/2026", now()).unwrap_err(),
            "ferry_contract"
        );
        let empty = json!({"port":"paros","date":"04/10/2026","fetched_at":now().to_rfc3339(),"count":0,"schedules":[]});
        assert_eq!(
            normalize(&empty, "04/10/2026", now()).unwrap()["calls"],
            json!([])
        );
    }

    #[test]
    fn root_url_has_no_credentials_query_fragment_or_path() {
        assert!(FerryService::new("http://ferry.local:8080/").is_ok());
        for base in [
            "ftp://ferry.local/",
            "http://user:password@ferry.local/",
            "http://ferry.local/api/",
            "http://ferry.local/?date=other",
            "http://ferry.local/#fragment",
        ] {
            assert!(FerryService::new(base).is_err());
        }
    }

    #[tokio::test]
    async fn http_route_date_redirect_and_stream_size_are_bounded() {
        use axum::{
            Json, Router,
            body::{Body, Bytes},
            extract::Query,
            http::StatusCode,
            response::IntoResponse,
            routing::get,
        };
        use std::{
            collections::HashMap,
            sync::atomic::{AtomicUsize, Ordering},
        };
        let requests = Arc::new(AtomicUsize::new(0));
        let redirects = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let followed = redirects.clone();
        let app = Router::new()
            .route(
                "/api/v1/schedule/paros",
                get(move |Query(query): Query<HashMap<String, String>>| {
                    let number = seen.fetch_add(1, Ordering::SeqCst);
                    async move {
                        assert_eq!(query.len(), 1);
                        assert_eq!(
                            query["date"],
                            Utc::now()
                                .with_timezone(&Athens)
                                .format("%d/%m/%Y")
                                .to_string()
                        );
                        match number {
                            0 => Json(fixture(Utc::now())).into_response(),
                            1 => (StatusCode::FOUND, [("location", "/redirect-target")])
                                .into_response(),
                            2 => Body::from_stream(futures_util::stream::iter([
                                Ok::<_, std::convert::Infallible>(Bytes::from(vec![
                                    b' ';
                                    128 * 1024
                                ])),
                                Ok(Bytes::from(vec![b' '; 128 * 1024 + 1])),
                            ]))
                            .into_response(),
                            _ => "not JSON".into_response(),
                        }
                    }
                }),
            )
            .route(
                "/redirect-target",
                get(move || {
                    followed.fetch_add(1, Ordering::SeqCst);
                    async { "unexpected" }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let service =
            FerryService::new(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert_eq!(service.snapshot("naxos").await.unwrap_err(), "ferry_port");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert_eq!(service.snapshot("paros").await.unwrap()["count"], 1);
        assert_eq!(service.snapshot("paros").await.unwrap_err(), "ferry_status");
        assert_eq!(redirects.load(Ordering::SeqCst), 0);
        assert_eq!(
            service.snapshot("paros").await.unwrap_err(),
            "ferry_contract"
        );
        assert_eq!(
            service.snapshot("paros").await.unwrap_err(),
            "ferry_contract"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 4);
        server.abort();
    }
}
