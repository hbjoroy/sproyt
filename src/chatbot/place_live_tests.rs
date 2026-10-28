use super::*;

#[tokio::test]
#[ignore = "requires an explicitly configured real vLLM endpoint; no Google requests or chat writes"]
async fn live_qwen_hotel_search_and_real_die_use_executable_tools() {
    let model = VllmChat {
        base: std::env::var("SPROYT_TEST_VLLM_URL").expect("explicit test endpoint"),
        key: std::env::var("SPROYT_TEST_VLLM_API_KEY").ok(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(55))
            .build()
            .unwrap(),
    };
    place_lookup::TEST_ENABLED.scope(true,async {
        for (kind,body) in [
            ("hotel","@terningar hotellet vi skal bu på er Elite Hotel Savoy i Norra Vallgatan 62, Malmö - finn du nokre gode ølbarar i nærleiken?"),
            ("die","@terningar trill ein terning"),
            ("pair","Trill")
        ] {
            let mut runtime=tools::ReadTools {place_lookup:None, dice: Vec::new(),weather_service:None,sea_temperature_service:None,sea_temperature_config:None,sea_temperature:None,weather:None,ferry:None,observations:None};
            let messages=[ContextMessage {id:"target".into(),author:"Operator test".into(),body:body.into(),source:None}];
            let guidance=if kind=="pair" {vec!["ved trill, trille eller trilla: bruk sekundet i klokkeslettet som seed for å velgje to tal mellom 1 og 6".into()]} else {vec!["Kort og lettbeint på nynorsk".into()]};
            let answer=model.reply_with_tools("terningar",&["trill".into()],&guidance,"target",&messages,None,None,None,None,None,false,None,Some(&mut runtime)).await.unwrap();
            assert!(!tools::contains_wire_markup(&answer));
            if kind=="hotel" {
                assert!(runtime.place_lookup.is_some(),"hotel request must actually select find_bars; answer: {answer}");
                let plan=runtime.place_lookup.unwrap();
                assert!(shared_places::grounded_in_context(&plan,"target",&messages));
                println!("live hotel: executable grounded search plan selected");
            } else {if kind=="pair" {assert_eq!(runtime.dice.len(),2,"the actual configured two-dice prompt must produce two real rolls");} println!("live {kind}: {answer}");}
        }
        let human=Uuid::now_v7();let channel=Uuid::now_v7();
        let make=|sequence:u64,body:&str| {
            let id=Uuid::now_v7();ContextMessage {id:id.to_string(),author:"Operator test".into(),body:body.into(),source:Some(serde_json::from_value(json!({
                "message_id":id,"circle_id":null,"channel_id":channel,"sender_id":human,"sender_kind":"human","provenance":"human",
                "parent_message_id":null,"sequence":sequence,"created_at":Utc::now().to_rfc3339(),"edited_at":null,"deleted_at":null
            })).unwrap())}
        };
        let prior=make(1,"Elite Hotel Savoy i Norra Vallgatan 62, Malmö. Finn gode ølbarar i nærleiken.");
        let anchor=ContextMessage {id:Uuid::now_v7().to_string(),author:"terningar".into(),body:"No har eg utgangspunktet!".into(),source:None};
        let target=make(3,"Søker du?");
        let followup=FollowupContext {mode:"implicit".into(),anchor:anchor.clone()};
        let messages=[prior,anchor,target.clone()];
        let mut runtime=tools::ReadTools {place_lookup:None, dice: Vec::new(),weather_service:None,sea_temperature_service:None,sea_temperature_config:None,sea_temperature:None,weather:None,ferry:None,observations:None};
        let answer=model.reply_with_tools("terningar",&["trill".into()],&["Kort og lettbeint på nynorsk".into()],&target.id,&messages,None,Some(&followup),None,None,None,false,None,Some(&mut runtime)).await.unwrap();
        assert!(runtime.place_lookup.is_some(),"followup must perform the requested search rather than promise it; answer: {answer}");
        println!("live followup: earlier hotel address leads to executable search");
    }).await;
}
