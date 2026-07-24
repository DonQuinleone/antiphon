# OpenPGP

Antiphon does OpenPGP with Sequoia end to end. Verification is
pure Rust inside the client; private-key operations (signing,
decryption) are delegated to your running gpg-agent over its
own protocol, so your keyring, smartcard and pinentry work
exactly as they do everywhere else. Antiphon never reads
secret key material.

## Verifying

Verification trusts a directory of certificates you curate:

    $XDG_CONFIG_HOME/antiphon/pgp/

Drop `.asc` or `.pgp` exports there (`gpg --armor --export
alice@example.com > ~/.config/antiphon/pgp/alice.asc`). The
pager then shows one of:

- `Good signature from Alice <alice@example.com> (0xKEYID)`,
  when the signature verifies against a cert in the directory.
- `Unknown signature from key 0xKEYID (not in keyring)`, when
  the message is signed by a key you have not added; Antiphon
  claims neither good nor bad.
- `BAD signature (0xKEYID)`, when a key you trust matches the
  signer and the content does not check out.

No line means the message is unsigned. Both PGP/MIME
(RFC 3156) and inline cleartext signatures are handled. There
is no web-of-trust or TOFU automation in v1: a cert is trusted
because you put it in the directory, nothing else.

## Signing

Signing is off by default and enabled per identity:

```toml
[[identity]]
address = "you@example.com"
pgp_sign = true
# optional; otherwise the key is matched by address
pgp_key = "8F0EA48BF8BE9D3B9E1B2B9C6E5F0D3A1C2B4D5E"
```

Before composing, `:sign`, `:nosign`, `:encrypt` and
`:noencrypt` override that default for the next message only,
and the compose statusline shows the live plan (`[sign]`,
`[sign+encrypt]`). Sealing happens after the editor closes,
through gpg-agent, so pinentry or a smartcard touch appears
exactly where gpg would put it. The signer is the identity's
`pgp_key` fingerprint or, absent that, the agent signing key
whose user ID carries the identity address.

If the signer is unknown, the agent refuses, or anything else
goes wrong, nothing is sent: the message stays in
`store/drafts/` and the statusline names the actual problem.
Plaintext is never sent silently where protection was asked
for.

## Encrypting

`:encrypt` requires a cert in the keyring directory for every
To and Cc address; missing ones are named and the send aborts
to a draft. Encryption signs inside (RFC 3156 layering), so
the recipient sees both protection and provenance. Received
`multipart/encrypted` mail is decrypted through gpg-agent when
opened, rendered as usual, and any signature inside is
verified against the keyring; a decryption failure shows the
error in place of the body.

## Checking the setup

`antiphon doctor` reports the keyring (how many certs are
trusted) and whether gpg-agent is reachable, with how many
signing keys it holds.

## Deferred

Smartcard-specific UX (touch prompts in the statusline) and
key discovery (WKD, keyservers) are named in the design and
land after v1's core flow is proven.
