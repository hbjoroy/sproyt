//! One bounded round of model-selected, read-only tools. Capability snapshots
//! come from the job's authorized configuration, never the agent's name.
use super::*;

pub(super) struct ReadTools<'a> {
    pub weather_service: Option<&'a weather::WeatherService>,
    pub weather: Option<Value>,
    pub ferry: Option<Value>,
    pub observations: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WeatherArgs {
    location: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FerryArgs {
    #[serde(default)]
    vessel: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

pub(super) fn validate_arguments(
    name: &str,
    arguments: &str,
) -> std::result::Result<(), &'static str> {
    if arguments.len() > 1024 {
        return Err("model_invalid_tool");
    }
    match name {
        "lookup_weather" => {
            let args: WeatherArgs =
                serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
            weather::validate_location(&args.location).map_err(|_| "model_invalid_tool")
        }
        "ferry_calls" => {
            let args: FerryArgs =
                serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
            if args.vessel.is_some_and(|vessel| {
                vessel.trim().is_empty()
                    || vessel.chars().count() > 120
                    || vessel.chars().any(char::is_control)
            }) {
                Err("model_invalid_tool")
            } else {
                Ok(())
            }
        }
        "vessel_observations" => serde_json::from_str::<EmptyArgs>(arguments)
            .map(|_| ())
            .map_err(|_| "model_invalid_tool"),
        _ => Err("model_invalid_tool"),
    }
}

impl ReadTools<'_> {
    pub fn definitions(&self) -> Vec<Value> {
        let mut tools = Vec::new();
        if self.weather.is_some() {
            tools.push(json!({"type":"function","function":{
                "name":"lookup_weather","description":"Current weather and next 3 days for an explicitly named place. Use the configured weather context for the default place. Name ambiguous places with island/country; results identify the place actually resolved. No GPS or automatic IP location.",
                "parameters":{"type":"object","properties":{"location":{"type":"string","maxLength":80}},"required":["location"],"additionalProperties":false}}}));
        }
        if self.ferry.is_some() {
            tools.push(json!({"type":"function","function":{
                "name":"ferry_calls","description":"Full current-day planned port calls at Parikia, Paros, including routes and operators. Optional vessel name filter. Use for the day's overview or a vessel outside the default 3 next/recent calls. Not actual arrivals, delays, cancellations or live ETA.",
                "parameters":{"type":"object","properties":{"vessel":{"type":"string","maxLength":120}},"additionalProperties":false}}}));
        }
        if self.observations.is_some() {
            tools.push(json!({"type":"function","function":{
                "name":"vessel_observations","description":"The authorized partial AIS observations near Paros received for this message. A position is not confirmation of docking, arrival, departure or delay; missing vessels are unknown.",
                "parameters":{"type":"object","properties":{},"additionalProperties":false}}}));
        }
        tools
    }

    pub async fn execute(
        &mut self,
        name: &str,
        arguments: &str,
    ) -> std::result::Result<Value, &'static str> {
        if arguments.len() > 1024 {
            return Err("model_invalid_tool");
        }
        let mut result = match name {
            "lookup_weather" if self.weather.is_some() => {
                let args: WeatherArgs =
                    serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
                weather::validate_location(&args.location).map_err(|_| "model_invalid_tool")?;
                let data = match self.weather_service {
                    Some(service) => service.snapshot_location(&args.location).await.ok(),
                    None => None,
                };
                let mut data = data.unwrap_or_else(|| {
                    unavailable_snapshot("Weather-Service / WeatherAPI", &args.location, None)
                });
                data["requested_location"] = json!(args.location);
                let mut evidence = data.clone();
                if let Some(default) = self.weather.take() {
                    // The initial model input included the configured location.
                    // Keep both sources and fence the earliest expiry, even
                    // when the reply compares their conditions.
                    let expiry = [
                        default["valid_until_epoch"].as_i64(),
                        data["valid_until_epoch"].as_i64(),
                    ]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(0);
                    evidence["default_weather"] = default;
                    evidence["valid_until_epoch"] = json!(expiry);
                }
                evidence["tool_selection"] =
                    json!({"name":name,"arguments":{"location":args.location}});
                self.weather = Some(evidence);
                data
            }
            "ferry_calls" if self.ferry.is_some() => {
                let args: FerryArgs =
                    serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
                if let Some(vessel) = &args.vessel
                    && (vessel.trim().is_empty()
                        || vessel.chars().count() > 120
                        || vessel.chars().any(char::is_control))
                {
                    return Err("model_invalid_tool");
                }
                let mut data = self.ferry.clone().ok_or("model_invalid_tool")?;
                if let Some(vessel) = args.vessel {
                    let needle = folded(&vessel);
                    let calls: Vec<_> = data["calls"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|call| {
                            folded(call["vessel"].as_str().unwrap_or_default()).contains(&needle)
                        })
                        .cloned()
                        .collect();
                    data["requested_vessel"] = json!(vessel);
                    data["calls"] = json!(calls);
                }
                data["selection"] = json!("model_requested_current_day_calls");
                data["returned_count"] = json!(data["calls"].as_array().map_or(0, Vec::len));
                // Retain the full source snapshot for publication; record the
                // selection as metadata, without changing its freshness fence.
                if let Some(snapshot) = self.ferry.as_mut() {
                    snapshot["tool_selection"] = json!({"vessel":data["requested_vessel"],"returned_count":data["returned_count"]});
                }
                data
            }
            "vessel_observations" if self.observations.is_some() => {
                let _: EmptyArgs =
                    serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
                self.observations.clone().ok_or("model_invalid_tool")?
            }
            _ => return Err("model_invalid_tool"),
        };
        if result.to_string().len() > 64 * 1024 {
            // Explicitly unavailable, never silently truncate a day overview.
            result = json!({"status":"unavailable","reason":"tool_result_too_large","note":"The requested result exceeds the safe context limit. Do not infer missing facts."});
        }
        Ok(result)
    }
}

fn folded(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capabilities_and_strict_arguments_bound_tool_execution() {
        let mut tools = ReadTools {
            weather_service: None,
            weather: None,
            ferry: None,
            observations: None,
        };
        assert!(tools.definitions().is_empty());
        assert!(
            tools
                .execute("lookup_weather", r#"{"location":"Athens"}"#)
                .await
                .is_err()
        );
        tools.ferry = Some(
            json!({"calls":[{"vessel":"Blue Star Delos","full_route":["Piraeus","Paros"]},{"vessel":"Paros Jet"}],"valid_until_epoch":123}),
        );
        let day = tools.execute("ferry_calls", "{}").await.unwrap();
        assert_eq!(day["returned_count"], 2);
        let vessel = tools
            .execute("ferry_calls", r#"{"vessel":"DELOS"}"#)
            .await
            .unwrap();
        assert_eq!(vessel["returned_count"], 1);
        assert_eq!(vessel["calls"][0]["full_route"][1], "Paros");
        assert_eq!(
            tools.ferry.as_ref().unwrap()["calls"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(tools.ferry.as_ref().unwrap()["valid_until_epoch"], 123);
        assert!(
            tools
                .execute("ferry_calls", r#"{"date":"tomorrow"}"#)
                .await
                .is_err()
        );
        assert!(tools.execute("vessel_observations", "{}").await.is_err());
        assert!(tools.execute("unknown", "{}").await.is_err());
    }

    #[tokio::test]
    async fn failed_place_lookup_never_substitutes_default_and_preserves_earliest_freshness() {
        let expiry = Utc::now().timestamp() + 10;
        let default = json!({"configured_location":"Parikia","current":{"temperature_c":22},"valid_until_epoch":expiry});
        let mut tools = ReadTools {
            weather_service: None,
            weather: Some(default.clone()),
            ferry: None,
            observations: None,
        };
        let result = tools
            .execute("lookup_weather", r#"{"location":"Bergen, Norway"}"#)
            .await
            .unwrap();
        assert_eq!(result["status"], "unavailable");
        assert_eq!(result["requested_location"], "Bergen, Norway");
        assert!(result.get("current").is_none());
        let evidence = tools.weather.unwrap();
        assert_eq!(evidence["default_weather"], default);
        assert_eq!(evidence["valid_until_epoch"], expiry);
    }

    async fn model_exchange(
        agent: &str,
        calls: Value,
    ) -> (std::result::Result<String, &'static str>, Vec<Value>) {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        let captured = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
        let record = captured.clone();
        let app = Router::new()
            .route("/v1/models", get(|| async { Json(json!({"data":[{"id":"qwen-test"}]})) }))
            .route("/v1/chat/completions", post(move |Json(request): Json<Value>| {
                let record = record.clone();
                let calls = calls.clone();
                async move {
                    let mut requests = record.lock().await;
                    requests.push(request);
                    if requests.len() == 1 {
                        Json(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":calls}}]}))
                    } else {
                        Json(json!({"choices":[{"message":{"content":"Delos har planlagt anløp frå Naxos.","tool_calls":[]}}]}))
                    }
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let model = VllmChat {
            base: format!("http://{address}/v1"),
            key: None,
            http: reqwest::Client::new(),
        };
        let ferry = json!({"source":"GTP ferry schedules","scheduled_only":true,"date":"09/10/2026","timezone":"Europe/Athens",
            "calls":[{"vessel":"Blue Star Delos","scheduled_arrival_local":"17:00","scheduled_departure_local":"17:15","from_port":"Naxos","to_port":"Piraeus","company":"Blue Star Ferries","full_route":["Naxos","Paros","Piraeus"]}],"valid_until_epoch":Utc::now().timestamp()+300});
        let weather =
            json!({"configured_location":"Parikia","valid_until_epoch":Utc::now().timestamp()+300});
        let mut tools = ReadTools {
            weather_service: None,
            weather: Some(weather.clone()),
            ferry: Some(ferry.clone()),
            observations: None,
        };
        let messages = [ContextMessage {
            source: None,
            id: "target".into(),
            author: "Kari".into(),
            body: format!("@{agent} vis dagslista"),
        }];
        let result = model
            .reply_with_tools(
                agent,
                &[],
                &[],
                "target",
                &messages,
                Some(&weather),
                None,
                Some(&ferry),
                None,
                None,
                false,
                None,
                Some(&mut tools),
            )
            .await;
        let requests = captured.lock().await.clone();
        server.abort();
        (result, requests)
    }

    #[tokio::test]
    async fn native_tools_are_shared_and_preserve_full_port_call_facts() {
        for agent in ["Maria", "Fogd"] {
            let (reply, requests) = model_exchange(agent,json!([{"id":"call-1","type":"function","function":{"name":"ferry_calls","arguments":"{}"}}])).await;
            assert!(reply.is_ok());
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0]["tools"].as_array().unwrap().len(), 2);
            let system = requests[0]["messages"][0]["content"].as_str().unwrap();
            assert!(!system.contains("You have no tools"));
            assert!(!system.contains("data covers only the configured coordinates"));
            assert!(!system.contains("Only up to three upcoming"));
            assert!(system.contains("never silently substitute the default place"));
            assert!(system.contains("full current-day list"));
            assert_eq!(requests[1]["tool_choice"], "none");
            let result = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "tool")
                .unwrap();
            assert_eq!(result["tool_call_id"], "call-1");
            let facts: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
            assert_eq!(facts["calls"][0]["to_port"], "Piraeus");
            assert_eq!(facts["calls"][0]["scheduled_departure_local"], "17:15");
            assert_eq!(facts["calls"][0]["full_route"].as_array().unwrap().len(), 3);
            assert_eq!(facts["scheduled_only"], true);
        }
    }

    #[tokio::test]
    async fn invalid_or_repeated_native_tools_never_reach_the_reply_round() {
        let valid = json!({"id":"call-1","type":"function","function":{"name":"ferry_calls","arguments":"{}"}});
        for calls in [
            json!([{"id":"call-1","type":"function","function":{"name":"vessel_observations","arguments":"{}"}}]),
            json!([{"id":"call-1","type":"function","function":{"name":"ferry_calls","arguments":"{\"url\":\"https://example.com\"}"}}]),
            json!([valid.clone(), valid.clone()]),
        ] {
            let (reply, requests) = model_exchange("Maria", calls).await;
            assert_eq!(reply, Err("model_invalid_tool"));
            assert_eq!(requests.len(), 1);
        }
    }

    /// Explicit operator smoke test; never posts a message or changes agent config.
    #[tokio::test]
    #[ignore = "requires explicitly configured local service tunnels and vLLM credentials"]
    async fn live_model_and_sources_complete_tool_round() {
        let model = VllmChat::from_env()
            .unwrap()
            .expect("SPROYT_VLLM_URL required");
        let ferry_service = ferry::FerryService::from_env()
            .unwrap()
            .expect("SPROYT_FERRY_URL required");
        let weather_service = weather::WeatherService::from_env()
            .unwrap()
            .expect("SPROYT_WEATHER_URL required");
        let ferry = ferry_service.snapshot("paros").await.unwrap();
        let weather = weather_service
            .snapshot_location("Parikia, Paros, Greece")
            .await
            .unwrap();
        for (agent, question, expected) in [
            (
                "Fogd",
                "Give a compact overview of all planned port calls today at Parikia, not only the three next calls. Use ferry_calls.",
                "ferry",
            ),
            (
                "Maria",
                "What is the weather in Bergen, Norway? Use lookup_weather for Bergen, not the default Parikia data.",
                "weather",
            ),
        ] {
            let messages = [ContextMessage {
                source: None,
                id: "target".into(),
                author: "Operator test".into(),
                body: format!("@{agent} {question}"),
            }];
            let mut tools = ReadTools {
                weather_service: Some(&weather_service),
                weather: Some(weather.clone()),
                ferry: Some(ferry.clone()),
                observations: None,
            };
            let reply = model
                .reply_with_tools(
                    agent,
                    &[],
                    &[],
                    "target",
                    &messages,
                    Some(&weather),
                    None,
                    Some(&ferry),
                    None,
                    None,
                    false,
                    None,
                    Some(&mut tools),
                )
                .await
                .unwrap();
            if expected == "ferry" {
                assert!(
                    tools
                        .ferry
                        .as_ref()
                        .unwrap()
                        .get("tool_selection")
                        .is_some(),
                    "model must actually request the full day tool"
                );
            } else {
                let result = tools.weather.as_ref().unwrap();
                assert!(
                    result.get("tool_selection").is_some(),
                    "model must actually request another place"
                );
                assert_eq!(result["resolved_location"]["country"], "Norway");
                assert!(reply.to_lowercase().contains("bergen"));
            }
            println!("{agent} live tool smoke: {reply}");
        }
    }
}
