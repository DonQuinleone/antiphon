use std::collections::HashMap;
use std::num::NonZeroU32;

use imap_client::client::tokio::{Client, ClientError};
use imap_client::imap_types::command::CommandBody;
use imap_client::imap_types::core::Vec1;
use imap_client::imap_types::fetch::{
    MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName,
};
use imap_client::imap_types::flag::{
    Flag, FlagFetch, FlagNameAttribute, StoreType,
};
use imap_client::imap_types::mailbox::Mailbox;
use imap_client::imap_types::response::{Data, StatusBody, StatusKind};
use imap_client::imap_types::sequence::{
    SeqOrUid, Sequence, SequenceSet,
};
use imap_client::tasks::Task;
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::tasks::logout::LogoutTask;
use imap_client::tasks::tasks::select::SelectDataUnvalidated;
use tokio::runtime::{Builder, Runtime};

use crate::auth::Auth;
use crate::engine::{RemoteFolder, SyncAccount};
use crate::error::SyncError;

/// Synchronous facade over the async imap-client stack. The
/// tokio runtime lives here and never leaks past this module:
/// every method drives one command to completion via block_on.
pub(crate) struct ImapSession {
    pub(crate) runtime: Runtime,
    pub(crate) client: Client,
}

pub(crate) struct SelectedFolder {
    pub uid_validity: Option<u32>,
    pub exists: u32,
    pub uid_next: Option<u32>,
}

pub(crate) struct FetchedMessage {
    pub uid: u32,
    pub seen: bool,
    pub body: Option<Vec<u8>>,
}

impl ImapSession {
    pub fn connect(account: &SyncAccount) -> Result<Self, SyncError> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| SyncError::Runtime { source })?;
        let mut client = runtime
            .block_on(Client::rustls(
                account.host.as_str(),
                account.port,
                false,
                None,
            ))
            .map_err(|source| SyncError::Connect {
                host: account.host.clone(),
                port: account.port,
                source: Box::new(source),
            })?;
        let authenticated = match &account.auth {
            Auth::Password(password) => runtime.block_on(
                client.login(account.user.as_str(), password.as_str()),
            ),
            Auth::XOauth2 { user, access_token } => {
                runtime.block_on(client.authenticate_xoauth2(
                    user.as_str(),
                    access_token.as_str(),
                ))
            }
        };
        authenticated.map_err(|source| SyncError::Login {
            user: account.user.clone(),
            source: Box::new(source),
        })?;
        Ok(Self { runtime, client })
    }

    pub fn list_selectable(
        &mut self,
    ) -> Result<Vec<RemoteFolder>, ClientError> {
        let listed =
            self.runtime.block_on(self.client.list("", "*"))?;
        let folders = listed
            .into_iter()
            .filter(|(_, _, attributes)| {
                !attributes.contains(&FlagNameAttribute::Noselect)
            })
            .map(|(mailbox, delimiter, _)| RemoteFolder {
                name: mailbox_name(&mailbox),
                delimiter: delimiter.map(|sep| sep.inner().to_string()),
            })
            .collect();
        Ok(folders)
    }

    pub fn examine(
        &mut self,
        folder: &str,
    ) -> Result<SelectedFolder, ClientError> {
        let data =
            self.runtime.block_on(self.client.examine(folder))?;
        Ok(selected_from(data))
    }

    pub fn select(
        &mut self,
        folder: &str,
    ) -> Result<SelectedFolder, ClientError> {
        let data = self.runtime.block_on(self.client.select(folder))?;
        Ok(selected_from(data))
    }

    /// Fetches flags and full bodies for every message with a
    /// UID at or above `first_uid`, in ascending UID order.
    /// Only the UIDs, so a large folder can be fetched in
    /// bounded batches instead of one unbounded response.
    pub fn list_new_uids(
        &mut self,
        first_uid: u32,
    ) -> Result<Vec<u32>, ClientError> {
        let fetched = self.uid_fetch(
            range_from(first_uid),
            vec![MessageDataItemName::Uid],
        )?;
        let mut uids: Vec<u32> =
            fetched.into_keys().map(|uid| uid.get()).collect();
        uids.sort_unstable();
        Ok(uids)
    }

    pub fn fetch_uids(
        &mut self,
        uids: &[u32],
    ) -> Result<Vec<FetchedMessage>, ClientError> {
        let Some(set) = uid_set(uids) else {
            return Ok(Vec::new());
        };
        let items = vec![
            MessageDataItemName::Flags,
            MessageDataItemName::BodyExt {
                section: None,
                partial: None,
                peek: true,
            },
        ];
        let fetched = self.uid_fetch(set, items)?;
        let mut messages: Vec<FetchedMessage> = fetched
            .into_iter()
            .map(|(uid, items)| FetchedMessage {
                uid: uid.get(),
                seen: items_seen(&items),
                body: items_body(items),
            })
            .collect();
        messages.sort_by_key(|message| message.uid);
        Ok(messages)
    }

    pub fn fetch_seen_flags(
        &mut self,
        first_uid: u32,
        last_uid: u32,
    ) -> Result<HashMap<u32, bool>, ClientError> {
        let fetched = self.uid_fetch(
            range(first_uid, last_uid),
            vec![MessageDataItemName::Flags],
        )?;
        Ok(fetched
            .into_iter()
            .map(|(uid, items)| (uid.get(), items_seen(&items)))
            .collect())
    }

    pub fn uid_exists(
        &mut self,
        uid: u32,
    ) -> Result<bool, ClientError> {
        let fetched = self.uid_fetch(single(uid), Vec::new())?;
        Ok(fetched.keys().any(|found| found.get() == uid))
    }

    pub fn uid_store(
        &mut self,
        uid: u32,
        kind: StoreType,
        flags: Vec<Flag<'static>>,
    ) -> Result<(), ClientError> {
        self.runtime.block_on(self.client.uid_silent_store(
            single(uid),
            kind,
            flags,
        ))
    }

    /// APPEND with the given flags; returns the new message's
    /// (uid, uidvalidity) when the server grants APPENDUID
    /// (RFC 4315), None from servers without UIDPLUS.
    pub fn append(
        &mut self,
        folder: &str,
        flags: Vec<Flag<'static>>,
        raw: &[u8],
    ) -> Result<Option<(u32, u32)>, ClientError> {
        let appended = self
            .runtime
            .block_on(self.client.appenduid(folder, flags, raw))?;
        Ok(appended.map(|(uid, validity)| (uid.get(), validity.get())))
    }

    pub fn uid_expunge(&mut self, uid: u32) -> Result<(), ClientError> {
        let task = UidExpungeTask {
            sequence_set: single(uid),
        };
        Ok(self.runtime.block_on(self.client.resolve(task))??)
    }

    pub fn expunge(&mut self) -> Result<(), ClientError> {
        self.runtime.block_on(self.client.expunge())?;
        Ok(())
    }

    pub fn supports_uidplus(&self) -> bool {
        self.client.state.ext_uidplus_supported()
    }

    pub fn logout(mut self) {
        let _ = self
            .runtime
            .block_on(self.client.resolve(LogoutTask::new()));
    }

    fn uid_fetch(
        &mut self,
        set: SequenceSet,
        items: Vec<MessageDataItemName<'static>>,
    ) -> Result<
        HashMap<NonZeroU32, Vec1<MessageDataItem<'static>>>,
        ClientError,
    > {
        let items =
            MacroOrMessageDataItemNames::MessageDataItemNames(items);
        self.runtime.block_on(self.client.uid_fetch(set, items))
    }
}

/// UID EXPUNGE (RFC 4315). imap-client ships no task for it,
/// but imap-codec encodes CommandBody::ExpungeUid, so this
/// custom task fills the gap.
struct UidExpungeTask {
    sequence_set: SequenceSet,
}

impl Task for UidExpungeTask {
    type Output = Result<(), TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::ExpungeUid {
            sequence_set: self.sequence_set.clone(),
        }
    }

    fn process_data(
        &mut self,
        data: Data<'static>,
    ) -> Option<Data<'static>> {
        match data {
            Data::Expunge(_) => None,
            other => Some(other),
        }
    }

    fn process_tagged(
        self,
        status_body: StatusBody<'static>,
    ) -> Self::Output {
        match status_body.kind {
            StatusKind::Ok => Ok(()),
            StatusKind::No => {
                Err(TaskError::UnexpectedNoResponse(status_body))
            }
            StatusKind::Bad => {
                Err(TaskError::UnexpectedBadResponse(status_body))
            }
        }
    }
}

fn selected_from(data: SelectDataUnvalidated) -> SelectedFolder {
    SelectedFolder {
        uid_validity: data.uid_validity.map(NonZeroU32::get),
        exists: data.exists.unwrap_or(0),
        uid_next: data.uid_next.map(NonZeroU32::get),
    }
}

fn mailbox_name(mailbox: &Mailbox<'_>) -> String {
    match mailbox {
        Mailbox::Inbox => String::from("INBOX"),
        Mailbox::Other(other) => {
            String::from_utf8_lossy(other.as_ref()).into_owned()
        }
    }
}

fn uid_value(uid: u32) -> SeqOrUid {
    SeqOrUid::Value(NonZeroU32::new(uid).expect("uids are never zero"))
}

fn uid_set(uids: &[u32]) -> Option<SequenceSet> {
    let sequences: Vec<Sequence> = uids
        .iter()
        .map(|uid| Sequence::Single(uid_value(*uid)))
        .collect();
    Some(SequenceSet(Vec1::try_from(sequences).ok()?))
}

fn single(uid: u32) -> SequenceSet {
    SequenceSet(Vec1::from(Sequence::Single(uid_value(uid))))
}

fn range_from(first: u32) -> SequenceSet {
    SequenceSet(Vec1::from(Sequence::Range(
        uid_value(first),
        SeqOrUid::Asterisk,
    )))
}

fn range(first: u32, last: u32) -> SequenceSet {
    SequenceSet(Vec1::from(Sequence::Range(
        uid_value(first),
        uid_value(last),
    )))
}

fn items_seen(items: &Vec1<MessageDataItem<'_>>) -> bool {
    items.as_ref().iter().any(|item| {
        matches!(
            item,
            MessageDataItem::Flags(flags)
                if flags.iter().any(|flag| {
                    matches!(flag, FlagFetch::Flag(Flag::Seen))
                })
        )
    })
}

fn items_body(
    items: Vec1<MessageDataItem<'static>>,
) -> Option<Vec<u8>> {
    items.into_inner().into_iter().find_map(|item| {
        let MessageDataItem::BodyExt { data, .. } = item else {
            return None;
        };
        data.into_option().map(|content| content.into_owned())
    })
}

/// connect() delegates the XOAUTH2 exchange to imap-client's
/// AuthenticateTask, and append() delegates APPEND to its
/// AppendUidTask; these tests pin the SASL line, SASL-IR and
/// APPENDUID behaviour that delegation relies on.
#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use imap_client::imap_types::auth::AuthMechanism;
    use imap_client::imap_types::command::CommandBody;
    use imap_client::imap_types::core::{Literal, Text};
    use imap_client::imap_types::extensions::binary::LiteralOrLiteral8;
    use imap_client::imap_types::flag::Flag;
    use imap_client::imap_types::mailbox::Mailbox;
    use imap_client::imap_types::response::{
        Code, StatusBody, StatusKind,
    };
    use imap_client::tasks::Task;
    use imap_client::tasks::tasks::appenduid::AppendUidTask;
    use imap_client::tasks::tasks::authenticate::AuthenticateTask;

    fn body_of(
        task: &AuthenticateTask,
    ) -> (AuthMechanism<'static>, Option<Vec<u8>>) {
        let CommandBody::Authenticate {
            mechanism,
            initial_response,
        } = task.command_body()
        else {
            panic!("not an AUTHENTICATE command");
        };
        let line = initial_response
            .map(|secret| secret.declassify().clone().into_owned());
        (mechanism, line)
    }

    #[test]
    fn xoauth2_task_sends_the_sasl_line_inline_with_ir() {
        let task = AuthenticateTask::xoauth2(
            "quin@example.com",
            "token-1",
            true,
        );
        let (mechanism, line) = body_of(&task);
        assert_eq!(mechanism, AuthMechanism::XOAuth2);
        assert_eq!(
            line.expect("initial response"),
            b"user=quin@example.com\x01auth=Bearer token-1\x01\x01"
        );
    }

    fn append_task(flags: Vec<Flag<'static>>) -> AppendUidTask {
        let literal =
            Literal::try_from(&b"Subject: d\r\n\r\nbody\r\n"[..])
                .unwrap();
        AppendUidTask::new(
            Mailbox::try_from("Drafts").unwrap(),
            LiteralOrLiteral8::Literal(literal),
        )
        .with_flags(flags)
    }

    fn tagged_ok(code: Option<Code<'static>>) -> StatusBody<'static> {
        StatusBody {
            kind: StatusKind::Ok,
            code,
            text: Text::try_from("APPEND done").unwrap(),
        }
    }

    #[test]
    fn append_task_sends_the_flags_with_the_message() {
        let task = append_task(vec![Flag::Draft, Flag::Seen]);
        let CommandBody::Append { mailbox, flags, .. } =
            task.command_body()
        else {
            panic!("not an APPEND command");
        };
        assert_eq!(mailbox, Mailbox::try_from("Drafts").unwrap());
        assert_eq!(flags, [Flag::Draft, Flag::Seen]);
    }

    #[test]
    fn append_task_reads_uid_and_validity_from_appenduid() {
        let code = Code::AppendUid {
            uid_validity: NonZeroU32::new(3).unwrap(),
            uid: NonZeroU32::new(7).unwrap(),
        };
        let granted = append_task(vec![Flag::Draft])
            .process_tagged(tagged_ok(Some(code)))
            .unwrap();
        let (uid, validity) = granted.expect("APPENDUID granted");
        assert_eq!((uid.get(), validity.get()), (7, 3));
    }

    #[test]
    fn append_without_uidplus_still_succeeds_without_a_uid() {
        let granted = append_task(vec![Flag::Draft])
            .process_tagged(tagged_ok(None))
            .unwrap();
        assert!(granted.is_none());
    }

    #[test]
    fn xoauth2_task_waits_for_the_challenge_without_ir() {
        let task = AuthenticateTask::xoauth2(
            "quin@example.com",
            "token-1",
            false,
        );
        let (mechanism, line) = body_of(&task);
        assert_eq!(mechanism, AuthMechanism::XOAuth2);
        assert!(line.is_none());
    }
}
