use icalendar::{
    Calendar, CalendarComponent, CalendarDateTime, Component,
    DatePerhapsTime, Event, EventLike,
};
use mail_parser::{MessageParser, MessagePart, MimeHeaders, PartType};

const METHOD_REQUEST: &str = "REQUEST";
const MAILTO_SCHEME: &str = "mailto:";
const NO_TITLE: &str = "(no title)";
const REPLY_HINT: &str = "accept/decline not yet wired";
const DATE_FORMAT: &str = "%d %b %Y";
const DATE_TIME_FORMAT: &str = "%d %b %Y %H:%M";
const LABEL_WIDTH: usize = 10;

/// A readable block for the first text/calendar part of a
/// message, empty when there is none. Render-only: the part's
/// bytes are never touched, so a forward carries the original
/// invite unchanged.
pub fn invite_lines(raw: &[u8]) -> Vec<String> {
    let Some(text) = calendar_text(raw) else {
        return Vec::new();
    };
    let Ok(calendar) = text.parse::<Calendar>() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for component in &calendar.components {
        let CalendarComponent::Event(event) = component else {
            continue;
        };
        push_event(&mut lines, event);
    }
    if lines.is_empty() {
        return lines;
    }
    let request =
        calendar.property_value("METHOD") == Some(METHOD_REQUEST);
    if request {
        push_field(&mut lines, "reply:", Some(REPLY_HINT.to_string()));
    }
    lines
}

fn calendar_text(raw: &[u8]) -> Option<String> {
    let message = MessageParser::default().parse(raw)?;
    message.parts.iter().find_map(|part| {
        if !is_calendar(part) {
            return None;
        }
        part_text(part)
    })
}

fn is_calendar(part: &MessagePart<'_>) -> bool {
    part.content_type().is_some_and(|content_type| {
        content_type.ctype().eq_ignore_ascii_case("text")
            && content_type.subtype().is_some_and(|subtype| {
                subtype.eq_ignore_ascii_case("calendar")
            })
    })
}

fn part_text(part: &MessagePart<'_>) -> Option<String> {
    match &part.body {
        PartType::Text(text) => Some(text.to_string()),
        PartType::Binary(bytes) | PartType::InlineBinary(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => None,
    }
}

fn push_event(lines: &mut Vec<String>, event: &Event) {
    let summary = event.get_summary().unwrap_or(NO_TITLE);
    lines.push(format!("calendar invite: {summary}"));
    push_field(lines, "organiser:", organiser(event));
    push_field(
        lines,
        "starts:",
        event.get_start().map(|moment| when(&moment)),
    );
    push_field(
        lines,
        "ends:",
        event.get_end().map(|moment| when(&moment)),
    );
    push_field(
        lines,
        "where:",
        event.get_location().map(str::to_string),
    );
    let attendees = attendee_names(event);
    push_field(
        lines,
        "attendees:",
        (!attendees.is_empty()).then(|| attendees.join(", ")),
    );
}

fn push_field(
    lines: &mut Vec<String>,
    label: &str,
    value: Option<String>,
) {
    let Some(value) = value else {
        return;
    };
    lines.push(format!("  {label:<LABEL_WIDTH$} {value}"));
}

fn organiser(event: &Event) -> Option<String> {
    let property = event.properties().get("ORGANIZER")?;
    let address = strip_mailto(property.value());
    let name =
        property.get_param_as("CN", |name| Some(name.to_string()));
    Some(named_address(name, address))
}

fn attendee_names(event: &Event) -> Vec<String> {
    event
        .get_attendees()
        .into_iter()
        .map(|attendee| {
            named_address(
                attendee.cn.clone(),
                strip_mailto(&attendee.cal_address),
            )
        })
        .collect()
}

fn named_address(name: Option<String>, address: String) -> String {
    match name {
        Some(name) => format!("{name} <{address}>"),
        None => address,
    }
}

fn strip_mailto(value: &str) -> String {
    let split = value.split_at_checked(MAILTO_SCHEME.len());
    let Some((scheme, rest)) = split else {
        return value.to_string();
    };
    if !scheme.eq_ignore_ascii_case(MAILTO_SCHEME) {
        return value.to_string();
    }
    rest.to_string()
}

fn when(moment: &DatePerhapsTime) -> String {
    match moment {
        DatePerhapsTime::Date(date) => {
            date.format(DATE_FORMAT).to_string()
        }
        DatePerhapsTime::DateTime(CalendarDateTime::Floating(time)) => {
            time.format(DATE_TIME_FORMAT).to_string()
        }
        DatePerhapsTime::DateTime(CalendarDateTime::Utc(time)) => {
            format!("{} UTC", time.format(DATE_TIME_FORMAT))
        }
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone {
            date_time,
            tzid,
        }) => {
            format!("{} ({tzid})", date_time.format(DATE_TIME_FORMAT))
        }
    }
}

#[cfg(test)]
mod tests {
    use icalendar::{Attendee, Property};

    use super::*;

    fn invite_message(ics: &str) -> Vec<u8> {
        let mut raw = String::from(
            "From: alba@example.com\r\n\
             To: me@example.com\r\n\
             Subject: invite\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; \
             boundary=\"b1\"\r\n\
             \r\n\
             --b1\r\n\
             Content-Type: text/plain\r\n\
             \r\n\
             See the attached invitation.\r\n\
             --b1\r\n\
             Content-Type: text/calendar; method=REQUEST\r\n\
             \r\n",
        );
        raw.push_str(ics);
        raw.push_str("\r\n--b1--\r\n");
        raw.into_bytes()
    }

    fn london(hour: u32, minute: u32) -> DatePerhapsTime {
        let date_time = chrono::NaiveDate::from_ymd_opt(2026, 8, 5)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap();
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone {
            date_time,
            tzid: "Europe/London".to_string(),
        })
    }

    #[test]
    fn a_built_request_renders_the_full_block() {
        let event = Event::new()
            .summary("Sprint review")
            .location("Room 2")
            .starts(london(14, 0))
            .ends(london(15, 0))
            .attendee(
                Attendee::new("mailto:bram@example.com".to_string())
                    .cn("Bram".to_string()),
            )
            .append_property(
                Property::new("ORGANIZER", "mailto:alba@example.com")
                    .add_parameter("CN", "Alba Voss")
                    .done(),
            )
            .done();
        let mut calendar = Calendar::new();
        calendar.push(event);
        calendar
            .append_property(Property::new("METHOD", METHOD_REQUEST));
        let raw = invite_message(&calendar.to_string());

        let lines = invite_lines(&raw);
        assert_eq!(
            lines,
            [
                "calendar invite: Sprint review",
                "  organiser: Alba Voss <alba@example.com>",
                "  starts:    05 Aug 2026 14:00 (Europe/London)",
                "  ends:      05 Aug 2026 15:00 (Europe/London)",
                "  where:     Room 2",
                "  attendees: Bram <bram@example.com>",
                "  reply:     accept/decline not yet wired",
            ]
        );
    }

    #[test]
    fn a_raw_folded_invite_parses_and_renders() {
        let ics = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "PRODID:-//Example//EN\r\n",
            "METHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:1@example.com\r\n",
            "DTSTAMP:20260720T090000Z\r\n",
            "DTSTART:20260805T130000Z\r\n",
            "DTEND;VALUE=DATE:20260806\r\n",
            "SUMMARY:Quarterly planning with the\r\n",
            "  whole platform group\r\n",
            "LOCATION:Room 2\r\n",
            "ORGANIZER;CN=Alba Voss:mailto:alba@example.com\r\n",
            "ATTENDEE;CN=Bram:mailto:bram@example.com\r\n",
            "ATTENDEE:mailto:cato@example.com\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        );
        let lines = invite_lines(&invite_message(ics));
        assert_eq!(
            lines,
            [
                "calendar invite: Quarterly planning with \
                 the whole platform group",
                "  organiser: Alba Voss <alba@example.com>",
                "  starts:    05 Aug 2026 13:00 UTC",
                "  ends:      06 Aug 2026",
                "  where:     Room 2",
                "  attendees: Bram <bram@example.com>, \
                 cato@example.com",
                "  reply:     accept/decline not yet wired",
            ]
        );
    }

    #[test]
    fn messages_without_a_calendar_part_render_nothing() {
        let plain = b"From: a@example.com\r\n\
            Subject: x\r\n\
            Content-Type: text/plain\r\n\r\nhello\r\n";
        assert!(invite_lines(plain).is_empty());
        assert!(invite_lines(b"").is_empty());
        let broken = invite_message("BEGIN:VCALENDAR\r\nnope");
        assert!(invite_lines(&broken).is_empty());
    }
}
