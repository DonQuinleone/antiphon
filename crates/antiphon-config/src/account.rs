use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountFile {
    pub account: Account,
    pub imap: Imap,
    pub smtp: Option<Smtp>,
    #[serde(default, rename = "identity")]
    pub identities: Vec<Identity>,
    #[serde(default, rename = "rules")]
    pub rules: Vec<Rule>,
    pub oauth: Option<Oauth>,
    pub graph: Option<Graph>,
    #[serde(default)]
    pub folder_names: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub folder_order: Vec<String>,
    #[serde(default)]
    pub folders_hidden: Vec<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub maildir: Option<String>,
    pub archive: Option<String>,
    pub trash: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Imap {
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub password_cmd: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Smtp {
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password_cmd: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub address: String,
    pub name: Option<String>,
    pub signature: Option<String>,
    #[serde(default, rename = "match")]
    pub matches: Vec<String>,
    #[serde(default)]
    pub pgp_sign: bool,
    pub pgp_key: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub match_list: Option<String>,
    pub match_sender: Option<String>,
    pub move_to: Option<String>,
    pub tag: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oauth {
    pub provider: OauthProvider,
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OauthProvider {
    Google,
    Microsoft,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    #[serde(default)]
    pub send: bool,
}
