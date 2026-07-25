use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crate::layout::StoreLayout;
use crate::search::{MessageSummary, SearchError, SearchIndex};

/// Recent mail is the best address book: recipients of sent
/// mail score highest, senders of received mail follow, and
/// newer sightings outweigh old ones.
const HARVEST_WINDOW: usize = 5000;
const SENT_RECIPIENT_WEIGHT: u32 = 3;
const SENDER_WEIGHT: u32 = 1;
/// A sighting in the newest fifth of the window counts double.
const RECENT_BONUS: u32 = 1;
const HARVEST_FILE: &str = "harvested.tsv";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    pub address: String,
    pub name: String,
    pub score: u32,
}

/// Scans the newest messages and writes a ranked address list
/// under store/contacts/, replacing the previous harvest.
pub fn harvest(
    layout: &StoreLayout,
    index: &SearchIndex,
) -> Result<Vec<Contact>, SearchError> {
    let messages = index.query("*", Some(HARVEST_WINDOW))?;
    let contacts = rank(&messages);
    let _ = save(layout, &contacts);
    Ok(contacts)
}

pub fn load(layout: &StoreLayout) -> Vec<Contact> {
    let Ok(text) = std::fs::read_to_string(harvest_path(layout)) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_line).collect()
}

fn rank(messages: &[MessageSummary]) -> Vec<Contact> {
    let recent_floor = messages.len() / 5;
    let mut scores: HashMap<String, Contact> = HashMap::new();
    for (position, message) in messages.iter().enumerate() {
        let recent = position < recent_floor;
        for (field, weight) in [
            (&message.to, SENT_RECIPIENT_WEIGHT),
            (&message.from, SENDER_WEIGHT),
        ] {
            for (address, name) in address_entries(field) {
                let points =
                    weight + if recent { RECENT_BONUS } else { 0 };
                let entry = scores
                    .entry(address.to_lowercase())
                    .or_insert(Contact {
                        address,
                        name: name.clone(),
                        score: 0,
                    });
                entry.score += points;
                if entry.name.is_empty() && !name.is_empty() {
                    entry.name = name;
                }
            }
        }
    }
    let mut contacts: Vec<Contact> = scores.into_values().collect();
    contacts.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.address.cmp(&right.address))
    });
    contacts
}

/// Splits an address header into (address, display name)
/// pairs; tolerant of bare addresses and comma-joined lists,
/// with commas inside quoted names or angle brackets kept.
/// Public because reply-all needs the same tolerant split.
pub fn address_entries(field: &str) -> Vec<(String, String)> {
    split_entries(field)
        .into_iter()
        .filter_map(|entry| {
            let entry = entry.trim();
            if let (Some(open), Some(close)) =
                (entry.find('<'), entry.rfind('>'))
                && open < close
            {
                let address = entry[open + 1..close].trim().to_owned();
                let name =
                    entry[..open].trim().trim_matches('"').to_owned();
                return valid(address).map(|a| (a, name));
            }
            valid(entry.to_owned()).map(|a| (a, String::new()))
        })
        .collect()
}

fn split_entries(field: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut angled = false;
    for ch in field.chars() {
        match ch {
            '"' => quoted = !quoted,
            '<' if !quoted => angled = true,
            '>' if !quoted => angled = false,
            ',' if !quoted && !angled => {
                entries.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    entries.push(current);
    entries
}

fn valid(address: String) -> Option<String> {
    let well_formed = address.contains('@')
        && !address.contains(char::is_whitespace)
        && address.len() > 2;
    well_formed.then_some(address)
}

fn save(layout: &StoreLayout, contacts: &[Contact]) -> io::Result<()> {
    let path = harvest_path(layout);
    std::fs::create_dir_all(layout.contacts_dir())?;
    let mut text = String::new();
    for contact in contacts {
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            contact.score, contact.address, contact.name
        ));
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

fn parse_line(line: &str) -> Option<Contact> {
    let mut parts = line.splitn(3, '\t');
    let score = parts.next()?.parse().ok()?;
    let address = parts.next()?.to_owned();
    let name = parts.next().unwrap_or_default().to_owned();
    Some(Contact {
        address,
        name,
        score,
    })
}

fn harvest_path(layout: &StoreLayout) -> PathBuf {
    layout.contacts_dir().join(HARVEST_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(from: &str, to: &str) -> MessageSummary {
        MessageSummary {
            id: String::new(),
            thread_id: String::new(),
            subject: String::new(),
            from: from.to_owned(),
            to: to.to_owned(),
            date_unix: 0,
            tags: Vec::new(),
            unread: false,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn recipients_of_own_mail_outrank_senders() {
        let messages = vec![
            summary("noise@example.com", "friend@example.com"),
            summary("friend@example.com", ""),
        ];
        let ranked = rank(&messages);
        assert_eq!(ranked[0].address, "friend@example.com");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn header_shapes_parse_and_bad_entries_drop() {
        let cases: [(&str, Vec<(&str, &str)>); 4] = [
            (
                "Alba Voss <alba@example.com>",
                vec![("alba@example.com", "Alba Voss")],
            ),
            ("bare@example.com", vec![("bare@example.com", "")]),
            (
                "\"Q, Jay\" <j@example.com>, two@example.com",
                vec![
                    ("j@example.com", "Q, Jay"),
                    ("two@example.com", ""),
                ],
            ),
            ("undisclosed-recipients:;", vec![]),
        ];
        for (field, expected) in cases {
            let got: Vec<(String, String)> = address_entries(field);
            let want: Vec<(String, String)> = expected
                .iter()
                .map(|(a, n)| ((*a).to_owned(), (*n).to_owned()))
                .collect();
            assert_eq!(got, want, "{field}");
        }
    }

    #[test]
    fn a_harvest_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let layout = StoreLayout::new(dir.path().join("store"));
        layout.init().unwrap();
        let contacts = vec![Contact {
            address: "alba@example.com".to_owned(),
            name: "Alba Voss".to_owned(),
            score: 7,
        }];
        save(&layout, &contacts).unwrap();
        assert_eq!(load(&layout), contacts);
        assert_eq!(load(&StoreLayout::new("/nowhere")), Vec::new());
    }
}
