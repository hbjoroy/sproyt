//! Reviewed operator facts, independent of any conversational persona.
//! Specifications are never evidence that a vessel is at a location.
use serde_json::{Value, json};

pub(super) fn catalogue() -> Vec<Value> {
    [
        (
            "blue-star-delos",
            "BLUE STAR DELOS",
            "Blue Star Ferries",
            145.9,
            2400,
            26.0,
            "https://www.bluestarferries.com/en-gb/ferries/blue-star-delos",
        ),
        (
            "blue-star-naxos",
            "BLUE STAR NAXOS",
            "Blue Star Ferries",
            124.2,
            1474,
            25.0,
            "https://www.bluestarferries.com/en-gb/ferries/blue-star-naxos",
        ),
        (
            "artemis",
            "ARTEMIS",
            "Hellenic Seaways",
            89.8,
            748,
            19.2,
            "https://www.hellenicseaways.gr/en-gb/ferries/artemis",
        ),
        (
            "champion-jet-3",
            "CHAMPION JET 3",
            "Seajets",
            87.0,
            1100,
            40.0,
            "https://www.seajets.com/learn-about-seajets/fleet",
        ),
        (
            "superjet-2",
            "SUPERJET 2",
            "Seajets",
            42.0,
            386,
            38.0,
            "https://www.seajets.com/learn-about-seajets/fleet",
        ),
        (
            "worldchampion-jet",
            "WORLDCHAMPION JET",
            "Seajets",
            87.0,
            1280,
            50.0,
            "https://www.seajets.com/learn-about-seajets/fleet",
        ),
    ]
    .into_iter()
    .map(|(id, name, operator, length, passengers, speed, source)| {
        json!({
            "catalogue_id":id, "name":name, "operator":operator,
            "length_metres":length, "passenger_capacity":passengers,
            "published_speed_knots":speed, "source_url":source,
            "verified_on":"2026-10-04", "kind":"operator_specification_not_live_activity"
        })
    })
    .collect()
}

pub(super) fn normalized_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

pub(super) fn identify(name: &str) -> Option<Value> {
    let name = normalized_name(name);
    // Only complete names/spacing aliases, never a port or a brand substring.
    catalogue()
        .into_iter()
        .find(|item| normalized_name(item["name"].as_str().unwrap_or_default()) == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_matches_spacing_aliases_without_brand_or_partial_identity() {
        assert_eq!(
            identify("Super Jet 2").unwrap()["catalogue_id"],
            "superjet-2"
        );
        assert_eq!(identify("BLUE STAR DELOS").unwrap()["length_metres"], 145.9);
        assert!(identify("Delos").is_none());
        assert!(identify("Blue Star").is_none());
        assert!(identify("Artemis II").is_none());
        assert_eq!(identify("Artemis").unwrap()["operator"], "Hellenic Seaways");
        assert_eq!(catalogue().len(), 6);
    }
}
