use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use antiphon_store::StoreLayout;

const USAGE: &str = "usage: mailgen --root <dir> --messages <n> \
     [--accounts <n>] [--seed <n>]";

const DEFAULT_ACCOUNTS: usize = 6;
const DEFAULT_SEED: u64 = 1;

const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3_600;
const SECS_PER_DAY: i64 = 86_400;

// Fixed "now" (2026-07-01T00:00:00Z): output depends on the seed
// alone, never on the clock.
const SYNTHETIC_NOW_UNIX: i64 = 1_782_864_000;
const DATE_SPAN_DAYS: i64 = 3_650;
const RECENT_DATE_BIAS_EXPONENT: f64 = 2.0;

const SHORT_THREAD_DEPTH_WEIGHTS: &[(usize, u32)] =
    &[(1, 55), (2, 30), (3, 15)];
const LONG_THREAD_FRACTION: f64 = 0.05;
const LONG_THREAD_DEPTH_MIN: usize = 4;
const LONG_THREAD_DEPTH_MAX: usize = 25;
const THREAD_EXTRA_PARTICIPANTS_MAX: usize = 4;

const REPLY_GAP_MIN_SECS: i64 = SECS_PER_MINUTE;
const REPLY_GAP_MAX_SECS: i64 = 3 * SECS_PER_DAY;
const REPLY_GAP_BIAS_EXPONENT: f64 = 2.0;

const FOLDER_SENT: &str = "sent";
const FOLDER_WEIGHTS: &[(&str, u32)] = &[
    ("inbox", 52),
    ("archive", 28),
    (FOLDER_SENT, 9),
    ("lists", 6),
    ("junk", 3),
    ("drafts", 2),
];

const SUBJECT_WORDS_MIN: usize = 2;
const SUBJECT_WORDS_MAX: usize = 9;

const BODY_MEDIAN_BYTES: usize = 2 * 1024;
const BODY_MIN_BYTES: usize = 80;
const BODY_MAX_BYTES: usize = 200 * 1024;
const BODY_SPREAD_DOUBLINGS: f64 = 3.3;
const BODY_LINE_WIDTH: usize = 72;
const BODY_PARAGRAPH_BREAK_CHANCE: f64 = 0.02;

const LIST_ID_FRACTION: f64 = 0.18;

const UNREAD_WINDOW_DAYS: i64 = 14;
const UNREAD_FRACTION_IN_WINDOW: f64 = 0.5;

const MAILDIR_CUR: &str = "cur";
const MAILDIR_NEW: &str = "new";
const MAILDIR_TMP: &str = "tmp";
const MAILDIR_SUBDIRS: [&str; 3] =
    [MAILDIR_CUR, MAILDIR_NEW, MAILDIR_TMP];
const SEEN_FLAG_SUFFIX: &str = ":2,S";

const SENDER_POOL_SIZE: usize = FIRST_NAMES.len() * SURNAMES.len();

const FIRST_NAMES: &[&str] = &[
    "Alba", "Bram", "Cato", "Dara", "Elio", "Fern", "Gwen", "Hale",
    "Iris", "Joss", "Kiri", "Lars", "Mira", "Nils", "Orla", "Pia",
    "Quin", "Rafa", "Sena", "Tobi", "Una", "Vera", "Wren", "Xeno",
    "Yara", "Zeph", "Ansel", "Boden", "Clea", "Doran", "Edda", "Falk",
    "Greer", "Halla", "Ivor", "Juna", "Kellan", "Lumi", "Merit",
    "Nova",
];

const SURNAMES: &[&str] = &[
    "Ashdown",
    "Birchwood",
    "Cresswell",
    "Dunmore",
    "Elderfield",
    "Fenwick",
    "Garrow",
    "Hazelden",
    "Ironwood",
    "Jessop",
    "Kingsmead",
    "Larkspur",
    "Marwood",
    "Netherby",
    "Oakhurst",
    "Pemberly",
    "Quillon",
    "Ravenshaw",
    "Silverton",
    "Thornbury",
    "Underhill",
    "Vexley",
    "Wexcombe",
    "Yarrow",
    "Zellwood",
    "Ambleside",
    "Bracken",
    "Corvel",
    "Dellmont",
    "Eastholm",
    "Farrowby",
    "Glenmore",
    "Harwick",
    "Inglewood",
    "Jarrow",
    "Kelbrook",
    "Lindenmere",
    "Moorfen",
    "Nightley",
    "Ostwick",
    "Penhale",
    "Quarrell",
    "Redfern",
    "Stonewick",
    "Tarnfield",
    "Umberly",
    "Varnley",
    "Whitlow",
    "Yewdale",
    "Zephrin",
];

const DOMAINS: &[&str] = &[
    "example.com",
    "example.org",
    "example.net",
    "mail.example",
    "corp.example",
    "dev.example",
    "lists.example",
    "shop.example",
];

const LIST_IDS: &[&str] = &[
    "patches.lists.example",
    "announce.lists.example",
    "dev.lists.example",
    "users.lists.example",
    "security.lists.example",
    "builds.lists.example",
    "review.lists.example",
    "ops.lists.example",
    "design.lists.example",
    "random.lists.example",
    "release.lists.example",
    "infra.lists.example",
];

const WORD_POOL: &[&str] = &[
    "the",
    "archive",
    "vault",
    "message",
    "thread",
    "index",
    "folder",
    "reply",
    "draft",
    "queue",
    "ledger",
    "harbour",
    "garden",
    "window",
    "lantern",
    "meadow",
    "copper",
    "river",
    "branch",
    "signal",
    "cache",
    "packet",
    "stanza",
    "chorus",
    "anthem",
    "marble",
    "timber",
    "quarry",
    "beacon",
    "cipher",
    "syntax",
    "kernel",
    "module",
    "socket",
    "buffer",
    "stream",
    "tunnel",
    "canvas",
    "palette",
    "fresco",
    "sonata",
    "tempo",
    "cadence",
    "metric",
    "budget",
    "margin",
    "docket",
    "minute",
    "agenda",
    "motion",
    "tally",
    "census",
    "survey",
    "atlas",
    "compass",
    "meridian",
    "summit",
    "valley",
    "harvest",
    "orchard",
    "cellar",
    "pantry",
    "kettle",
    "saffron",
    "juniper",
    "bramble",
    "heather",
    "willow",
    "alder",
    "rowan",
    "gorse",
    "fern",
    "moss",
    "slate",
    "granite",
    "basalt",
    "pebble",
    "shore",
    "tide",
    "current",
    "estuary",
    "delta",
    "inlet",
    "channel",
    "lagoon",
    "reef",
    "drift",
    "breeze",
    "zephyr",
    "aurora",
    "twilight",
    "ember",
    "lumen",
    "prism",
    "spectrum",
    "echo",
    "resonance",
    "timbre",
];

const WEEKDAY_NAMES: [&str; 7] =
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const UNIX_EPOCH_WEEKDAY: i64 = 4;
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep",
    "Oct", "Nov", "Dec",
];

const F64_MANTISSA_BITS: u32 = 53;

struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed =
            (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed =
            (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    fn unit(&mut self) -> f64 {
        let mantissa =
            self.next_u64() >> (u64::BITS - F64_MANTISSA_BITS);
        mantissa as f64 / (1u64 << F64_MANTISSA_BITS) as f64
    }

    fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }
}

struct Config {
    root: PathBuf,
    messages: usize,
    accounts: usize,
    seed: u64,
}

struct Sender {
    display: String,
    address: String,
}

struct MessageParts<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    date_unix: i64,
    message_id: &'a str,
    references: &'a [String],
    list_id: Option<&'a str>,
    body: &'a str,
}

struct Generator<'a> {
    layout: &'a StoreLayout,
    rng: SplitMix64,
    senders: Vec<Sender>,
    accounts: usize,
    file_seq: usize,
}

fn main() -> ExitCode {
    let config = match parse_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("mailgen: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Err(message) = run(&config) {
        eprintln!("mailgen: {message}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn parse_args() -> Result<Config, String> {
    let mut root = None;
    let mut messages = None;
    let mut accounts = DEFAULT_ACCOUNTS;
    let mut seed = DEFAULT_SEED;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--root" => root = Some(PathBuf::from(&value)),
            "--messages" => {
                messages = Some(parse_count(&flag, &value)?);
            }
            "--accounts" => {
                accounts = parse_count(&flag, &value)?;
            }
            "--seed" => {
                seed = value.parse().map_err(|_| {
                    format!(
                        "--seed wants an unsigned integer, \
                         got {value}"
                    )
                })?;
            }
            unknown => {
                return Err(format!("unknown flag {unknown}"));
            }
        }
    }
    let root =
        root.ok_or_else(|| String::from("--root is required"))?;
    let messages = messages
        .ok_or_else(|| String::from("--messages is required"))?;
    Ok(Config {
        root,
        messages,
        accounts,
        seed,
    })
}

fn parse_count(flag: &str, value: &str) -> Result<usize, String> {
    let parsed: usize = value.parse().map_err(|_| {
        format!("{flag} wants a positive integer, got {value}")
    })?;
    if parsed == 0 {
        return Err(format!("{flag} must be at least 1"));
    }
    Ok(parsed)
}

fn run(config: &Config) -> Result<(), String> {
    let layout = StoreLayout::new(&config.root);
    layout.init().map_err(|source| {
        format!(
            "initialising store at {}: {source}",
            layout.root().display()
        )
    })?;
    let generating = Instant::now();
    let written = generate(&layout, config)?;
    let generated_in = generating.elapsed();
    let indexing = Instant::now();
    run_notmuch_new(&layout.notmuch_config_path())?;
    let indexed_in = indexing.elapsed();
    println!(
        "wrote {written} messages across {} accounts \
         in {generated_in:.2?}",
        config.accounts
    );
    println!("notmuch new indexed the store in {indexed_in:.2?}");
    println!("store root: {}", layout.root().display());
    Ok(())
}

fn generate(
    layout: &StoreLayout,
    config: &Config,
) -> Result<usize, String> {
    create_folder_tree(layout, config.accounts)?;
    let mut rng = SplitMix64::new(config.seed);
    let senders = build_sender_pool(&mut rng);
    let mut generator = Generator {
        layout,
        rng,
        senders,
        accounts: config.accounts,
        file_seq: 0,
    };
    let mut written = 0;
    let mut thread_seq = 0;
    while written < config.messages {
        let budget = config.messages - written;
        written += generator.write_thread(thread_seq, budget)?;
        thread_seq += 1;
    }
    Ok(written)
}

fn create_folder_tree(
    layout: &StoreLayout,
    accounts: usize,
) -> Result<(), String> {
    for account in 0..accounts {
        let root = layout.account_maildir(&account_name(account));
        for (folder, _) in FOLDER_WEIGHTS {
            for sub in MAILDIR_SUBDIRS {
                let dir = root.join(folder).join(sub);
                fs::create_dir_all(&dir).map_err(|source| {
                    format!("creating {}: {source}", dir.display())
                })?;
            }
        }
    }
    Ok(())
}

fn account_name(index: usize) -> String {
    format!("acct{index}")
}

fn build_sender_pool(rng: &mut SplitMix64) -> Vec<Sender> {
    (0..SENDER_POOL_SIZE)
        .map(|index| {
            let first = FIRST_NAMES[index % FIRST_NAMES.len()];
            let last = SURNAMES[index / FIRST_NAMES.len()];
            let domain =
                DOMAINS[rng.below(DOMAINS.len() as u64) as usize];
            Sender {
                display: format!("{first} {last}"),
                address: format!(
                    "{}.{}@{domain}",
                    first.to_lowercase(),
                    last.to_lowercase()
                ),
            }
        })
        .collect()
}

impl Generator<'_> {
    fn write_thread(
        &mut self,
        thread_seq: usize,
        budget: usize,
    ) -> Result<usize, String> {
        let depth = thread_depth(&mut self.rng).min(budget);
        let account =
            account_name(self.rng.below(self.accounts as u64) as usize);
        let account_address = format!("{account}@example.com");
        let account_mailbox =
            format!("Synthetic Account <{account_address}>");
        let participants = self.draw_participants();
        let list_id = self.rng.chance(LIST_ID_FRACTION).then(|| {
            LIST_IDS[self.rng.below(LIST_IDS.len() as u64) as usize]
        });
        let root_subject = subject_line(&mut self.rng);
        let mut date_unix = thread_start_unix(&mut self.rng);
        let mut references: Vec<String> = Vec::new();
        for position in 0..depth {
            let message_id = format!(
                "mailgen.t{thread_seq}.m{position}\
                 @synthetic.example"
            );
            let folder = *pick_weighted(&mut self.rng, FOLDER_WEIGHTS);
            let counterpart = self.draw_counterpart(&participants);
            let (from, to) = if folder == FOLDER_SENT {
                (account_mailbox.clone(), counterpart)
            } else {
                (counterpart, account_mailbox.clone())
            };
            let subject = if position == 0 {
                root_subject.clone()
            } else {
                format!("Re: {root_subject}")
            };
            let size = body_size(&mut self.rng);
            let body = body_text(&mut self.rng, size);
            let message = render_message(&MessageParts {
                from: &from,
                to: &to,
                subject: &subject,
                date_unix,
                message_id: &message_id,
                references: &references,
                list_id,
                body: &body,
            });
            self.write_message(&account, folder, date_unix, &message)?;
            references.push(message_id);
            date_unix += reply_gap_secs(&mut self.rng);
        }
        Ok(depth)
    }

    fn draw_participants(&mut self) -> Vec<usize> {
        let extras =
            self.rng.below(THREAD_EXTRA_PARTICIPANTS_MAX as u64 + 1)
                as usize;
        (0..1 + extras)
            .map(|_| zipf_index(&mut self.rng, SENDER_POOL_SIZE))
            .collect()
    }

    fn draw_counterpart(&mut self, participants: &[usize]) -> String {
        let pick = participants
            [self.rng.below(participants.len() as u64) as usize];
        let sender = &self.senders[pick];
        format!("{} <{}>", sender.display, sender.address)
    }

    fn write_message(
        &mut self,
        account: &str,
        folder: &str,
        date_unix: i64,
        content: &str,
    ) -> Result<(), String> {
        let age_secs = SYNTHETIC_NOW_UNIX - date_unix;
        let recent = age_secs < UNREAD_WINDOW_DAYS * SECS_PER_DAY;
        let unread = folder != FOLDER_SENT
            && recent
            && self.rng.chance(UNREAD_FRACTION_IN_WINDOW);
        let (subdir, suffix) = if unread {
            (MAILDIR_NEW, "")
        } else {
            (MAILDIR_CUR, SEEN_FLAG_SUFFIX)
        };
        let name =
            format!("{date_unix}.m{}.mailgen{suffix}", self.file_seq);
        self.file_seq += 1;
        let path = self
            .layout
            .account_maildir(account)
            .join(folder)
            .join(subdir)
            .join(name);
        fs::write(&path, content).map_err(|source| {
            format!("writing {}: {source}", path.display())
        })
    }
}

fn thread_depth(rng: &mut SplitMix64) -> usize {
    if rng.chance(LONG_THREAD_FRACTION) {
        let span =
            (LONG_THREAD_DEPTH_MAX - LONG_THREAD_DEPTH_MIN + 1) as u64;
        return LONG_THREAD_DEPTH_MIN + rng.below(span) as usize;
    }
    *pick_weighted(rng, SHORT_THREAD_DEPTH_WEIGHTS)
}

fn pick_weighted<'t, T>(
    rng: &mut SplitMix64,
    table: &'t [(T, u32)],
) -> &'t T {
    let total: u32 = table.iter().map(|(_, weight)| *weight).sum();
    let mut roll = rng.below(u64::from(total)) as u32;
    for (item, weight) in table {
        if roll < *weight {
            return item;
        }
        roll -= weight;
    }
    unreachable!("weighted roll exceeded {total}")
}

// pool^u inverts the zipf(1) CDF, so low ranks dominate.
fn zipf_index(rng: &mut SplitMix64, pool: usize) -> usize {
    let rank = (pool as f64).powf(rng.unit()) as usize;
    rank.clamp(1, pool) - 1
}

fn subject_line(rng: &mut SplitMix64) -> String {
    let span = (SUBJECT_WORDS_MAX - SUBJECT_WORDS_MIN + 1) as u64;
    let count = SUBJECT_WORDS_MIN + rng.below(span) as usize;
    let words: Vec<&str> = (0..count)
        .map(|_| WORD_POOL[rng.below(WORD_POOL.len() as u64) as usize])
        .collect();
    words.join(" ")
}

fn thread_start_unix(rng: &mut SplitMix64) -> i64 {
    let age_days = DATE_SPAN_DAYS as f64
        * rng.unit().powf(RECENT_DATE_BIAS_EXPONENT);
    SYNTHETIC_NOW_UNIX - (age_days * SECS_PER_DAY as f64) as i64
}

fn reply_gap_secs(rng: &mut SplitMix64) -> i64 {
    let spread = (REPLY_GAP_MAX_SECS - REPLY_GAP_MIN_SECS) as f64;
    REPLY_GAP_MIN_SECS
        + (spread * rng.unit().powf(REPLY_GAP_BIAS_EXPONENT)) as i64
}

// Sum of four uniforms approximates a bell curve, giving
// log-normal-ish sizes around the median.
fn body_size(rng: &mut SplitMix64) -> usize {
    let bell = rng.unit() + rng.unit() + rng.unit() + rng.unit() - 2.0;
    let scale = (bell * BODY_SPREAD_DOUBLINGS).exp2();
    ((BODY_MEDIAN_BYTES as f64 * scale) as usize)
        .clamp(BODY_MIN_BYTES, BODY_MAX_BYTES)
}

fn body_text(rng: &mut SplitMix64, target_bytes: usize) -> String {
    let mut body =
        String::with_capacity(target_bytes + BODY_LINE_WIDTH);
    let mut line_len = 0;
    while body.len() < target_bytes {
        let word =
            WORD_POOL[rng.below(WORD_POOL.len() as u64) as usize];
        if line_len > 0 && line_len + 1 + word.len() > BODY_LINE_WIDTH {
            body.push('\n');
            line_len = 0;
            if rng.chance(BODY_PARAGRAPH_BREAK_CHANCE) {
                body.push('\n');
            }
        }
        if line_len > 0 {
            body.push(' ');
            line_len += 1;
        }
        body.push_str(word);
        line_len += word.len();
    }
    body.push('\n');
    body
}

fn render_message(parts: &MessageParts) -> String {
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

fn run_notmuch_new(config: &Path) -> Result<(), String> {
    let out = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", config)
        .output()
        .map_err(|source| format!("running notmuch new: {source}"))?;
    if !out.status.success() {
        return Err(format!(
            "notmuch new failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
