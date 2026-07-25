{
  description = "A modern mail client for the terminal";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";

  outputs = { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll = f:
        nixpkgs.lib.genAttrs systems
          (system: f nixpkgs.legacyPackages.${system});
      versionOf = self:
        if self ? shortRev
        then "0-unstable-${self.shortRev}"
        else "0-unstable-dirty";
    in
    {
      packages = forAll (pkgs: rec {
        antiphon = pkgs.rustPlatform.buildRustPackage {
          pname = "antiphon";
          version = versionOf self;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.scdoc
            pkgs.installShellFiles
          ];
          buildInputs = [ pkgs.notmuch ];
          env.ANTIPHON_VERSION = "v${versionOf self}";
          postInstall = ''
            for page in antiphon antiphond antiphon-sendmail; do
              scdoc <doc/$page.1.scd >$page.1
              installManPage $page.1
            done
            install -Dm644 dist/systemd/antiphond.service \
              $out/lib/systemd/user/antiphond.service
            substituteInPlace \
              $out/lib/systemd/user/antiphond.service \
              --replace-fail "%h/.cargo/bin/antiphond" \
              "$out/bin/antiphond"
          '';
          nativeCheckInputs = [ pkgs.git ];
          meta = {
            description =
              "A modern mail client for the terminal";
            homepage =
              "https://git.sr.ht/~donquinleone/antiphon";
            license = nixpkgs.lib.licenses.gpl3Plus;
            mainProgram = "antiphon";
          };
        };
        default = antiphon;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            pkg-config
            notmuch
            scdoc
            shellcheck
          ];
        };
      });
    };
}
