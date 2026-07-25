use icalendar::{
    Calendar, CalendarComponent, Component, Event, Property,
};

const METHOD_REQUEST: &str = "REQUEST";
const METHOD_REPLY: &str = "REPLY";
const PRODID: &str = "-//antiphon//antiphon//EN";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rsvp {
    Accept,
    Tentative,
    Decline,
}

impl Rsvp {
    fn partstat(self) -> &'static str {
        match self {
            Rsvp::Accept => "ACCEPTED",
            Rsvp::Tentative => "TENTATIVE",
            Rsvp::Decline => "DECLINED",
        }
    }

    pub fn subject_prefix(self) -> &'static str {
        match self {
            Rsvp::Accept => "Accepted",
            Rsvp::Tentative => "Tentative",
            Rsvp::Decline => "Declined",
        }
    }
}

/// Everything the compose layer needs to send an iTIP reply:
/// where it goes, what it says, and the calendar part that
/// updates the organiser's copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItipReply {
    pub organiser: String,
    pub subject: String,
    pub ics: String,
}

/// Builds an RFC 5546 REPLY for the first REQUEST invite in
/// the message: same UID, organiser echoed, exactly one
/// attendee (the replying identity) carrying the PARTSTAT,
/// SEQUENCE preserved, nothing else altered.
pub fn itip_reply(
    raw: &[u8],
    attendee: &str,
    rsvp: Rsvp,
    now_unix: i64,
) -> Option<ItipReply> {
    let source = request_calendar(raw)?;
    let event =
        source.components.iter().find_map(
            |component| match component {
                CalendarComponent::Event(event) => Some(event),
                _ => None,
            },
        )?;
    let uid = event.get_uid()?.to_owned();
    let organiser_prop = event.property_value("ORGANIZER")?;
    let organiser = organiser_prop
        .trim_start_matches("mailto:")
        .trim_start_matches("MAILTO:")
        .to_owned();
    let summary = event.get_summary().unwrap_or("event");

    let mut reply = Event::new();
    reply.uid(&uid);
    reply.add_property("DTSTAMP", format_utc(now_unix));
    reply.add_property("ORGANIZER", organiser_prop.to_owned());
    let mut attendee_prop =
        Property::new("ATTENDEE", format!("mailto:{attendee}"));
    attendee_prop.add_parameter("PARTSTAT", rsvp.partstat());
    reply.append_property(attendee_prop);
    if let Some(sequence) = event.property_value("SEQUENCE") {
        reply.add_property("SEQUENCE", sequence.to_owned());
    }

    let mut calendar = Calendar::new();
    calendar
        .append_property(Property::new("PRODID", PRODID))
        .append_property(Property::new("METHOD", METHOD_REPLY));
    calendar.push(reply.done());
    Some(ItipReply {
        subject: format!("{}: {summary}", rsvp.subject_prefix()),
        organiser,
        ics: calendar.to_string(),
    })
}

fn request_calendar(raw: &[u8]) -> Option<Calendar> {
    let text = crate::invite::calendar_text(raw)?;
    let calendar: Calendar = text.parse().ok()?;
    let is_request =
        calendar.property_value("METHOD") == Some(METHOD_REQUEST);
    if !is_request {
        return None;
    }
    Some(calendar)
}

fn format_utc(unix: i64) -> String {
    use chrono::{DateTime, Utc};
    let stamp =
        DateTime::<Utc>::from_timestamp(unix, 0).unwrap_or_default();
    stamp.format("%Y%m%dT%H%M%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: i64 = 1_784_800_000;

    fn invite_message(method: &str, sequence: &str) -> Vec<u8> {
        let ics = [
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//Example//Test//EN",
            &format!("METHOD:{method}"),
            "BEGIN:VEVENT",
            "UID:planning-7@example.com",
            "DTSTAMP:20260724T210000Z",
            "DTSTART:20260801T140000Z",
            "SUMMARY:Planning call",
            &format!("SEQUENCE:{sequence}"),
            "ORGANIZER;CN=Alba:mailto:alba@example.com",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:me@example.com",
            "END:VEVENT",
            "END:VCALENDAR",
        ]
        .join("\r\n");
        format!(
            "From: alba@example.com\r\n\
             To: me@example.com\r\n\
             Subject: invite\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/calendar; method={method}\r\n\
             \r\n\
             {ics}\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn a_reply_carries_uid_partstat_and_sequence() {
        let raw = invite_message("REQUEST", "3");
        let reply =
            itip_reply(&raw, "me@example.com", Rsvp::Decline, NOON)
                .unwrap();
        assert_eq!(reply.organiser, "alba@example.com");
        assert_eq!(reply.subject, "Declined: Planning call");
        assert!(reply.ics.contains("METHOD:REPLY"));
        assert!(reply.ics.contains("UID:planning-7@example.com"));
        assert!(
            reply.ics.contains("PARTSTAT=DECLINED"),
            "{}",
            reply.ics
        );
        assert!(reply.ics.contains("SEQUENCE:3"));
        assert!(
            reply.ics.contains("mailto:me@example.com"),
            "{}",
            reply.ics
        );
    }

    #[test]
    fn accept_and_tentative_map_their_partstat() {
        let raw = invite_message("REQUEST", "0");
        for (rsvp, partstat) in [
            (Rsvp::Accept, "PARTSTAT=ACCEPTED"),
            (Rsvp::Tentative, "PARTSTAT=TENTATIVE"),
        ] {
            let reply =
                itip_reply(&raw, "me@example.com", rsvp, NOON).unwrap();
            assert!(reply.ics.contains(partstat), "{}", reply.ics);
        }
    }

    #[test]
    fn only_requests_produce_replies() {
        let raw = invite_message("PUBLISH", "0");
        assert!(
            itip_reply(&raw, "me@example.com", Rsvp::Accept, NOON)
                .is_none()
        );
    }
}
