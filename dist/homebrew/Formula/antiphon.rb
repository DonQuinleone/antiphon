class Antiphon < Formula
  desc "Modern mail client for the terminal"
  homepage "https://git.sr.ht/~donquinleone/antiphon"
  url "https://git.sr.ht/~donquinleone/antiphon/archive/v1.3.0.tar.gz"
  sha256 "952e9ed4a0358062581ca6ea88171a3218ac3e090a8b3af830ec94e14f32c281"
  license "GPL-3.0-or-later"

  depends_on "rust" => :build
  depends_on "scdoc" => :build
  depends_on "notmuch"

  def install
    ENV["ANTIPHON_VERSION"] = "v#{version}"
    system "cargo", "build", "--release", "--workspace", "--locked"
    bin.install "target/release/antiphon"
    bin.install "target/release/antiphond"

    %w[antiphon antiphond antiphon-sendmail].each do |name|
      man1.install scdoc("doc/#{name}.1.scd") => "#{name}.1"
    end
  end

  # scdoc(1) is a stdin-to-stdout filter with no output flag, so
  # render it here rather than shelling out through a subshell.
  def scdoc(source)
    rendered = buildpath/"#{File.basename(source, ".scd")}.roff"
    rendered.write(
      IO.popen("scdoc", "r+") do |io|
        io.write((buildpath/source).read)
        io.close_write
        io.read
      end,
    )
    rendered
  end

  def caveats
    <<~EOS
      OpenPGP signing and decryption go through gpg-agent. Install
      gnupg yourself if you sign or encrypt mail:

        brew install gnupg
    EOS
  end

  test do
    assert_match "antiphon", shell_output("#{bin}/antiphon --version")
  end
end
