//! Direct addresses use circle-scoped display names, not global human handles.
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .nfc()
        .collect()
}

#[derive(Default)]
pub(super) struct Addresses {
    pub ids: HashSet<String>,
    pub found: bool,
}

pub(super) fn targets(body: &str, names: &[(String, String)]) -> Addresses {
    let body = normalized(body);
    let names = names
        .iter()
        .map(|(id, name)| (id, normalized(name)))
        .collect::<Vec<_>>();
    let mut addresses = Addresses::default();
    let mut consumed = 0;
    for (start, _) in body.match_indices('@') {
        if start < consumed
            || body[..start].chars().next_back().is_some_and(|ch| {
                !ch.is_whitespace() && !matches!(ch, '(' | '[' | '{' | '"' | '\'')
            })
        {
            continue;
        }
        let tail = &body[start + 1..];
        let matches = names
            .iter()
            .filter(|(_, name)| {
                !name.is_empty() && tail.starts_with(name) && {
                    let mut after = tail[name.len()..].chars();
                    let next = after.next();
                    !(next.is_some_and(|ch| {
                        ch.is_alphanumeric()
                            || unicode_normalization::char::is_combining_mark(ch)
                            || matches!(ch, '_' | '-' | '@')
                    }) || (next == Some('.') && after.next().is_some_and(char::is_alphanumeric)))
                }
            })
            .collect::<Vec<_>>();
        let Some(longest) = matches.iter().map(|(_, name)| name.len()).max() else {
            continue;
        };
        let longest = matches
            .into_iter()
            .filter(|(_, name)| name.len() == longest)
            .collect::<Vec<_>>();
        addresses.found = true;
        consumed = start + 1 + longest[0].1.len();
        if longest.len() == 1 {
            addresses.ids.insert(longest[0].0.clone());
        }
    }
    addresses
}

pub(super) fn direct(body: &str, name: &str) -> bool {
    targets(body, &[(String::new(), name.to_owned())]).found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_exact_unicode_names_with_punctuation_and_spaces() {
        let names = vec![
            ("short".into(), "Vêr".into()),
            ("full".into(), "Vêr Venn".into()),
        ];
        assert_eq!(
            targets("Hei (@VÊR   VENN!), korleis går det?", &names).ids,
            HashSet::from(["full".into()])
        );
        assert!(direct("@Ve\u{302}r!", "Vêr"));
        for body in [
            "Vêr",
            "@Vêrvennen",
            "@Vêr_extra",
            "@Vêr-extra",
            "a@Vêr",
            "@Vêr.example",
            "https://example/@Vêr",
        ] {
            assert!(!direct(body, "Vêr"), "{body}");
        }
        assert!(direct("@Vêr.", "Vêr"));
        assert!(!direct("@Bot\u{301}", "Bot"));
    }

    #[test]
    fn ambiguous_names_do_not_address_either_agent_or_hide_other_mentions() {
        let names = vec![
            ("one".into(), "Vêr".into()),
            ("two".into(), "VÊR".into()),
            ("other".into(), "Hjelpar".into()),
        ];
        let result = targets("@Vêr og @Hjelpar!", &names);
        assert!(result.found);
        assert_eq!(result.ids, HashSet::from(["other".into()]));
    }
}
