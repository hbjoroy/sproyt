//! Shared, sourced chat answers. Provider arguments and raw responses are never
//! stored; only the bounded presentation goes through normal idempotent delivery.
use super::place_lookup::PlaceLookupPlan;
use super::*;

const HEADER: &str = "**Barforslag · Google Maps**";
const MAX_PLACES: usize = 8;

// Ground place names in human text without requiring the model to reproduce
// punctuation, casing, Nordic accents or intervening address words verbatim.
fn words(value: &str) -> Vec<String> {
    let folded: String = value
        .nfkd()
        .filter(|ch| !unicode_normalization::char::is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .map(|ch| if ch == 'ø' { 'o' } else { ch })
        .collect();
    let folded = folded.replace('æ', "ae");
    folded
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}
fn bar_word(word: &str) -> bool {
    matches!(
        word,
        "bar"
            | "bars"
            | "barar"
            | "barer"
            | "olbar"
            | "olbarer"
            | "olbarar"
            | "pub"
            | "pubs"
            | "pubar"
            | "puber"
            | "beer"
            | "beers"
            | "ol"
            | "craft"
            | "craftbeer"
            | "brewpub"
            | "brewpubs"
            | "food"
            | "mat"
            | "sport"
            | "sports"
            | "near"
            | "nearby"
            | "around"
            | "in"
            | "at"
            | "by"
            | "i"
            | "ved"
            | "naer"
            | "naerheten"
            | "naerleiken"
            | "av"
            | "med"
            | "og"
            | "and"
            | "good"
            | "gode"
            | "a"
            | "the"
    )
}
fn grounded_text(plan: &PlaceLookupPlan, body: &str) -> bool {
    // Mask macros and mentions before considering any place evidence.
    let mut plain = body.to_owned();
    while let Some(start) = plain.find("[[") {
        let end = plain[start..]
            .find("]]")
            .map_or(plain.len(), |offset| start + offset + 2);
        plain.replace_range(start..end, " ");
    }
    let plain = plain
        .split_whitespace()
        .filter(|word| !word.starts_with('@'))
        .collect::<Vec<_>>()
        .join(" ");
    let source = words(&plain);
    let query = words(&plan.query);
    if plan.origin_mode == "shared" {
        let phrase = format!(" {} ", source.join(" "));
        let explicit = [
            "naer meg",
            "i naerleiken av meg",
            "i naerheten av meg",
            "her eg er",
            "her jeg er",
            "near me",
            "around me",
            "naera mig",
        ]
        .iter()
        .any(|near| phrase.contains(&format!(" {near} ")));
        return explicit
            && !query.is_empty()
            && query.iter().all(|word| {
                bar_word(word)
                    || matches!(word.as_str(), "me" | "meg" | "mig")
                    || source.contains(word)
            });
    }
    if query.is_empty()
        || !query
            .iter()
            .all(|word| bar_word(word) || source.contains(word))
    {
        return false;
    }
    if plan.origin_mode == "named" {
        let Some(origin) = plan.named_origin.as_deref() else {
            return false;
        };
        let origin = words(origin);
        let mut remaining = source.iter();
        !origin.is_empty()
            && origin
                .iter()
                .all(|word| remaining.any(|candidate| candidate == word))
    } else {
        query
            .iter()
            .any(|word| !bar_word(word) && source.contains(word))
    }
}

pub(super) fn grounded_in_context(
    plan: &PlaceLookupPlan,
    target: &str,
    messages: &[ContextMessage],
) -> bool {
    let Some(target) = messages.iter().find(|message| message.id == target) else {
        return false;
    };
    if grounded_text(plan, &target.body) {
        return true;
    }
    // A live position can only come from this request's grant and explicit text.
    if plan.origin_mode == "shared" {
        return false;
    }
    let Some(identity) = &target.source else {
        return false;
    };
    messages.iter().any(|message| {
        message.source.as_ref().is_some_and(|source| {
            source.sender_id == identity.sender_id
                && source.channel_id == identity.channel_id
                && source.sender_kind == Some(crate::domain::PrincipalKind::Human)
                && source.provenance == Some(crate::agent::ActivityProvenance::Human)
                && source.deleted_at.is_none()
                && source.edited_at.is_none()
                && source.sequence < identity.sequence
                && source.created_at <= identity.created_at
                && identity
                    .created_at
                    .signed_duration_since(source.created_at)
                    .num_seconds()
                    <= WINDOW_SECONDS
        }) && grounded_text(plan, &message.body)
    })
}

// Provider text must never become a Sprøyt macro, mention, HTML or Markdown link.
fn text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '[' => '［',
            ']' => '］',
            '<' => '‹',
            '>' => '›',
            '@' => '＠',
            '\\' => '＼',
            '`' => 'ˋ',
            '*' => '＊',
            '_' => '＿',
            ch if ch.is_control() && ch != '\n' => ' ',
            ch => ch,
        })
        .collect()
}

fn candidates(data: &Value) -> std::result::Result<Vec<String>, &'static str> {
    let places = data["places"].as_array().ok_or("maps_invalid_response")?;
    places
        .iter()
        .take(MAX_PLACES)
        .map(|place| {
            let title = place["attribution"]["title"]
                .as_str()
                .ok_or("maps_attribution")?;
            let url = place["googleMapsLinks"]["placeUrl"]
                .as_str()
                .ok_or("maps_attribution")?;
            let source_url = place["attribution"]["url"]
                .as_str()
                .ok_or("maps_attribution")?;
            if title.trim().is_empty()
                || title.len() > 1024
                || url.len() > 4096
                || !maps::google_url(url)
                || !maps::google_url(source_url)
            {
                return Err("maps_attribution");
            }
            // URL angle brackets keep parentheses and Unicode URLs inside the link.
            let url = url.replace('<', "%3C").replace('>', "%3E");
            let source_url = source_url.replace('<', "%3C").replace('>', "%3E");
            let title = text(title);
            Ok(if url == source_url {
                format!("[{title}](<{url}>)")
            } else {
                format!("[{title}](<{url}>) · [Kjelde](<{source_url}>)")
            })
        })
        .collect()
}

fn selection(choices: &[String], draw: u32) -> std::result::Result<String, &'static str> {
    let selected = choices
        .get(draw.checked_sub(1).ok_or("model_invalid_tool")? as usize)
        .ok_or("model_invalid_tool")?;
    Ok(format!(
        "🎲 Terningen gav {draw} av {}: {selected}",
        choices.len()
    ))
}

pub(super) fn render(
    data: &Value,
    choose_random: bool,
) -> std::result::Result<String, &'static str> {
    let choices = candidates(data)?;
    if choices.is_empty() {
        return Ok("Eg fann ingen barforslag i dette søket hos Google Maps. Prøv eit tydelegare stadnamn eller eit større område.".into());
    }
    let mut answer = format!("{HEADER}\n\n");
    if choose_random {
        answer.push_str(&selection(
            &choices,
            tools::roll_die(choices.len() as u32)?,
        )?);
        answer.push_str("\n\n");
    }
    let summary = data["summary"].as_str().ok_or("maps_invalid_response")?;
    // Avoid cutting source links or leaking opaque provider tokens/coordinates.
    answer.push_str(&text(&summary.chars().take(8_000).collect::<String>()));
    answer.push_str("\n\n");
    for (index, choice) in choices.iter().enumerate() {
        answer.push_str(&format!("{}. {choice}\n", index + 1));
    }
    answer.push_str(&format!(
        "\nKjelde: Google Maps · Henta {} UTC. Opningstider og tilbod kan endre seg.",
        Utc::now().format("%d.%m.%Y %H:%M")
    ));
    MessageBody::new(answer.clone()).map_err(|_| "maps_response_too_large")?;
    Ok(answer)
}

/// A followup can select from this agent's actual published list, without a new
/// provider request or an invented model roll. The result is cached by the job.
pub(super) fn choose_from_context(
    job: &Job,
    messages: &[ContextMessage],
) -> std::result::Result<Option<String>, &'static str> {
    let Some(target) = messages
        .iter()
        .find(|message| message.id == job.source_message_id)
    else {
        return Ok(None);
    };
    let body = target.body.to_lowercase();
    if !(body.contains("trill")
        || body.contains("terning")
        || body.contains("random")
        || body.contains("tilfeldig"))
        || !(body.contains("bar")
            || body.contains("velj")
            || body.contains("velg")
            || body.contains("vel ")
            || body.contains("choose")
            || body.contains("pick"))
    {
        return Ok(None);
    }
    let Some(target_source) = &target.source else {
        return Ok(None);
    };
    let Some(previous) = messages.iter().rev().find(|message| {
        message.body.starts_with(HEADER)
            && message.source.as_ref().is_some_and(|source| {
                source.sender_id.to_string() == job.agent_id
                    && source.channel_id == target_source.channel_id
                    && source.sequence < target_source.sequence
                    && source.deleted_at.is_none()
                    && source.edited_at.is_none()
            })
    }) else {
        return Ok(None);
    };
    let choices: Vec<String> = previous
        .body
        .lines()
        .filter_map(|line| {
            let (number, choice) = line.split_once(". ")?;
            if number
                .parse::<usize>()
                .ok()
                .is_none_or(|n| !(1..=MAX_PLACES).contains(&n))
                || !choice.starts_with('[')
                || !choice.contains("](<https://")
            {
                return None;
            }
            Some(choice.to_owned())
        })
        .take(MAX_PLACES)
        .collect();
    if choices.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "{}\n\nValt frå barforslaga ovanfor. Kjelde: Google Maps.",
        selection(&choices, tools::roll_die(choices.len() as u32)?)?
    )))
}

impl CircleChatAgents {
    pub(super) async fn shared_place_search(
        &self,
        job: &Job,
        plan: &PlaceLookupPlan,
    ) -> std::result::Result<String, &'static str> {
        if self
            .source(job)
            .await
            .map_err(|_| "source_lookup")?
            .is_none()
        {
            return Err("location_changed");
        }
        let author = self
            .store
            .values(
                "select cast(sender_id as text) from messages where id=?uuid",
                std::slice::from_ref(&job.source_message_id),
            )
            .await
            .map_err(|_| "source_lookup")?;
        let author =
            UserId::new(author.first().ok_or("source_lookup")?).map_err(|_| "source_lookup")?;
        let token = self
            .claim_place_lookup(&author)
            .await
            .map_err(|_| "maps_busy")?
            .ok_or("maps_busy")?;
        let result = async {
            let service = maps::MapsService::from_env()?.ok_or("maps_configuration")?;
            let descriptor = place_lookup::PlaceLookupDescriptor {
                job_id: job.id.clone(),
                agent_id: job.agent_id.clone(),
                source_message_id: job.source_message_id.clone(),
                source_author_id: author.to_string(),
                share_id: None,
                query: plan.query.clone(),
                origin_mode: plan.origin_mode.clone(),
                named_origin: plan.named_origin.clone(),
            };
            let query = descriptor.provider_query().ok_or("model_invalid_tool")?;
            if self
                .source(job)
                .await
                .map_err(|_| "source_lookup")?
                .is_none()
            {
                return Err("location_changed");
            }
            let source = self
                .source(job)
                .await
                .map_err(|_| "source_lookup")?
                .ok_or("location_changed")?;
            let context = self
                .context(job, &source)
                .await
                .map_err(|_| "source_lookup")?;
            if !grounded_in_context(plan, &job.source_message_id, &context) {
                return Err("location_changed");
            }
            let location = self
                .location_for_job(job)
                .await
                .map_err(|_| "location_changed")?;
            if plan.origin_mode == "shared" && location.is_none() {
                return Err("location_changed");
            }
            let data = service
                .search_places(
                    &query,
                    if plan.origin_mode == "shared" {
                        location.as_ref()
                    } else {
                        None
                    },
                )
                .await?;
            // Revocation or edits during external I/O prevent publication.
            self.location_for_job(job)
                .await
                .map_err(|_| "location_changed")?;
            if self
                .source(job)
                .await
                .map_err(|_| "source_lookup")?
                .is_none()
            {
                return Err("location_changed");
            }
            render(&data, plan.choose_random)
        }
        .await;
        self.release_place_lookup(&author, &token)
            .await
            .map_err(|_| "maps_busy")?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data() -> Value {
        json!({"summary":"To pubar nær stasjonen [0] [1]", "contentToken":"SECRET-RAW-TOKEN", "places":[
            {"id":"secret-place-id", "location":{"latitude":55.1234,"longitude":12.4567},"attribution":{"title":"Pub A","url":"https://www.google.com/maps?cid=1"},"googleMapsLinks":{"placeUrl":"https://www.google.com/maps?cid=1"}},
            {"attribution":{"title":"Pub B","url":"https://www.google.com/maps?cid=2"},"googleMapsLinks":{"placeUrl":"https://www.google.com/maps?cid=2"}}
        ]})
    }
    fn context_message(sender: Uuid, channel: Uuid, sequence: u64, body: &str) -> ContextMessage {
        let id = Uuid::now_v7();
        ContextMessage {id:id.to_string(),author:"Same display name".into(),body:body.into(),source:Some(serde_json::from_value(json!({
            "message_id":id,"circle_id":null,"channel_id":channel,"sender_id":sender,"sender_kind":"human","provenance":"human",
            "parent_message_id":null,"sequence":sequence,"created_at":Utc::now().to_rfc3339(),"edited_at":null,"deleted_at":null
        })).unwrap())}
    }
    #[test]
    fn followup_location_is_bound_to_the_same_human_channel_and_recent_unedited_text() {
        let (sender, channel) = (Uuid::now_v7(), Uuid::now_v7());
        let prior = context_message(sender, channel, 1, "Finn barar i Bergen");
        let target = context_message(sender, channel, 2, "Kan du finne eit forslag?");
        let plan = place_lookup::parse_plan(r#"{"query":"Bergen","origin_mode":"none"}"#).unwrap();
        assert!(grounded_in_context(
            &plan,
            &target.id,
            &[prior.clone(), target.clone()]
        ));
        for change in ["sender", "channel", "edited", "old", "agent"] {
            let mut changed = prior.clone();
            let source = changed.source.as_mut().unwrap();
            match change {
                "sender" => source.sender_id = Uuid::now_v7(),
                "channel" => source.channel_id = Uuid::now_v7(),
                "edited" => source.edited_at = Some(Utc::now()),
                "old" => source.created_at -= chrono::Duration::minutes(21),
                "agent" => source.sender_kind = Some(crate::domain::PrincipalKind::Agent),
                _ => unreachable!(),
            }
            assert!(
                !grounded_in_context(&plan, &target.id, &[changed, target.clone()]),
                "{change}"
            );
        }
        let shared =
            place_lookup::parse_plan(r#"{"query":"barar","origin_mode":"shared"}"#).unwrap();
        let mut near = prior;
        near.body = "Finn barar nær meg".into();
        assert!(!grounded_in_context(
            &shared,
            &target.id,
            &[near, target.clone()]
        ));
    }

    #[test]
    fn hotel_address_accepts_generated_bar_query_and_normalized_place_spelling() {
        let body = "@terningar hotellet vi skal bu på er Elite Hotel Savoy i Norra Vallgatan 62, Malmö - finn du nokre gode ølbarar i nærleiken?";
        for query in [
            "beer bars",
            "ølbarer near Elite Hotel Savoy Malmö",
            "craft beer bars in Malmö",
        ] {
            let plan=place_lookup::parse_plan(&json!({"query":query,"origin_mode":"named","named_origin":"Elite Hotel Savoy, Malmo"}).to_string()).unwrap();
            assert!(grounded_text(&plan, body), "{query}");
        }
        let invented=place_lookup::parse_plan(r#"{"query":"bars","origin_mode":"named","named_origin":"Hotel California Copenhagen"}"#).unwrap();
        assert!(!grounded_text(&invented, body));
        let missing = place_lookup::parse_plan(r#"{"query":"bars","origin_mode":"none"}"#).unwrap();
        assert!(!grounded_text(&missing, "find bars"));
    }

    #[test]
    fn generated_near_me_query_still_requires_explicit_target_location_intent() {
        let shared =
            place_lookup::parse_plan(r#"{"query":"beer bars near me","origin_mode":"shared"}"#)
                .unwrap();
        assert!(grounded_text(&shared, "Finn ølbarar nær meg"));
        assert!(!grounded_text(&shared, "Finn ølbarar i Malmö"));
    }

    #[test]
    fn shared_answer_keeps_sources_but_not_raw_tokens_or_location() {
        let answer = render(&data(), false).unwrap();
        assert!(answer.starts_with(HEADER));
        assert!(answer.contains("1. [Pub A](<https://www.google.com/maps?cid=1>)"));
        assert!(answer.contains("2. [Pub B]"));
        assert!(answer.contains("Kjelde: Google Maps"));
        for secret in [
            "SECRET-RAW-TOKEN",
            "secret-place-id",
            "55.1234",
            "12.4567",
            "text_query",
            "origin_mode",
        ] {
            assert!(!answer.contains(secret));
        }
    }
    #[test]
    fn random_selection_is_one_of_the_actual_results_and_keeps_attribution() {
        let answer = render(&data(), true).unwrap();
        assert!(answer.contains("🎲 Terningen gav"));
        assert!(answer.contains("av 2:"));
        assert!(answer.contains("Kjelde: Google Maps"));
        assert_eq!(
            selection(&["Pub A".into(), "Pub B".into()], 2).unwrap(),
            "🎲 Terningen gav 2 av 2: Pub B"
        );
        assert!(selection(&["Pub A".into()], 2).is_err());
    }
    #[test]
    fn unsafe_provider_content_cannot_become_macros_mentions_or_links() {
        let mut value = data();
        value["summary"] = json!("[[task:42]] @harald <script> [evil](javascript:alert(1))");
        let answer = render(&value, false).unwrap();
        for unsafe_text in ["[[task:", "@harald", "<script>", "[evil]("] {
            assert!(!answer.contains(unsafe_text));
        }
        value["places"][0]["googleMapsLinks"]["placeUrl"] = json!("https://evil.example");
        assert!(render(&value, false).is_err());
    }
}

#[cfg(test)]
#[path = "shared_places_tests.rs"]
mod integration_tests;
