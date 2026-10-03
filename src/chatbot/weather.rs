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
        let mut url = self.base.join(path).map_err(|_| "weather_configuration")?;
        url.query_pairs_mut().append_pair(
            "location",
            &format!("{},{}", config.latitude, config.longitude),
        );
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
    use super::*;
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
