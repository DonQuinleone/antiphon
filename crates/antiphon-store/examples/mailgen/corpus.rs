use crate::rng::SplitMix64;

const SUBJECT_WORDS_MIN: usize = 2;
const SUBJECT_WORDS_MAX: usize = 9;

const BODY_MEDIAN_BYTES: usize = 2 * 1024;
const BODY_MIN_BYTES: usize = 80;
const BODY_MAX_BYTES: usize = 200 * 1024;
const BODY_SPREAD_DOUBLINGS: f64 = 3.3;
const BODY_LINE_WIDTH: usize = 72;
const BODY_PARAGRAPH_BREAK_CHANCE: f64 = 0.02;

pub(crate) const SENDER_POOL_SIZE: usize =
    FIRST_NAMES.len() * SURNAMES.len();

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

pub(crate) const LIST_IDS: &[&str] = &[
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

pub(crate) struct Sender {
    pub(crate) display: String,
    pub(crate) address: String,
}

pub(crate) fn build_sender_pool(rng: &mut SplitMix64) -> Vec<Sender> {
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

pub(crate) fn subject_line(rng: &mut SplitMix64) -> String {
    let span = (SUBJECT_WORDS_MAX - SUBJECT_WORDS_MIN + 1) as u64;
    let count = SUBJECT_WORDS_MIN + rng.below(span) as usize;
    let words: Vec<&str> = (0..count)
        .map(|_| WORD_POOL[rng.below(WORD_POOL.len() as u64) as usize])
        .collect();
    words.join(" ")
}

// Sum of four uniforms approximates a bell curve, giving
// log-normal-ish sizes around the median.
pub(crate) fn body_size(rng: &mut SplitMix64) -> usize {
    let bell = rng.unit() + rng.unit() + rng.unit() + rng.unit() - 2.0;
    let scale = (bell * BODY_SPREAD_DOUBLINGS).exp2();
    ((BODY_MEDIAN_BYTES as f64 * scale) as usize)
        .clamp(BODY_MIN_BYTES, BODY_MAX_BYTES)
}

pub(crate) fn body_text(
    rng: &mut SplitMix64,
    target_bytes: usize,
) -> String {
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
