//! One bounded round of model-selected, read-only tools. Capability snapshots
//! come from the job's authorized configuration, never the agent's name.
use super::*;
#[path = "tool_wire.rs"]
mod wire;
pub(super) use wire::{contains_wire_markup, decode_calls};

pub(super) struct ReadTools<'a> {
    pub place_lookup: Option<super::place_lookup::PlaceLookupPlan>,
    pub weather_service: Option<&'a weather::WeatherService>,
    pub sea_temperature_service: Option<&'a sea_temperature::SeaTemperatureService>,
    pub sea_temperature_config: Option<sea_temperature::SeaTemperatureConfig>,
    pub sea_temperature: Option<Value>,
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

const MAX_DIE_SIDES: u32 = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DieArgs {
    sides: u32,
}

fn die_arguments(arguments: &str) -> std::result::Result<DieArgs, &'static str> {
    let args: DieArgs = serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
    if !(1..=MAX_DIE_SIDES).contains(&args.sides) {
        return Err("model_invalid_tool");
    }
    Ok(args)
}

// Rejection sampling avoids modulo bias. The injected source makes boundary
// cases testable without relying on statistical tests of operating-system RNG.
fn unbiased_die(
    mut next: impl FnMut() -> std::result::Result<u64, &'static str>,
    sides: u32,
) -> std::result::Result<u32, &'static str> {
    if !(1..=MAX_DIE_SIDES).contains(&sides) {
        return Err("model_invalid_tool");
    }
    let n = u64::from(sides);
    let threshold = n.wrapping_neg() % n;
    for _ in 0..128 {
        let value = next()?;
        if value >= threshold {
            return Ok((value % n) as u32 + 1);
        }
    }
    Err("random_unavailable")
}

pub(super) fn roll_die(sides: u32) -> std::result::Result<u32, &'static str> {
    unbiased_die(
        || {
            let mut bytes = [0; 8];
            getrandom::fill(&mut bytes).map_err(|_| "random_unavailable")?;
            Ok(u64::from_le_bytes(bytes))
        },
        sides,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeaTemperatureArgs {
    #[serde(default)]
    locations: Option<Vec<String>>,
}

fn sea_arguments(arguments: &str) -> std::result::Result<SeaTemperatureArgs, &'static str> {
    let args: SeaTemperatureArgs =
        serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
    if let Some(ids) = &args.locations
        && (ids.is_empty()
            || ids.len() > 2
            || ids.iter().any(|id| {
                id.is_empty()
                    || id.len() > 40
                    || !id.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            })
            || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len())
    {
        return Err("model_invalid_tool");
    }
    Ok(args)
}

pub(super) fn validate_arguments(
    name: &str,
    arguments: &str,
) -> std::result::Result<(), &'static str> {
    if arguments.len() > 1024 {
        return Err("model_invalid_tool");
    }
    match name {
        "roll_die" => die_arguments(arguments).map(|_| ()),
        "find_bars" => super::place_lookup::parse_plan(arguments).map(|_| ()),
        "lookup_sea_temperature" => sea_arguments(arguments).map(|_| ()),
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
    pub fn validate_call(
        &self,
        name: &str,
        arguments: &str,
    ) -> std::result::Result<(), &'static str> {
        validate_arguments(name, arguments)?;
        if name == "lookup_sea_temperature" {
            let config = self
                .sea_temperature_config
                .as_ref()
                .ok_or("model_invalid_tool")?;
            let args = sea_arguments(arguments)?;
            if args
                .locations
                .as_ref()
                .is_some_and(|ids| ids.iter().any(|id| !config.locations.contains(id)))
            {
                return Err("model_invalid_tool");
            }
        }
        Ok(())
    }
    pub fn definitions(&self) -> Vec<Value> {
        let mut tools = Vec::new();
        tools.push(json!({"type":"function","function":{
            "name":"roll_die","description":"Roll one real random die with the requested number of sides. Use the returned value; never invent a roll. For a random choice among N known options, request N sides and use the resulting 1-based index.",
            "parameters":{"type":"object","properties":{"sides":{"type":"integer","minimum":1,"maximum":MAX_DIE_SIDES}},"required":["sides"],"additionalProperties":false}}}));
        if super::place_lookup::enabled() {
            tools.push(super::place_lookup::definition());
        }
        if let Some(config) = &self.sea_temperature_config {
            let catalogue = sea_temperature::locations()
                .into_iter()
                .filter(|item| {
                    item["id"]
                        .as_str()
                        .is_some_and(|id| config.locations.iter().any(|allowed| allowed == id))
                })
                .collect::<Vec<_>>();
            tools.push(json!({"type":"function","function":{
                "name":"lookup_sea_temperature","description":format!("Regional modelled sea temperature, never a beach measurement. Use for current sea/bathing temperature questions. Default place: {}. Allowed places: {}. Lookup at most two places in one call. The server renders the factual sea result; no free rewrite is needed. No GPS or arbitrary coordinates. If another place is requested, explain that it is unavailable instead of substituting a default.",config.default_location,json!(catalogue)),
                "parameters":{"type":"object","properties":{"locations":{"type":"array","minItems":1,"maxItems":2,"uniqueItems":true,"items":{"type":"string","enum":config.locations}}},"additionalProperties":false}}}));
        }
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
            "roll_die" => {
                let args = die_arguments(arguments)?;
                json!({"sides": args.sides, "value": roll_die(args.sides)?})
            }
            "find_bars" if super::place_lookup::enabled() => {
                self.place_lookup = Some(super::place_lookup::parse_plan(arguments)?);
                return Ok(json!({"status":"lookup_planned"}));
            }
            "lookup_sea_temperature" if self.sea_temperature_config.is_some() => {
                let args = sea_arguments(arguments)?;
                let config = self
                    .sea_temperature_config
                    .as_ref()
                    .ok_or("model_invalid_tool")?;
                if args
                    .locations
                    .as_ref()
                    .is_some_and(|ids| ids.iter().any(|id| !config.locations.contains(id)))
                {
                    return Err("model_invalid_tool");
                }
                let data = match self.sea_temperature_service {
                    Some(service) => service.lookup(config, args.locations.as_deref()).await,
                    None => return Err("sea_temperature_disabled"),
                };
                self.sea_temperature = Some(data.clone());
                data
            }
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

    #[test]
    fn die_rejects_bias_boundaries_and_rng_failure() {
        // For six sides the first four u64 values are rejected: 2^64 % 6 = 4.
        let mut values = [0, 1, 2, 3, 4].into_iter();
        assert_eq!(unbiased_die(|| Ok(values.next().unwrap()), 6).unwrap(), 5);
        assert_eq!(unbiased_die(|| Ok(u64::MAX), 1).unwrap(), 1);
        assert_eq!(unbiased_die(|| Ok(u64::MAX), 0), Err("model_invalid_tool"));
        assert_eq!(
            unbiased_die(|| Ok(0), MAX_DIE_SIDES + 1),
            Err("model_invalid_tool")
        );
        assert_eq!(
            unbiased_die(|| Err("random_unavailable"), 6),
            Err("random_unavailable")
        );
        assert_eq!(unbiased_die(|| Ok(0), 6), Err("random_unavailable"));
    }

    #[tokio::test]
    async fn real_die_tool_is_bounded_strict_and_available_to_all_agents() {
        let mut tools = ReadTools {
            place_lookup: None,
            weather_service: None,
            sea_temperature_service: None,
            sea_temperature_config: None,
            sea_temperature: None,
            weather: None,
            ferry: None,
            observations: None,
        };
        assert!(
            tools
                .definitions()
                .iter()
                .any(|item| item["function"]["name"] == "roll_die")
        );
        for invalid in [
            "{}",
            r#"{"sides":0}"#,
            r#"{"sides":10001}"#,
            r#"{"sides":-1}"#,
            r#"{"sides":1.5}"#,
            r#"{"sides":6,"extra":1}"#,
        ] {
            assert!(tools.validate_call("roll_die", invalid).is_err());
            assert!(tools.execute("roll_die", invalid).await.is_err());
        }
        for sides in [1, 2, 6, MAX_DIE_SIDES] {
            let result = tools
                .execute("roll_die", &json!({"sides":sides}).to_string())
                .await
                .unwrap();
            assert_eq!(result["sides"], sides);
            assert!((1..=u64::from(sides)).contains(&result["value"].as_u64().unwrap()));
        }
    }

    #[tokio::test]
    async fn marine_capability_is_independent_and_authorized_before_io() {
        let mut tools = ReadTools {
            place_lookup: None,
            weather_service: None,
            sea_temperature_service: None,
            sea_temperature_config: Some(sea_temperature::SeaTemperatureConfig {
                default_location: "parikia".into(),
                locations: vec!["parikia".into(), "bjoroy".into()],
            }),
            sea_temperature: None,
            weather: None,
            ferry: None,
            observations: None,
        };
        let definitions = tools.definitions();
        assert!(
            definitions
                .iter()
                .any(|item| item["function"]["name"] == "lookup_sea_temperature")
        );
        assert!(
            definitions
                .iter()
                .any(|item| item["function"]["name"] == "roll_die")
        );
        assert!(tools.validate_call("lookup_sea_temperature", "{}").is_ok());
        assert!(
            tools
                .validate_call(
                    "lookup_sea_temperature",
                    r#"{"locations":["parikia","bjoroy"]}"#
                )
                .is_ok()
        );
        for invalid in [
            r#"{"locations":["aliki"]}"#,
            r#"{"locations":[]}"#,
            r#"{"locations":["parikia","parikia"]}"#,
            r#"{"locations":["parikia","bjoroy","nordnes"]}"#,
            r#"{"latitude":37.0}"#,
        ] {
            assert!(
                tools
                    .validate_call("lookup_sea_temperature", invalid)
                    .is_err()
            );
            assert!(
                tools
                    .execute("lookup_sea_temperature", invalid)
                    .await
                    .is_err()
            );
            assert!(tools.sea_temperature.is_none());
        }
        assert_eq!(
            tools
                .execute("lookup_sea_temperature", "{}")
                .await
                .unwrap_err(),
            "sea_temperature_disabled"
        );
        tools.sea_temperature_config = None;
        assert!(
            !tools
                .definitions()
                .iter()
                .any(|item| item["function"]["name"] == "lookup_sea_temperature")
        );
        assert!(tools.validate_call("lookup_sea_temperature", "{}").is_err());
    }

    #[tokio::test]
    async fn capabilities_and_strict_arguments_bound_tool_execution() {
        let mut tools = ReadTools {
            place_lookup: None,
            weather_service: None,
            sea_temperature_service: None,
            sea_temperature_config: None,
            sea_temperature: None,
            weather: None,
            ferry: None,
            observations: None,
        };
        assert!(
            tools
                .definitions()
                .iter()
                .any(|item| item["function"]["name"] == "roll_die")
        );
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
            place_lookup: None,
            weather_service: None,
            sea_temperature_service: None,
            sea_temperature_config: None,
            sea_temperature: None,
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
        model_exchange_body(agent, calls, "vis dagslista").await
    }
    async fn model_exchange_body(
        agent: &str,
        calls: Value,
        body: &str,
    ) -> (std::result::Result<String, &'static str>, Vec<Value>) {
        model_exchange_draft(
            agent,
            json!({"role":"assistant","content":null,"tool_calls":calls}),
            body,
        )
        .await
    }
    async fn model_exchange_draft(
        agent: &str,
        draft: Value,
        body: &str,
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
                let draft = draft.clone();
                async move {
                    let mut requests = record.lock().await;
                    requests.push(request);
                    if requests.len() == 1 {
                        Json(json!({"choices":[{"message":draft}]}))
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
            place_lookup: None,
            weather_service: None,
            sea_temperature_service: None,
            sea_temperature_config: None,
            sea_temperature: None,
            weather: Some(weather.clone()),
            ferry: Some(ferry.clone()),
            observations: None,
        };
        let messages = [ContextMessage {
            source: None,
            id: "target".into(),
            author: "Kari".into(),
            body: format!("@{agent} {body}"),
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
    async fn qwen_content_tools_are_executed_and_fragments_are_never_published() {
        let content = "<tool_call>\n<function=roll_die>\n<parameter=sides>\n6\n</parameter>\n</function>\n</tool_call>";
        let (reply, requests) =
            model_exchange_draft("terningar", json!({"content":content}), "trill ein terning")
                .await;
        assert!(reply.is_ok());
        assert_eq!(requests.len(), 2);
        let result = requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "tool")
            .unwrap();
        let result: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(result["sides"], 6);
        assert!((1..=6).contains(&result["value"].as_u64().unwrap()));
        let leaked = "Ja, no! </parameter>\n</function>\n</tool_call>";
        let (reply, requests) =
            model_exchange_draft("terningar", json!({"content":leaked}), "Søker du?").await;
        assert_eq!(reply, Err("model_invalid_tool"));
        assert_eq!(requests.len(), 1);
        assert_eq!(
            crate::chatbot::validate_model_answer(leaked, false),
            Err("model_invalid_reply")
        );
    }

    #[tokio::test]
    async fn agent_receives_real_die_result_in_the_tool_round() {
        let calls = json!([{"id":"die-1","type":"function","function":{"name":"roll_die","arguments":r#"{"sides":6}"#}}]);
        let (reply, requests) = model_exchange_body("Maria", calls, "Trill ein terning").await;
        assert!(reply.is_ok());
        assert_eq!(requests.len(), 2);
        let result = requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "tool" && message["tool_call_id"] == "die-1")
            .unwrap();
        let result: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(result["sides"], 6);
        assert!((1..=6).contains(&result["value"].as_u64().unwrap()));
    }

    #[tokio::test]
    async fn place_lookup_model_selection_never_gets_a_result_or_second_round() {
        super::super::place_lookup::TEST_ENABLED.scope(true, async {
            let call=json!({"id":"maps-1","type":"function","function":{"name":"find_bars","arguments":r#"{"query":"bars in Bergen","origin_mode":"none"}"#}});
            let (reply,requests)=model_exchange_body("Maria",json!([call.clone()]), "bars in Bergen").await;
            assert_eq!(reply.unwrap(),super::super::place_lookup::NEUTRAL_REPLY);
            assert_eq!(requests.len(),1);
            let (reply,requests)=model_exchange("Maria",json!([call.clone()])).await;
            assert_eq!(reply.unwrap(),super::super::place_lookup::CLARIFICATION_REPLY);
            assert_eq!(requests.len(),1);
            let extra=json!({"id":"ferry-1","type":"function","function":{"name":"ferry_calls","arguments":"{}"}});
            let (reply,requests)=model_exchange("Maria",json!([call,extra])).await;
            assert_eq!(reply,Err("model_invalid_tool"));
            assert_eq!(requests.len(),1);
            let (reply,requests)=model_exchange("Maria",json!([{"id":"maps-1","type":"function","function":{"name":"find_bars","arguments":r#"{"query":"bars","origin_mode":"shared"}"#}}])).await;
            assert_eq!(reply,Err("model_invalid_tool"));
            assert_eq!(requests.len(),1);
        }).await;
    }

    #[tokio::test]
    async fn native_tools_are_shared_and_preserve_full_port_call_facts() {
        for agent in ["Maria", "Fogd"] {
            let (reply, requests) = model_exchange(agent,json!([{"id":"call-1","type":"function","function":{"name":"ferry_calls","arguments":"{}"}}])).await;
            assert!(reply.is_ok());
            assert_eq!(requests.len(), 2);
            assert!(
                requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == "roll_die")
            );
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
                place_lookup: None,
                weather_service: Some(&weather_service),
                sea_temperature_service: None,
                sea_temperature_config: None,
                sea_temperature: None,
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
