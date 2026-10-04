//! Receive position frames from our own existing AIS stream. The stations
//! endpoint's last_seen advances on static messages too; use it for names/types
//! only, NEVER for positions or position freshness.
use super::operators;
use chrono::Utc;
use reqwest::Url;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{RwLock, watch};

const MAX_STATIONS: usize = 4096;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINE: usize = 16 * 1024;
const MAX_POSITIONS: usize = 512;
const POSITION_AGE: i64 = 600;
const SNAPSHOT_AGE: i64 = 60;

#[derive(Clone)]
pub(super) struct ObservationService {
    base: Url,
    http: reqwest::Client,
    state: Arc<RwLock<State>>,
}
#[derive(Default)]
struct State {
    identities: HashMap<u64, Identity>,
    positions: HashMap<u64, Position>,
    connected: bool,
    last_stream_activity: Option<i64>,
}
#[derive(Clone, Default)]
struct Identity {
    name: Option<String>,
    ship_type: Option<u64>,
}
struct Position {
    lat: f64,
    lon: f64,
    sog: Option<f64>,
    cog: Option<f64>,
    received_at: i64,
}

fn number(value: &Value, key: &str, min: f64, max: f64) -> Option<f64> {
    value[key]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= min && *v < max)
}
fn identity_name(value: &Value, key: &str) -> Option<String> {
    let name = value[key].as_str()?.trim().trim_end_matches('@').trim();
    (!name.is_empty() && name.chars().count() <= 120 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}
fn vessel_id(value: &Value) -> Option<u64> {
    value["mmsi"]
        .as_u64()
        .filter(|id| (100_000_000..=999_999_999).contains(id))
}

impl State {
    fn metadata(&mut self, rows: &Value) -> Result<(), &'static str> {
        let rows = rows.as_array().ok_or("ais_stations_contract")?;
        if rows.len() > MAX_STATIONS {
            return Err("ais_stations_contract");
        }
        let mut identities = HashMap::new();
        for row in rows {
            if let Some(id) = vessel_id(row) {
                identities.insert(
                    id,
                    Identity {
                        name: identity_name(row, "vessel_name"),
                        ship_type: row["ship_type"].as_u64().filter(|v| *v <= 99),
                    },
                );
            }
        }
        self.identities = identities;
        Ok(())
    }
    fn frame(&mut self, frame: &Value, now: i64) {
        let Some(id) = vessel_id(frame) else {
            return;
        };
        let Some(kind) = frame["type"].as_u64() else {
            return;
        };
        if matches!(kind, 5 | 19 | 24)
            && (self.identities.len() < MAX_STATIONS || self.identities.contains_key(&id))
        {
            let identity = self.identities.entry(id).or_default();
            if let Some(name) = identity_name(frame, "shipname") {
                identity.name = Some(name);
            }
            if let Some(ship_type) = frame["ship_type"].as_u64().filter(|v| *v <= 99) {
                identity.ship_type = Some(ship_type);
            }
        }
        if !matches!(kind, 1 | 2 | 3 | 18 | 19 | 27) {
            return;
        }
        let (Some(lat), Some(lon)) = (
            number(frame, "lat", -90.0, 90.00001),
            number(frame, "lon", -180.0, 180.00001),
        ) else {
            return;
        };
        // A vessel leaving the supported area loses its old local position.
        if !(36.7..=37.6).contains(&lat) || !(24.7..=25.6).contains(&lon) {
            self.positions.remove(&id);
            return;
        }
        self.positions
            .retain(|_, p| now - p.received_at <= POSITION_AGE);
        if self.positions.len() >= MAX_POSITIONS
            && !self.positions.contains_key(&id)
            && let Some(oldest) = self
                .positions
                .iter()
                .min_by_key(|(_, p)| p.received_at)
                .map(|(id, _)| *id)
        {
            self.positions.remove(&oldest);
        }
        self.positions.insert(
            id,
            Position {
                lat,
                lon,
                // Message 27 uses whole knots and 63 for unavailable; the
                // other position reports use tenths of knots and 102.3.
                sog: number(frame, "sog", 0.0, if kind == 27 { 63.0 } else { 102.3 }),
                cog: number(frame, "cog", 0.0, 360.0),
                received_at: now,
            },
        );
    }
    fn snapshot(&self, target: &str, now: i64) -> Value {
        let target = operators::normalized_name(target);
        let mut rows: Vec<_> = self
            .positions
            .iter()
            // Reserve the full model/publication window when admitting a
            // position; almost-expired vessels must not invalidate the reply.
            .filter(|(_, p)| {
                p.received_at <= now && now - p.received_at <= POSITION_AGE - SNAPSHOT_AGE
            })
            .map(|(id, p)| {
                let identity = self.identities.get(id).cloned().unwrap_or_default();
                let catalogue = identity.name.as_deref().and_then(operators::identify);
                let named = identity.name.as_ref().is_some_and(|name| {
                    let name = operators::normalized_name(name);
                    name.len() >= 4 && target.contains(&name)
                });
                let ferry = catalogue.is_some()
                    || identity
                        .ship_type
                        .is_some_and(|t| (40..=49).contains(&t) || (60..=69).contains(&t));
                (*id, p, identity, catalogue, named, ferry)
            })
            .collect();
        rows.sort_by(|a, b| {
            b.4.cmp(&a.4)
                .then(b.5.cmp(&a.5))
                .then(b.1.received_at.cmp(&a.1.received_at))
                .then(a.0.cmp(&b.0))
        });
        let valid_until = rows
            .iter()
            .take(10)
            .fold(now + SNAPSHOT_AGE, |deadline, row| {
                deadline.min(row.1.received_at + POSITION_AGE)
            });
        let selected:Vec<_>=rows.into_iter().take(10).map(|(id,p,identity,catalogue,_,ferry)|json!({
            "mmsi":id,"reported_name":identity.name,"ship_type":identity.ship_type,
            "ferry_candidate":ferry,"catalogue_name_match":catalogue.map(|v|v["catalogue_id"].clone()),
            "latitude":p.lat,"longitude":p.lon,"speed_over_ground_knots":p.sog,
            "course_over_ground_degrees":p.cog,"position_received_at_epoch":p.received_at,
            "age_seconds":now-p.received_at,"position_observed_at_epoch":Value::Null,
            "time_basis":"position_frame_received_by_sproyt; transmitter_absolute_time_unknown"
        })).collect();
        let connected = self.connected && self.last_stream_activity.is_some_and(|t| now - t <= 45);
        json!({
            "version":1,"port":"paros","source":"Own AIS receiver network via ship-tracker",
            "fetched_at_epoch":now,"valid_until_epoch":valid_until,
            "stream_connected":connected,"status":if !selected.is_empty(){"recent_position_reports"}else if connected{"no_recent_position_reports"}else{"unavailable_or_warming"},
            "coverage":"partial; absence is not evidence that no vessel is present",
            "vessels":selected,"operator_facts":operators::catalogue(),
            "operator_relationship":"Blue Star Ferries and Hellenic Seaways are distinct Attica Group brands; Seajets is a separate operator",
            "operator_relationship_source":"https://www.attica-group.com/en/group-profile",
            "interpretation":"AIS reports position and movement, not confirmed docking, route, delay or live ETA. User eyewitness statements are separately attributed observations. Operator specifications are not live activity. Match names cautiously; never identify a silhouette from this list alone."
        })
    }
}

impl ObservationService {
    pub(super) fn from_env() -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let Ok(base) = std::env::var("SPROYT_AIS_URL") else {
            return Ok(None);
        };
        if base.is_empty() {
            return Ok(None);
        }
        Self::new(&base).map(Some)
    }
    fn new(base: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let base = Url::parse(base)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err("invalid SPROYT_AIS_URL".into());
        }
        Ok(Self {
            base,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            state: Arc::new(RwLock::new(State::default())),
        })
    }
    pub(super) fn start_worker(&self, mut shutdown: watch::Receiver<bool>) {
        let service = self.clone();
        tokio::spawn(async move {
            let mut failures = 0u32;
            while !*shutdown.borrow() {
                let started = std::time::Instant::now();
                tokio::select! {_=shutdown.changed()=>break,_=service.session()=>{}}
                service.state.write().await.connected = false;
                failures = if started.elapsed() >= Duration::from_secs(60) {
                    0
                } else {
                    failures.saturating_add(1).min(5)
                };
                let pause = Duration::from_secs((1u64 << failures).min(30));
                tokio::select! {_=shutdown.changed()=>break,_=tokio::time::sleep(pause)=>{}}
            }
            service.state.write().await.connected = false;
        });
    }
    pub(super) async fn snapshot(&self, target: &str) -> Value {
        self.state
            .read()
            .await
            .snapshot(target, Utc::now().timestamp())
    }
    async fn metadata(&self) -> Result<(), &'static str> {
        let url = self
            .base
            .join("api/stations")
            .map_err(|_| "ais_configuration")?;
        let mut response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|_| "ais_transport")?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err("ais_contract");
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "ais_transport")? {
            if body.len().saturating_add(chunk.len()) > MAX_BYTES {
                return Err("ais_contract");
            }
            body.extend_from_slice(&chunk);
        }
        let rows = serde_json::from_slice(&body).map_err(|_| "ais_contract")?;
        self.state.write().await.metadata(&rows)
    }
    async fn session(&self) -> Result<(), &'static str> {
        // A metadata failure must not prevent receiving correctly identified
        // MMSI positions. Refresh names at each bounded stream renewal.
        let _ = self.metadata().await;
        let url = self
            .base
            .join("api/events")
            .map_err(|_| "ais_configuration")?;
        let mut response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .map_err(|_| "ais_transport")?;
        if !response.status().is_success()
            || !response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("text/event-stream"))
        {
            return Err("ais_contract");
        }
        {
            let mut state = self.state.write().await;
            state.connected = true;
            state.last_stream_activity = Some(Utc::now().timestamp());
        }
        let mut parser = EventParser::default();
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(45), response.chunk())
                .await
                .map_err(|_| "ais_timeout")?
                .map_err(|_| "ais_transport")?
                .ok_or("ais_closed")?;
            let now = Utc::now().timestamp();
            let events = parser.push(&chunk)?;
            let mut state = self.state.write().await;
            state.last_stream_activity = Some(now);
            for event in events {
                state.frame(&event, now);
            }
        }
    }
}

// Require a complete, bounded SSE record across arbitrary network chunks.
#[derive(Default)]
struct EventParser {
    line: Vec<u8>,
    data: Vec<u8>,
}
impl EventParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>, &'static str> {
        if bytes.len() > MAX_BYTES {
            return Err("ais_event_size");
        }
        let mut events = Vec::new();
        for byte in bytes {
            if *byte != b'\n' {
                if self.line.len() >= MAX_LINE {
                    return Err("ais_event_size");
                }
                self.line.push(*byte);
                continue;
            }
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
            if self.line.is_empty() {
                if !self.data.is_empty() {
                    if let Ok(event) = serde_json::from_slice(&self.data) {
                        events.push(event);
                    }
                    self.data.clear();
                }
            } else if let Some(mut data) = self.line.strip_prefix(b"data:") {
                if data.first() == Some(&b' ') {
                    data = &data[1..];
                }
                if self.data.len().saturating_add(data.len() + 1) > MAX_LINE {
                    return Err("ais_event_size");
                }
                self.data.extend_from_slice(data);
                self.data.push(b'\n');
            }
            self.line.clear();
        }
        Ok(events)
    }
}

pub(super) fn valid_snapshot(value: &Value, now: i64) -> bool {
    value["version"] == 1
        && value["port"] == "paros"
        && value["fetched_at_epoch"]
            .as_i64()
            .is_some_and(|t| t <= now && now - t <= SNAPSHOT_AGE)
        && value["valid_until_epoch"]
            .as_i64()
            .is_some_and(|t| t > now && t <= now + SNAPSHOT_AGE)
        && value["vessels"].as_array().is_some_and(|rows| {
            rows.len() <= 10
                && rows.iter().all(|row| {
                    row["position_received_at_epoch"]
                        .as_i64()
                        .is_some_and(|t| t <= now && now - t <= POSITION_AGE)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_frames_and_station_last_seen_never_refresh_position_age() {
        let mut state = State::default();
        state.metadata(&json!([{"mmsi":240000001,"vessel_name":"BLUE STAR DELOS","last_seen":1000,"lat":37.08,"lon":25.14,"ship_type":60}])).unwrap();
        assert!(state.positions.is_empty());
        state.frame(
            &json!({"type":1,"mmsi":240000001,"lat":37.08,"lon":25.14,"sog":8.2,"cog":90}),
            1000,
        );
        state.frame(
            &json!({"type":5,"mmsi":240000001,"shipname":"BLUE STAR DELOS"}),
            1550,
        );
        assert_eq!(
            state.snapshot("Delos", 1540)["vessels"][0]["age_seconds"],
            540
        );
        assert_eq!(state.snapshot("Delos", 1601)["vessels"], json!([]));
    }
    #[test]
    fn geographic_scope_sentinels_and_identity_are_preserved() {
        let mut state = State::default();
        state.frame(
            &json!({"type":18,"mmsi":240000001,"lat":37.08,"lon":25.14,"sog":102.3,"cog":360}),
            1000,
        );
        let row = state.snapshot("", 1000)["vessels"][0].clone();
        assert!(row["reported_name"].is_null());
        assert!(row["speed_over_ground_knots"].is_null());
        assert!(row["course_over_ground_degrees"].is_null());
        assert!(row["position_observed_at_epoch"].is_null());
        state.frame(&json!({"type":1,"mmsi":240000002,"lat":91,"lon":181}), 1000);
        assert_eq!(state.positions.len(), 1);
        state.frame(
            &json!({"type":1,"mmsi":240000001,"lat":60.3,"lon":5.1}),
            1001,
        );
        assert!(state.positions.is_empty());
    }
    #[test]
    fn sse_chunking_comments_crlf_and_limits() {
        let mut parser = EventParser::default();
        assert!(
            parser
                .push(b":connected\r\ndata: {\"type\":1,\"mmsi\":")
                .unwrap()
                .is_empty()
        );
        assert!(parser.push(b"240000001}\r\n").unwrap().is_empty());
        let events = parser.push(b"\r\n:keepalive\n\n").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["mmsi"], 240000001);
        assert!(parser.push(&vec![b'x'; MAX_LINE + 1]).is_err());
    }
    #[test]
    fn snapshots_expire_on_retry_without_promising_complete_coverage() {
        let mut state = State::default();
        state.frame(
            &json!({"type":1,"mmsi":240000001,"lat":37.08,"lon":25.14}),
            1000,
        );
        let snapshot = state.snapshot("", 1000);
        assert!(valid_snapshot(&snapshot, 1000));
        assert!(!valid_snapshot(&snapshot, 1060));
        assert!(!valid_snapshot(&snapshot, 999));
        assert!(!snapshot["stream_connected"].as_bool().unwrap());
        let expiring = state.snapshot("", 1540);
        assert_eq!(expiring["valid_until_epoch"], 1600);
        assert!(valid_snapshot(&expiring, 1599));
        assert!(!valid_snapshot(&expiring, 1600));
        assert_eq!(state.snapshot("", 1541)["vessels"], json!([]));
        assert_eq!(state.snapshot("", 1600)["vessels"], json!([]));
        assert_eq!(state.snapshot("", 1601)["status"], "unavailable_or_warming");
    }
    #[test]
    fn endpoint_is_server_configured_without_credentials_or_query_overrides() {
        assert!(ObservationService::new("http://localhost:18885/").is_ok());
        for bad in [
            "ftp://host/",
            "http://user:pass@host/",
            "http://host/?url=x",
            "http://host/api/",
        ] {
            assert!(ObservationService::new(bad).is_err());
        }
    }

    #[test]
    fn long_range_speed_sentinel_is_not_reported_as_sixty_three_knots() {
        let mut state = State::default();
        state.frame(
            &json!({"type":27,"mmsi":240000001,"lat":37.08,"lon":25.14,"sog":63,"cog":511}),
            1000,
        );
        let row = state.snapshot("", 1000)["vessels"][0].clone();
        assert!(row["speed_over_ground_knots"].is_null());
        assert!(row["course_over_ground_degrees"].is_null());
        state.frame(
            &json!({"type":27,"mmsi":240000001,"lat":37.08,"lon":25.14,"sog":62}),
            1001,
        );
        assert_eq!(
            state.snapshot("", 1001)["vessels"][0]["speed_over_ground_knots"],
            62.0
        );
    }

    #[tokio::test]
    async fn http_contract_uses_stream_positions_and_never_follows_redirects() {
        use axum::{
            Json, Router,
            body::{Body, Bytes},
            http::{StatusCode, header::CONTENT_TYPE},
            response::IntoResponse,
            routing::get,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        let event_requests = Arc::new(AtomicUsize::new(0));
        let redirects = Arc::new(AtomicUsize::new(0));
        let seen = event_requests.clone();
        let followed = redirects.clone();
        let app = Router::new()
            .route("/api/stations", get(|| async {
                Json(json!([
                    {"mmsi":240000001,"vessel_name":"BLUE STAR DELOS","ship_type":60,"lat":60.3,"lon":5.1,"last_seen":9999999999i64},
                    {"mmsi":240000002,"vessel_name":"ARTEMIS","ship_type":60,"lat":37.08,"lon":25.14,"last_seen":9999999999i64}
                ]))
            }))
            .route("/api/events", get(move || {
                let number = seen.fetch_add(1, Ordering::SeqCst);
                async move {
                    if number > 0 {
                        return (StatusCode::FOUND, [("location", "/redirect-target")]).into_response();
                    }
                    let stream = futures_util::stream::iter([
                        Ok::<_, std::convert::Infallible>(Bytes::from_static(b":connected\r\ndata: {\"type\":1,\"mmsi\":240000001,\"lat\":37.08,")),
                        Ok(Bytes::from_static(b"\"lon\":25.14,\"sog\":8.2}\r\n\r\ndata: {\"type\":5,\"mmsi\":240000002,\"shipname\":\"ARTEMIS\"}\n\n")),
                    ]);
                    ([(CONTENT_TYPE, "text/event-stream")], Body::from_stream(stream)).into_response()
                }
            }))
            .route("/redirect-target", get(move || {
                followed.fetch_add(1, Ordering::SeqCst);
                async { "unexpected" }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let service =
            ObservationService::new(&format!("http://{}/", listener.local_addr().unwrap()))
                .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        assert_eq!(service.session().await, Err("ais_closed"));
        let snapshot = service.snapshot("BLUE STAR DELOS").await;
        assert_eq!(snapshot["vessels"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["vessels"][0]["reported_name"], "BLUE STAR DELOS");
        assert_eq!(snapshot["vessels"][0]["latitude"], 37.08);
        assert_eq!(snapshot["vessels"][0]["speed_over_ground_knots"], 8.2);
        assert!(snapshot["vessels"][0]["position_observed_at_epoch"].is_null());
        assert!(valid_snapshot(&snapshot, Utc::now().timestamp()));
        assert_eq!(service.session().await, Err("ais_contract"));
        assert_eq!(event_requests.load(Ordering::SeqCst), 2);
        assert_eq!(redirects.load(Ordering::SeqCst), 0);
        server.abort();
    }
}
