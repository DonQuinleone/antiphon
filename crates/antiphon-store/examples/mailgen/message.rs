pub(crate) const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3_600;
pub(crate) const SECS_PER_DAY: i64 = 86_400;

const WEEKDAY_NAMES: [&str; 7] =
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const UNIX_EPOCH_WEEKDAY: i64 = 4;
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep",
    "Oct", "Nov", "Dec",
];

pub(crate) struct MessageParts<'a> {
    pub(crate) from: &'a str,
    pub(crate) to: &'a str,
    pub(crate) subject: &'a str,
    pub(crate) date_unix: i64,
    pub(crate) message_id: &'a str,
    pub(crate) references: &'a [String],
    pub(crate) list_id: Option<&'a str>,
    pub(crate) body: &'a str,
}

pub(crate) fn render_message(parts: &MessageParts) -> String {
    let mut out = format!(
        "From: {}\nTo: {}\nSubject: {}\nDate: {}\n\
         Message-ID: <{}>\n",
        parts.from,
        parts.to,
        parts.subject,
        rfc2822_utc(parts.date_unix),
        parts.message_id,
    );
    if let Some(parent) = parts.references.last() {
        out.push_str(&format!("In-Reply-To: <{parent}>\n"));
        let chain: Vec<String> = parts
            .references
            .iter()
            .map(|id| format!("<{id}>"))
            .collect();
        out.push_str(&format!("References: {}\n", chain.join(" ")));
    }
    if let Some(list) = parts.list_id {
        out.push_str(&format!("List-Id: <{list}>\n"));
    }
    out.push('\n');
    out.push_str(parts.body);
    out
}

fn rfc2822_utc(unix: i64) -> String {
    let days = unix.div_euclid(SECS_PER_DAY);
    let seconds = unix.rem_euclid(SECS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let weekday = WEEKDAY_NAMES[(days + UNIX_EPOCH_WEEKDAY)
        .rem_euclid(WEEKDAY_NAMES.len() as i64)
        as usize];
    format!(
        "{weekday}, {day:02} {} {year} \
         {:02}:{:02}:{:02} +0000",
        MONTH_NAMES[(month - 1) as usize],
        seconds / SECS_PER_HOUR,
        seconds % SECS_PER_HOUR / SECS_PER_MINUTE,
        seconds % SECS_PER_MINUTE,
    )
}

// Hinnant's civil_from_days algorithm.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460
        + day_of_era / 36_524
        - day_of_era / 146_096)
        / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era
        - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}
