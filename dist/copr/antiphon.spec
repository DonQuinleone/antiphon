# A plain cargo release build carries no debug info, so an automatic
# -debuginfo subpackage would be empty and abort the build.
%global debug_package %{nil}

Name:           antiphon
Version:        1.1.1
Release:        1%{?dist}
Summary:        Modern mail client for the terminal

License:        GPL-3.0-or-later
URL:            https://git.sr.ht/~donquinleone/antiphon
Source0:        %{url}/archive/v%{version}.tar.gz

# The workspace links against the system notmuch library, so the
# headers are needed to build and the shared library at runtime.
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  scdoc
BuildRequires:  notmuch-devel
BuildRequires:  systemd-rpm-macros
# hidapi (FIDO2 vault unlock) compiles its C hidraw backend and
# links libudev; this pulls both pkg-config and systemd-devel.
BuildRequires:  pkgconfig(libudev)

Requires:       notmuch

# OpenPGP signing and decryption go through gpg-agent, but they are
# opt-in per identity, so gnupg is only a weak dependency.
Recommends:     gnupg2

%description
Antiphon is a mail client for people who live in the terminal and
refuse to choose between speed, security and civilised e-mail. Mail
lives in a local Maildir indexed by notmuch, and a separate daemon
(antiphond) handles all network traffic so the interface never
stutters.

%prep
%autosetup -n %{name}-v%{version}

%build
# A third-party application repo cannot mirror every pinned crate as
# its own Fedora RPM, so this builds straight from Cargo.lock the way
# the AUR and Homebrew packages do. Copr's buildroot allows network
# access, so cargo fetches the locked dependencies from crates.io.
export ANTIPHON_VERSION="v%{version}"
cargo build --release --workspace --locked

for page in antiphon antiphond antiphon-sendmail; do
    scdoc <doc/${page}.1.scd >${page}.1
done

%install
install -Dm0755 target/release/antiphon \
    %{buildroot}%{_bindir}/antiphon
install -Dm0755 target/release/antiphond \
    %{buildroot}%{_bindir}/antiphond

for page in antiphon antiphond antiphon-sendmail; do
    install -Dm0644 ${page}.1 \
        %{buildroot}%{_mandir}/man1/${page}.1
done

install -Dm0644 dist/systemd/antiphond.service \
    %{buildroot}%{_userunitdir}/antiphond.service
# The shipped unit targets a cargo install; repoint it at the
# packaged binary.
sed -i 's|%%h/\.cargo/bin/antiphond|%{_bindir}/antiphond|' \
    %{buildroot}%{_userunitdir}/antiphond.service

%files
%license LICENSE
%doc README.md
%{_bindir}/antiphon
%{_bindir}/antiphond
%{_mandir}/man1/antiphon.1*
%{_mandir}/man1/antiphond.1*
%{_mandir}/man1/antiphon-sendmail.1*
%{_userunitdir}/antiphond.service

%changelog
* Tue Jul 28 2026 DonQuinleone <don@donquinleone.com> - 1.1.1-1
- Initial Copr package.
