use std::fs;

use antiphon_store::StoreLayout;

use crate::Config;
use crate::corpus::{
    LIST_IDS, SENDER_POOL_SIZE, Sender, body_size, body_text,
    build_sender_pool, subject_line,
};
use crate::message::{
    MessageParts, SECS_PER_DAY, SECS_PER_MINUTE, render_message,
};
use crate::rng::{SplitMix64, pick_weighted, zipf_index};

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

const LIST_ID_FRACTION: f64 = 0.18;

const UNREAD_WINDOW_DAYS: i64 = 14;
const UNREAD_FRACTION_IN_WINDOW: f64 = 0.5;

const MAILDIR_CUR: &str = "cur";
const MAILDIR_NEW: &str = "new";
const MAILDIR_TMP: &str = "tmp";
const MAILDIR_SUBDIRS: [&str; 3] =
    [MAILDIR_CUR, MAILDIR_NEW, MAILDIR_TMP];
const SEEN_FLAG_SUFFIX: &str = ":2,S";

struct Generator<'a> {
    layout: &'a StoreLayout,
    rng: SplitMix64,
    senders: Vec<Sender>,
    accounts: usize,
    file_seq: usize,
}

pub(crate) fn generate(
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
