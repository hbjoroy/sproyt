//! Isolated Geoapify source for explicit bar discovery, independent of Google.
//! Callers must authorize and disclose the external provider before sending a
//! named location or position, revalidate afterwards, and present attribution.
//! A full result page may be truncated: a later random draw is only uniform
//! over the returned, valid candidates, not all bars in the area.
use reqwest::{Client, Url, header::HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};

const GEOCODE_ENDPOINT: &str = "https://api.geoapify.com/v1/mcp";
const PLACES_ENDPOINT: &str = "https://api.geoapify.com/v2/places";
const MAX_RESPONSE: usize = 256 * 1024;
const MAX_PLACES: usize = 100;
const MAX_GEOCODES: usize = 5;
pub(super) const RADIUS_METERS: u32 = 1500;

// Coordinates and provider text deliberately have no Debug implementation.
#[derive(Clone, Serialize)]
pub(super) struct Point {
    pub latitude: f64,
    pub longitude: f64,
}
#[derive(Clone, Serialize)]
pub(super) struct GeocodeCandidate {
    pub label: String,
    pub point: Point,
}
#[derive(Clone, Serialize)]
pub(super) struct Place {
    pub id: String,
    pub name: String,
    pub address: Option<String>,
    pub point: Point,
}
#[derive(Clone, Serialize)]
pub(super) struct PlaceSearch {
    pub places: Vec<Place>,
    pub potentially_truncated: bool,
    pub radius_m: u32,
    pub attribution: Attribution,
}
#[derive(Clone, Serialize)]
pub(super) struct Attribution {
    pub osm_label: &'static str,
    pub osm_url: &'static str,
    pub provider_label: &'static str,
    pub provider_url: &'static str,
}
fn attribution() -> Attribution {
    Attribution {
        osm_label: "© OpenStreetMap contributors",
        osm_url: "https://www.openstreetmap.org/copyright",
        provider_label: "Powered by Geoapify",
        provider_url: "https://www.geoapify.com/",
    }
}

#[derive(Clone)]
pub(super) struct GeoapifyService {
    http: Client,
    key: HeaderValue,
    geocode_endpoint: Url,
    places_endpoint: Url,
}
#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_ENABLED: bool; }

impl GeoapifyService {
    pub(super) fn from_env() -> Result<Option<Self>, &'static str> {
        #[cfg(test)]
        if TEST_ENABLED.try_with(|enabled| *enabled).unwrap_or(false) {
            let endpoint =
                Url::parse("http://127.0.0.1:9/").map_err(|_| "geoapify_configuration")?;
            return Self::new("dummy-geoapify-key-1234", endpoint.clone(), endpoint).map(Some);
        }
        let key = match std::env::var("SPROYT_GEOAPIFY_API_KEY") {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(_) => return Err("geoapify_configuration"),
        };
        Self::new(
            &key,
            Url::parse(GEOCODE_ENDPOINT).map_err(|_| "geoapify_configuration")?,
            Url::parse(PLACES_ENDPOINT).map_err(|_| "geoapify_configuration")?,
        )
        .map(Some)
    }
    fn new(key: &str, geocode_endpoint: Url, places_endpoint: Url) -> Result<Self, &'static str> {
        if key.trim() != key || !(16..=256).contains(&key.len()) {
            return Err("geoapify_configuration");
        }
        let mut header = HeaderValue::from_str(key).map_err(|_| "geoapify_configuration")?;
        header.set_sensitive(true);
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "geoapify_configuration")?;
        Ok(Self {
            http,
            key: header,
            geocode_endpoint,
            places_endpoint,
        })
    }
    /// Return candidates for user selection, never infer a unique origin.
    pub(super) async fn geocode(&self, query: &str) -> Result<Vec<GeocodeCandidate>, &'static str> {
        let query = checked_text(query, 240)?;
        if query.contains("://") || query.contains('@') {
            return Err("geoapify_argument");
        }
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"geocode_address","arguments":{"query":query,"limit":MAX_GEOCODES,"lang":"no"}}});
        let response = self.post(&self.geocode_endpoint, &body).await?;
        if response.get("error").is_some() || response["result"]["isError"].as_bool() == Some(true)
        {
            return Err("geoapify_provider");
        }
        let structured = response["result"]
            .get("structuredContent")
            .ok_or("geoapify_schema")?;
        let results = structured["results"].as_array().ok_or("geoapify_schema")?;
        if results.len() > MAX_GEOCODES {
            return Err("geoapify_schema");
        }
        let mut values = Vec::new();
        for result in results {
            let Some(label) = result["formatted"].as_str() else {
                continue;
            };
            let (Some(lat), Some(lon)) = (result["lat"].as_f64(), result["lon"].as_f64()) else {
                continue;
            };
            let Ok(label) = checked_text(label, 512) else {
                continue;
            };
            let Ok(point) = point(lat, lon) else { continue };
            values.push(GeocodeCandidate {
                label: label.to_owned(),
                point,
            });
        }
        Ok(values)
    }
    /// Restrict the provider query and independently verify its output radius.
    /// The caller must supply coordinates from an explicit location grant or a
    /// user-selected Geoapify geocode candidate, never Google data or a model.
    pub(super) async fn bars(&self, origin: &Point) -> Result<PlaceSearch, &'static str> {
        let origin = point(origin.latitude, origin.longitude)?;
        let body = json!({"categories":["catering.bar","catering.pub"],"filter":{"type":"circle","lat":origin.latitude,"lon":origin.longitude,"radius":RADIUS_METERS},"limit":MAX_PLACES,"lang":"no"});
        let response = self.post(&self.places_endpoint, &body).await?;
        if response["type"].as_str() != Some("FeatureCollection") {
            return Err("geoapify_schema");
        }
        let features = response["features"].as_array().ok_or("geoapify_schema")?;
        if features.len() > MAX_PLACES {
            return Err("geoapify_schema");
        }
        let potentially_truncated = features.len() == MAX_PLACES;
        let mut ids = HashSet::new();
        let mut places = Vec::new();
        for f in features {
            let props = &f["properties"];
            let (Some(id), Some(name)) = (props["place_id"].as_str(), props["name"].as_str())
            else {
                continue;
            };
            let (Ok(id), Ok(name)) = (checked_text(id, 512), checked_text(name, 240)) else {
                continue;
            };
            let Some(cats) = props["categories"].as_array() else {
                continue;
            };
            if !cats
                .iter()
                .any(|cat| matches!(cat.as_str(), Some("catering.bar" | "catering.pub")))
            {
                continue;
            }
            let (Some(lat), Some(lon)) = (props["lat"].as_f64(), props["lon"].as_f64()) else {
                continue;
            };
            let Ok(destination) = point(lat, lon) else {
                continue;
            };
            // Small allowance avoids rejecting points on the boundary due to rounding.
            if distance_m(&origin, &destination) > f64::from(RADIUS_METERS) + 5.0 {
                continue;
            }
            if !ids.insert(id.to_owned()) {
                continue;
            }
            let address = props["formatted"]
                .as_str()
                .and_then(|value| checked_text(value, 512).ok())
                .map(str::to_owned);
            places.push(Place {
                id: id.to_owned(),
                name: name.to_owned(),
                address,
                point: destination,
            });
        }
        Ok(PlaceSearch {
            places,
            potentially_truncated,
            radius_m: RADIUS_METERS,
            attribution: attribution(),
        })
    }
    async fn post(&self, endpoint: &Url, body: &Value) -> Result<Value, &'static str> {
        let mut response = self
            .http
            .post(endpoint.clone())
            .header("x-api-key", self.key.clone())
            .header("accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|_| "geoapify_transport")?;
        if !response.status().is_success() {
            return Err("geoapify_status");
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE as u64)
        {
            return Err("geoapify_too_large");
        }
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "geoapify_transport")? {
            if data.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                return Err("geoapify_too_large");
            }
            data.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&data).map_err(|_| "geoapify_schema")
    }
}
fn checked_text(text: &str, max: usize) -> Result<&str, &'static str> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        Err("geoapify_argument")
    } else {
        Ok(text)
    }
}
fn point(latitude: f64, longitude: f64) -> Result<Point, &'static str> {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err("geoapify_argument");
    }
    Ok(Point {
        latitude,
        longitude,
    })
}
fn distance_m(a: &Point, b: &Point) -> f64 {
    let (a_lat, b_lat) = (a.latitude.to_radians(), b.latitude.to_radians());
    let d_lat = b_lat - a_lat;
    let d_lon = (b.longitude - a.longitude).to_radians();
    let h = (d_lat / 2.0).sin().powi(2) + a_lat.cos() * b_lat.cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.clamp(0.0, 1.0).sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::OriginalUri,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use tokio::sync::mpsc;
    fn place(id: &str, name: &str, lat: f64, lon: f64, category: &str) -> Value {
        json!({"type":"Feature","properties":{"place_id":id,"name":name,"lat":lat,"lon":lon,"formatted":"Somewhere","categories":[category]}})
    }
    async fn mock(
        status: StatusCode,
        response: String,
    ) -> (
        GeoapifyService,
        mpsc::UnboundedReceiver<(String, String, Value)>,
        tokio::task::JoinHandle<()>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let app = Router::new().route(
            "/{*path}",
            post(
                move |headers: HeaderMap,
                      OriginalUri(uri): OriginalUri,
                      Json(body): Json<Value>| {
                    let tx = tx.clone();
                    let response = response.clone();
                    async move {
                        tx.send((
                            uri.to_string(),
                            headers
                                .get("x-api-key")
                                .and_then(|h| h.to_str().ok())
                                .unwrap_or("")
                                .to_owned(),
                            body,
                        ))
                        .unwrap();
                        (status, response)
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (
            GeoapifyService::new(
                "dummy-geoapify-key-1234",
                Url::parse(&format!("{base}/geocode")).unwrap(),
                Url::parse(&format!("{base}/places")).unwrap(),
            )
            .unwrap(),
            rx,
            task,
        )
    }
    #[tokio::test]
    async fn geocode_returns_ambiguous_candidates_and_keeps_key_and_query_out_of_url() {
        let value = json!({"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"results":[{"formatted":"Central station, Copenhagen","lat":55.67,"lon":12.56},{"formatted":"Central station, Malmo","lat":55.61,"lon":13.00}]}}});
        let (service, mut received, task) = mock(StatusCode::OK, value.to_string()).await;
        let candidates = service.geocode("Central station").await.unwrap();
        assert_eq!(candidates.len(), 2);
        let (uri, key, body) = received.recv().await.unwrap();
        assert_eq!(uri, "/geocode");
        assert_eq!(key, "dummy-geoapify-key-1234");
        assert_eq!(body["params"]["name"], "geocode_address");
        assert_eq!(body["params"]["arguments"]["query"], "Central station");
        task.abort();
    }
    #[tokio::test]
    async fn bars_enforce_circle_categories_and_deduplicate() {
        let value = json!({"type":"FeatureCollection","features":[place("one","Pub",55.67,12.56,"catering.pub"),place("one","Pub duplicate",55.67,12.56,"catering.pub"),place("two","Not a bar",55.67,12.56,"catering.restaurant"),place("three","Too far",60.0,12.56,"catering.bar")]});
        let (service, mut received, task) = mock(StatusCode::OK, value.to_string()).await;
        let results = service.bars(&point(55.67, 12.56).unwrap()).await.unwrap();
        assert_eq!(results.places.len(), 1);
        assert_eq!(results.places[0].id, "one");
        assert!(!results.potentially_truncated);
        assert_eq!(
            results.attribution.osm_url,
            "https://www.openstreetmap.org/copyright"
        );
        let (uri, key, body) = received.recv().await.unwrap();
        assert_eq!(uri, "/places");
        assert_eq!(key, "dummy-geoapify-key-1234");
        assert_eq!(body["categories"], json!(["catering.bar", "catering.pub"]));
        assert_eq!(body["filter"]["radius"], 1500);
        assert_eq!(body["filter"]["lat"], 55.67);
        assert_eq!(body["filter"]["lon"], 12.56);
        assert_eq!(body["limit"], 100);
        task.abort();
    }
    #[tokio::test]
    async fn full_page_reports_potential_truncation_and_missing_names_are_ignored() {
        let features: Vec<_> = (0..100)
            .map(|n| place(&format!("id-{n}"), "Pub", 55.67, 12.56, "catering.pub"))
            .collect();
        let (service, _received, task) = mock(
            StatusCode::OK,
            json!({"type":"FeatureCollection","features":features}).to_string(),
        )
        .await;
        let result = service.bars(&point(55.67, 12.56).unwrap()).await.unwrap();
        assert!(result.potentially_truncated);
        assert_eq!(result.places.len(), 100);
        task.abort();
    }
    #[tokio::test]
    async fn errors_redirect_and_oversized_response_are_bounded() {
        for (status, content, expected) in [
            (StatusCode::UNAUTHORIZED, "{}".to_owned(), "geoapify_status"),
            (StatusCode::FOUND, "{}".to_owned(), "geoapify_status"),
            (
                StatusCode::OK,
                "x".repeat(MAX_RESPONSE + 1),
                "geoapify_too_large",
            ),
            (StatusCode::OK, "not json".to_owned(), "geoapify_schema"),
        ] {
            let (service, _received, task) = mock(status, content).await;
            assert_eq!(service.geocode("Station").await.err().unwrap(), expected);
            task.abort();
        }
    }
    #[tokio::test]
    async fn invalid_query_and_coordinates_never_reach_provider() {
        let (service, mut received, task) = mock(StatusCode::OK, "{}".to_owned()).await;
        for query in [
            "",
            "https://example.org",
            "person@example.org",
            "line\nbreak",
        ] {
            assert_eq!(
                service.geocode(query).await.err().unwrap(),
                "geoapify_argument"
            );
        }
        assert_eq!(
            service
                .bars(&Point {
                    latitude: f64::NAN,
                    longitude: 0.0
                })
                .await
                .err()
                .unwrap(),
            "geoapify_argument"
        );
        assert!(received.try_recv().is_err());
        task.abort();
    }
    #[tokio::test]
    async fn network_timeout_is_bounded_and_does_not_expose_details() {
        let app = Router::new().route(
            "/geocode",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                "{}"
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "http://{}/geocode",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut service =
            GeoapifyService::new("dummy-geoapify-key-1234", url.clone(), url).unwrap();
        service.http = Client::builder()
            .timeout(Duration::from_millis(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        assert_eq!(
            service.geocode("Station").await.err().unwrap(),
            "geoapify_transport"
        );
        task.abort();
    }
    #[tokio::test]
    async fn redirect_does_not_forward_api_key() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let followed = Arc::new(AtomicUsize::new(0));
        let counter = followed.clone();
        let app = Router::new()
            .route(
                "/geocode",
                post(|| async {
                    (
                        StatusCode::FOUND,
                        [("location", "/redirected")],
                        "redirection",
                    )
                }),
            )
            .route(
                "/redirected",
                post(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        "{}"
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "http://{}/geocode",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = GeoapifyService::new("dummy-geoapify-key-1234", url.clone(), url).unwrap();
        assert_eq!(
            service.geocode("Station").await.err().unwrap(),
            "geoapify_status"
        );
        assert_eq!(followed.load(Ordering::SeqCst), 0);
        task.abort();
    }
}
