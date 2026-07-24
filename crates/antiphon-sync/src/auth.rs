#[derive(Clone, Debug)]
pub enum Auth {
    Password(String),
    XOauth2 { user: String, access_token: String },
}
