use antiphon_store::contacts::Contact;

pub(super) const MAX_SUGGESTIONS: usize = 5;
const MIN_FRAGMENT_CHARS: usize = 2;

/// Suggestions under a recipient field: harvested contacts
/// matching the entry being typed, best-ranked first.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Completion {
    pub items: Vec<String>,
    pub selected: usize,
}

impl Completion {
    pub fn step(&mut self, step: i32) {
        let count = self.items.len() as i32;
        if count == 0 {
            return;
        }
        let next = (self.selected as i32 + step).rem_euclid(count);
        self.selected = next as usize;
    }

    pub fn chosen(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
    }
}

/// Matches the fragment after the last comma against address
/// and display name; contacts arrive ranked, so order holds.
pub(super) fn suggest(
    contacts: &[Contact],
    field: &str,
) -> Vec<String> {
    let fragment = fragment(field).to_lowercase();
    if fragment.chars().count() < MIN_FRAGMENT_CHARS {
        return Vec::new();
    }
    contacts
        .iter()
        .filter(|contact| {
            contact.address.to_lowercase().contains(&fragment)
                || contact.name.to_lowercase().contains(&fragment)
        })
        .take(MAX_SUGGESTIONS)
        .map(entry)
        .collect()
}

/// Replaces the fragment being typed with the chosen entry,
/// keeping every completed entry before it.
pub(super) fn accept(field: &str, choice: &str) -> String {
    match field.rfind(',') {
        Some(comma) => {
            format!("{}, {choice}", field[..comma].trim_end())
        }
        None => choice.to_string(),
    }
}

fn fragment(field: &str) -> &str {
    field.rsplit(',').next().unwrap_or(field).trim()
}

fn entry(contact: &Contact) -> String {
    if contact.name.is_empty() {
        return contact.address.clone();
    }
    format!("{} <{}>", contact.name, contact.address)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(address: &str, name: &str, score: u32) -> Contact {
        Contact {
            address: address.to_string(),
            name: name.to_string(),
            score,
        }
    }

    fn book() -> Vec<Contact> {
        vec![
            contact("alba@example.com", "Alba Voss", 9),
            contact("mara@example.org", "", 5),
            contact("albert@example.net", "Albert Ng", 2),
        ]
    }

    #[test]
    fn fragments_match_address_and_name_in_rank_order() {
        let hits = suggest(&book(), "alb");
        assert_eq!(
            hits,
            [
                "Alba Voss <alba@example.com>",
                "Albert Ng <albert@example.net>"
            ]
        );
        assert_eq!(suggest(&book(), "voss").len(), 1);
        assert_eq!(suggest(&book(), "a"), Vec::<String>::new());
        assert_eq!(suggest(&book(), ""), Vec::<String>::new());
    }

    #[test]
    fn the_fragment_is_the_entry_after_the_last_comma() {
        let hits = suggest(&book(), "alba@example.com, mar");
        assert_eq!(hits, ["mara@example.org"]);
    }

    #[test]
    fn accepting_replaces_only_the_fragment() {
        assert_eq!(
            accept("alb", "Alba Voss <alba@example.com>"),
            "Alba Voss <alba@example.com>"
        );
        assert_eq!(
            accept("one@example.com, mar", "mara@example.org"),
            "one@example.com, mara@example.org"
        );
    }

    #[test]
    fn selection_steps_wrap_both_ways() {
        let mut completion = Completion {
            items: vec!["a".into(), "b".into(), "c".into()],
            selected: 0,
        };
        completion.step(-1);
        assert_eq!(completion.chosen(), Some("c"));
        completion.step(1);
        assert_eq!(completion.chosen(), Some("a"));
        Completion::default().step(1);
    }
}
