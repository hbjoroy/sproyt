//! Read-only, bounded Weather-Service adapter. No model-selected URLs or GPS.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WeatherConfig {
    pub location: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl WeatherConfig {
    pub(super) fn validate(&self) -> Result<()> {
        if self.location.trim().is_empty()
            || self.location.chars().count() > 80
            || self.location.chars().any(char::is_control)
            || !self.latitude.is_finite()
            || !self.longitude.is_finite()
            || !(-90.0..=90.0).contains(&self.latitude)
            || !(-180.0..=180.0).contains(&self.longitude)
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct WeatherService {
    base: Url,
    http: reqwest::Client,
}

impl WeatherService {
    pub(super) fn from_env()
    -> std::result::Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let Ok(base) = std::env::var("SPROYT_WEATHER_URL") else {
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
            return Err("invalid SPROYT_WEATHER_URL".into());
        }
        Ok(Self {
            base,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    async fn get(
        &self,
        path: &str,
        config: &WeatherConfig,
    ) -> std::result::Result<Value, &'static str> {
        self.get_location(path, &format!("{},{}", config.latitude, config.longitude))
            .await
    }

    async fn get_location(
        &self,
        path: &str,
        location: &str,
    ) -> std::result::Result<Value, &'static str> {
        let mut url = self.base.join(path).map_err(|_| "weather_configuration")?;
        url.query_pairs_mut().append_pair("location", location);
        if path == "forecast" {
            url.query_pairs_mut()
                .append_pair("days", "3")
                .append_pair("include_hourly", "true")
                .append_pair("include_aqi", "false")
                .append_pair("include_alerts", "false");
        }
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| "weather_transport")?
            .error_for_status()
            .map_err(|_| "weather_status")?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "weather_transport")? {
            if bytes.len() + chunk.len() > 256 * 1024 {
                return Err("weather_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "weather_invalid_json")
    }

    pub(super) async fn snapshot(
        &self,
        config: &WeatherConfig,
    ) -> std::result::Result<Value, &'static str> {
        let (current, forecast) =
            tokio::try_join!(self.get("current", config), self.get("forecast", config))?;
        normalize(config, &current, &forecast, Utc::now().timestamp())
    }

    /// Resolve an explicitly named place through the configured provider only.
    pub(super) async fn snapshot_location(
        &self,
        location: &str,
    ) -> std::result::Result<Value, &'static str> {
        validate_location(location)?;
        let requested = location.trim();
        let paros = has_paros_qualifier(requested);
        let aliki = paros && is_paros_aliki(requested);
        // This provider-verified locality point is server-owned, never model
        // coordinates or user GPS. Name searches otherwise select mainland Aliki.
        let lookup = if aliki { "36.997,25.137" } else { requested };
        let current = self.get_location("current", lookup).await?;
        let resolved = &current["location"];
        let field = |key: &str, required: bool| {
            resolved[key]
                .as_str()
                .filter(|s| {
                    (!required || !s.trim().is_empty())
                        && s.chars().count() <= 80
                        && !s.chars().any(char::is_control)
                })
                .ok_or("weather_location")
        };
        let name = field("name", true)?;
        let region = field("region", false)?;
        let country = field("country", true)?;
        let config = WeatherConfig {
            location: name.to_owned(),
            latitude: resolved["lat"].as_f64().ok_or("weather_location")?,
            longitude: resolved["lon"].as_f64().ok_or("weather_location")?,
        };
        config.validate().map_err(|_| "weather_location")?;
        if paros
            && (!country.eq_ignore_ascii_case("Greece")
                || !(36.95..=37.30).contains(&config.latitude)
                || !(25.04..=25.35).contains(&config.longitude)
                || (aliki
                    && ((config.latitude - 36.997).abs() > 0.01
                        || (config.longitude - 25.137).abs() > 0.01)))
        {
            return Err("weather_location");
        }
        let forecast = self.get("forecast", &config).await?;
        let mut snapshot = normalize(&config, &current, &forecast, Utc::now().timestamp())?;
        snapshot
            .as_object_mut()
            .unwrap()
            .remove("configured_location");
        snapshot["requested_location"] = json!(requested);
        snapshot["lookup_basis"] = json!(if aliki {
            "verified_paros_aliki_coordinates"
        } else {
            "provider_named_place"
        });
        snapshot["resolved_location"] = json!({
            "name": name, "region": region, "country": country,
            "latitude": config.latitude, "longitude": config.longitude,
        });
        Ok(snapshot)
    }
}

fn has_paros_qualifier(location: &str) -> bool {
    location
        .to_lowercase()
        .split(|character: char| !character.is_alphabetic())
        .any(|word| matches!(word, "paros" | "πάρος"))
}

fn is_paros_aliki(location: &str) -> bool {
    let first = location
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let locality = first
        .strip_suffix(" paros")
        .or_else(|| first.strip_suffix(" πάρος"))
        .unwrap_or(&first)
        .trim();
    matches!(locality, "aliki" | "alyki" | "αλυκή")
}

pub(super) fn validate_location(location: &str) -> std::result::Result<(), &'static str> {
    // WeatherAPI also accepts URLs, IP addresses, coordinates and auto:ip.
    // This entry point intentionally accepts only explicit names.
    let trimmed = location.trim();
    if trimmed.is_empty()
        || location.chars().count() > 80
        || location.chars().any(char::is_control)
        || location
            .chars()
            .any(|c| matches!(c, ':' | '/' | '\\' | '@' | '?' | '#'))
        || trimmed.parse::<std::net::IpAddr>().is_ok()
        || !trimmed.chars().any(char::is_alphabetic)
    {
        return Err("weather_location");
    }
    Ok(())
}

fn number(value: &Value, key: &str, min: f64, max: f64) -> Value {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (min..=max).contains(v))
        .map_or(Value::Null, |n| json!(n))
}

fn normalize(
    config: &WeatherConfig,
    current: &Value,
    forecast: &Value,
    now: i64,
) -> std::result::Result<Value, &'static str> {
    let location = &current["location"];
    let latitude = location["lat"].as_f64().ok_or("weather_location")?;
    let longitude = location["lon"].as_f64().ok_or("weather_location")?;
    if !latitude.is_finite()
        || !longitude.is_finite()
        || (latitude - config.latitude).abs() > 0.25
        || (longitude - config.longitude).abs() > 0.25
    {
        return Err("weather_location");
    }
    let timezone = location["tz_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .ok_or("weather_timezone")?;
    timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| "weather_timezone")?;
    let forecast_location = &forecast["location"];
    let forecast_lat = forecast_location["lat"]
        .as_f64()
        .ok_or("weather_location")?;
    let forecast_lon = forecast_location["lon"]
        .as_f64()
        .ok_or("weather_location")?;
    if !forecast_lat.is_finite()
        || !forecast_lon.is_finite()
        || (forecast_lat - latitude).abs() > 0.01
        || (forecast_lon - longitude).abs() > 0.01
        || forecast_location["tz_id"].as_str() != Some(timezone)
    {
        return Err("weather_location");
    }
    let observed = current["current"]["last_updated_epoch"]
        .as_i64()
        .ok_or("weather_timestamp")?;
    if observed > now + 300 || observed < now - 7200 {
        return Err("weather_stale");
    }
    let current = &current["current"];
    let days = forecast["forecast"]["forecastday"]
        .as_array()
        .ok_or("weather_forecast")?;
    // The hourly UV/pressure contract is required before this capability is usable.
    let mut hours = Vec::new();
    for day in days.iter().take(3) {
        for hour in day["hour"].as_array().ok_or("weather_forecast")? {
            let time = hour["time_epoch"].as_i64().ok_or("weather_timestamp")?;
            if time < now - 3600 || time > now + 72 * 3600 {
                continue;
            }
            if hour.get("uv").is_none() || hour.get("pressure_mb").is_none() {
                return Err("weather_contract");
            }
            if hours.len() >= 73 {
                return Err("weather_forecast");
            }
            hours.push(json!({"time_epoch":time,"temperature_c":number(hour,"temp_c",-100.0,70.0),
                "uv":number(hour,"uv",0.0,30.0),"pressure_hpa":number(hour,"pressure_mb",800.0,1200.0),
                "wind_kph":number(hour,"wind_kph",0.0,400.0),"rain_chance_percent":number(hour,"chance_of_rain",0.0,100.0)}));
        }
    }
    if hours.is_empty() {
        return Err("weather_forecast");
    }
    hours.sort_by_key(|hour| hour["time_epoch"].as_i64());
    let baseline_pressure = number(current, "pressure_mb", 800.0, 1200.0).as_f64();
    for hour in &mut hours {
        hour["pressure_change_from_current_hpa"] =
            match (baseline_pressure, hour["pressure_hpa"].as_f64()) {
                (Some(before), Some(after)) => json!(((after - before) * 10.0).round() / 10.0),
                _ => Value::Null,
            };
    }
    Ok(
        json!({"source":"Weather-Service / WeatherAPI","configured_location":config.location,
        "latitude":latitude,"longitude":longitude,"timezone":timezone,"fetched_at_epoch":now,
        "valid_until_epoch":now+300,"observed_at_epoch":observed,
        "current":{"temperature_c":number(current,"temp_c",-100.0,70.0),"uv":number(current,"uv",0.0,30.0),
            "pressure_hpa":number(current,"pressure_mb",800.0,1200.0),"wind_kph":number(current,"wind_kph",0.0,400.0)},
        "hourly_forecast":hours}),
    )
}

pub(super) fn followup_candidate(text: &str) -> bool {
    text.contains('?')
        || matches_trigger(
            text,
            &[
                "UV".into(),
                "lufttrykk".into(),
                "trykk".into(),
                "vind".into(),
                "vêr".into(),
                "vær".into(),
                "weather".into(),
                "pressure".into(),
                "rain".into(),
            ],
        )
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires explicitly configured Weather-Service tunnel"]
    async fn live_paros_localities_resolve_on_the_island() {
        let service = WeatherService::from_env()
            .unwrap()
            .expect("SPROYT_WEATHER_URL required");
        for (requested, basis) in [
            ("Naoussa, Paros, Greece", "provider_named_place"),
            ("Aliki, Paros, Greece", "verified_paros_aliki_coordinates"),
        ] {
            let snapshot = service.snapshot_location(requested).await.unwrap();
            assert_eq!(snapshot["requested_location"], requested);
            assert_eq!(snapshot["lookup_basis"], basis);
            assert_eq!(snapshot["resolved_location"]["country"], "Greece");
            println!("{requested}: {}", snapshot["resolved_location"]);
        }
    }
    use super::*;
    #[tokio::test]
    async fn named_places_reject_non_names_before_network_access() {
        let service = WeatherService::new("http://127.0.0.1:1/").unwrap();
        for location in [
            "",
            "   ",
            "Bergen\nNorway",
            "Bergen\tNorway",
            "https://example.org/",
            "//example.org",
            "auto:ip",
            "127.0.0.1",
            "::1",
            "37.09,25.15",
        ] {
            assert_eq!(
                service.snapshot_location(location).await,
                Err("weather_location")
            );
        }
        assert_eq!(validate_location(&"a".repeat(81)), Err("weather_location"));
        assert_eq!(validate_location(" Bjorøy, Norway "), Ok(()));
    }

    async fn named_place_fixture(
        current: Value,
        forecast: Value,
    ) -> std::result::Result<Value, &'static str> {
        location_fixture(
            "St. John's & Harbour, Canada",
            "St. John's & Harbour, Canada",
            current,
            forecast,
        )
        .await
    }

    async fn location_fixture(
        requested: &str,
        expected_lookup: &str,
        current: Value,
        forecast: Value,
    ) -> std::result::Result<Value, &'static str> {
        use axum::{Json, Router, extract::Query, routing::get};
        use std::collections::HashMap;
        let expected_lookup = expected_lookup.to_owned();
        let forecast_lookup = format!(
            "{},{}",
            current["location"]["lat"].as_f64().unwrap(),
            current["location"]["lon"].as_f64().unwrap()
        );
        let app = Router::new()
            .route(
                "/current",
                get(
                    move |Query(query): Query<HashMap<String, String>>| async move {
                        assert_eq!(query.len(), 1);
                        assert_eq!(query["location"], expected_lookup);
                        Json(current)
                    },
                ),
            )
            .route(
                "/forecast",
                get(
                    move |Query(query): Query<HashMap<String, String>>| async move {
                        assert_eq!(query["location"], forecast_lookup);
                        assert_eq!(query["days"], "3");
                        assert_eq!(query["include_hourly"], "true");
                        Json(forecast)
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let service =
            WeatherService::new(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = service.snapshot_location(requested).await;
        server.abort();
        result
    }

    #[tokio::test]
    async fn explicit_paros_places_reject_off_island_resolution_and_aliki_aliases_use_verified_point()
     {
        let now = Utc::now().timestamp();
        let location = json!({"name":"Alyki","region":"South Aegean","country":"Greece",
            "lat":36.997,"lon":25.137,"tz_id":"Europe/Athens"});
        let current = json!({"location":location,"current":{"last_updated_epoch":now,"uv":2.0,"pressure_mb":1012.0}});
        let forecast = json!({"location":location,"forecast":{"forecastday":[{"hour":[{"time_epoch":now+3600,"uv":4.0,"pressure_mb":1008.5}]}]}});
        for requested in [
            "Aliki,Paros,Greece",
            "Alyki, Paros",
            "Αλυκή, Πάρος",
            "ALIKI PAROS",
        ] {
            let snapshot = location_fixture(
                requested,
                "36.997,25.137",
                current.clone(),
                forecast.clone(),
            )
            .await
            .unwrap();
            assert_eq!(snapshot["requested_location"], requested);
            assert_eq!(snapshot["resolved_location"]["name"], "Alyki");
            assert_eq!(snapshot["resolved_location"]["region"], "South Aegean");
            assert_eq!(snapshot["lookup_basis"], "verified_paros_aliki_coordinates");
        }
        for (requested, key, wrong_value) in [
            ("Naoussa, Paros, Greece", "lat", json!(38.2)),
            ("Naoussa, Πάρος", "lon", json!(23.05)),
            ("Naoussa, Paros", "country", json!("Armenia")),
        ] {
            let mut wrong = current.clone();
            wrong["location"][key] = wrong_value;
            assert_eq!(
                location_fixture(requested, requested, wrong, forecast.clone()).await,
                Err("weather_location")
            );
        }
        for requested in [
            "Naoussa, Paros, Greece",
            "Aliki Beach, Paros",
            "Aliki near Paros",
            "Aliki and Naoussa, Paros",
        ] {
            let snapshot =
                location_fixture(requested, requested, current.clone(), forecast.clone())
                    .await
                    .unwrap();
            assert_eq!(snapshot["lookup_basis"], "provider_named_place");
        }
        assert!(!has_paros_qualifier("Parosville"));
        assert!(!is_paros_aliki("Naoussa, Aliki, Paros"));
    }

    #[tokio::test]
    async fn named_places_report_provider_resolution_and_reject_invalid_or_mismatched_responses() {
        let now = Utc::now().timestamp();
        let location = json!({"name":"St. John's", "region":"Newfoundland and Labrador", "country":"Canada",
            "lat":47.56, "lon":-52.71, "tz_id":"America/St_Johns"});
        let current = json!({"location":location,"current":{"last_updated_epoch":now,"uv":2.0,"pressure_mb":1012.0}});
        let forecast = json!({"location":location,"forecast":{"forecastday":[{"hour":[{"time_epoch":now+3600,"uv":4.0,"pressure_mb":1008.5}]}]}});
        let snapshot = named_place_fixture(current.clone(), forecast.clone())
            .await
            .unwrap();
        assert_eq!(
            snapshot["requested_location"],
            "St. John's & Harbour, Canada"
        );
        assert_eq!(snapshot["resolved_location"]["name"], "St. John's");
        assert_eq!(
            snapshot["resolved_location"]["region"],
            "Newfoundland and Labrador"
        );
        assert_eq!(snapshot["resolved_location"]["country"], "Canada");
        assert_eq!(snapshot["resolved_location"]["latitude"], 47.56);
        assert!(snapshot.get("configured_location").is_none());
        for (key, value) in [
            ("lat", json!(91.0)),
            ("lon", json!(-181.0)),
            ("name", json!("")),
            ("country", json!("Canada\n")),
        ] {
            let mut invalid = current.clone();
            invalid["location"][key] = value;
            assert_eq!(
                named_place_fixture(invalid, forecast.clone()).await,
                Err("weather_location")
            );
        }
        let mut mismatch = forecast.clone();
        mismatch["location"]["lat"] = json!(48.0);
        assert_eq!(
            named_place_fixture(current.clone(), mismatch).await,
            Err("weather_location")
        );
        let mut mismatch = forecast.clone();
        mismatch["location"]["tz_id"] = json!("America/Toronto");
        assert_eq!(
            named_place_fixture(current.clone(), mismatch).await,
            Err("weather_location")
        );
        let mut stale = current.clone();
        stale["current"]["last_updated_epoch"] = json!(now - 7201);
        assert_eq!(
            named_place_fixture(stale, forecast).await,
            Err("weather_stale")
        );
    }

    #[test]
    fn grounded_weather_rejects_stale_wrong_location_and_missing_contract() {
        let config = WeatherConfig {
            location: "Parikia".into(),
            latitude: 37.085,
            longitude: 25.148,
        };
        let current = json!({"location":{"lat":37.09,"lon":25.15,"tz_id":"Europe/Athens"},
            "current":{"last_updated_epoch":10000,"temp_c":23.0,"uv":4.0,"pressure_mb":1012.0}});
        let forecast = json!({"location":{"lat":37.09,"lon":25.15,"tz_id":"Europe/Athens"},"forecast":{"forecastday":[{"hour":[{"time_epoch":10800,"uv":5.0,"pressure_mb":1008.0}]}]}});
        let snapshot = normalize(&config, &current, &forecast, 10300).unwrap();
        assert_eq!(snapshot["current"]["pressure_hpa"], 1012.0);
        assert_eq!(snapshot["hourly_forecast"][0]["uv"], 5.0);
        assert!(normalize(&config, &current, &forecast, 20000).is_err());
        let mut wrong = current.clone();
        wrong["location"]["lat"] = json!(60.0);
        assert!(normalize(&config, &wrong, &forecast, 10300).is_err());
        let mut invalid_timezone = current.clone();
        invalid_timezone["location"]["tz_id"] = json!("Unknown/Timezone");
        assert_eq!(
            normalize(&config, &invalid_timezone, &forecast, 10300),
            Err("weather_timezone")
        );
        let mut old_contract = forecast.clone();
        old_contract["forecast"]["forecastday"][0]["hour"][0]
            .as_object_mut()
            .unwrap()
            .remove("uv");
        assert!(normalize(&config, &current, &old_contract, 10300).is_err());
    }
    #[test]
    fn configuration_and_followup_are_bounded() {
        assert!(WeatherService::new("http://weather.local/").is_ok());
        assert!(WeatherService::new("http://user:password@weather.local/").is_err());
        assert!(WeatherService::new("http://weather.local/?location=other").is_err());
        assert!(followup_candidate("Og i morgon?"));
        assert!(followup_candidate("UV-indeksen då"));
        assert!(!followup_candidate("Skål!"));
        assert!(
            WeatherConfig {
                location: "Her".into(),
                latitude: f64::NAN,
                longitude: 0.0
            }
            .validate()
            .is_err()
        );
    }

    #[tokio::test]
    async fn http_adapter_uses_fixed_routes_and_rejects_redirects_and_oversized_bodies() {
        use axum::{Json, Router, extract::Query, http::StatusCode, routing::get};
        use std::collections::HashMap;
        let now = Utc::now().timestamp();
        let location = json!({"lat":37.09,"lon":25.15,"tz_id":"Europe/Athens"});
        let current = json!({"location":location,"current":{"last_updated_epoch":now,"pressure_mb":1012.0,"uv":2.0}});
        let forecast = json!({"location":location,"forecast":{"forecastday":[{"hour":[{"time_epoch":now+3600,"uv":4.0,"pressure_mb":1008.5}]}]}});
        let app = Router::new()
            .route(
                "/current",
                get(
                    move |Query(query): Query<HashMap<String, String>>| async move {
                        assert_eq!(query["location"], "37.085,25.148");
                        Json(current)
                    },
                ),
            )
            .route(
                "/forecast",
                get(
                    move |Query(query): Query<HashMap<String, String>>| async move {
                        assert_eq!(query["days"], "3");
                        assert_eq!(query["include_hourly"], "true");
                        Json(forecast)
                    },
                ),
            )
            .route(
                "/redirect",
                get(|| async {
                    (
                        StatusCode::FOUND,
                        [("location", "http://untrusted.invalid/")],
                    )
                }),
            )
            .route("/large", get(|| async { " ".repeat(256 * 1024 + 1) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = WeatherService::new(&base).unwrap();
        let config = WeatherConfig {
            location: "Parikia".into(),
            latitude: 37.085,
            longitude: 25.148,
        };
        let snapshot = service.snapshot(&config).await.unwrap();
        assert_eq!(
            snapshot["hourly_forecast"][0]["pressure_change_from_current_hpa"],
            -3.5
        );
        assert!(service.get("redirect", &config).await.is_err());
        assert!(matches!(
            service.get("large", &config).await,
            Err("weather_response_too_large")
        ));
        server.abort();
    }
}
