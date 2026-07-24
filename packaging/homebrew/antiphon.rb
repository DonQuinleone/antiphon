# @VERSION@ and @SHA256@ are filled by the release pipeline;
# see packaging/README.md.
class Antiphon < Formula
  desc "Modern mail client for the terminal"
  homepage "https://github.com/DonQuinleone/antiphon"
  url "https://github.com/DonQuinleone/antiphon/archive/v@VERSION@.tar.gz"
  sha256 "@SHA256@"
  license "GPL-3.0-or-later"

  depends_on "rust" => :build
  depends_on "gnupg"
  depends_on "notmuch"

  def install
    system "cargo", "build", "--release", "--workspace",
           "--locked"
    bin.install "target/release/antiphon"
    bin.install "target/release/antiphond"
  end

  test do
    assert_match "antiphon", shell_output("#{bin}/antiphon --version")
  end
end
