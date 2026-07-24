# Running antiphond

antiphond is an ordinary foreground process: it does not fork,
it logs to stdout/stderr, and it shuts down cleanly (sealing
the vault) on SIGHUP, SIGINT or SIGTERM. That makes it a
first-class citizen under any supervisor, and trivially
scriptable everywhere else. Ready-made service files live in
[dist/](../dist/).

The units below assume `antiphond` on your PATH or at the path
named in the file; adjust the path if you installed elsewhere
(`cargo install` puts it in `~/.cargo/bin`).

## Zero setup: let the client start it

None of this is required. When `antiphon` finds no daemon it
starts one itself (in its own process group, logging to
`$XDG_STATE_HOME/antiphon/antiphond.log`), and that daemon
outlives the terminal. Disable with:

```toml
[daemon]
autostart = false
```

A supervisor is still the better home for a daemon you want
running before the first client launch, restarted on failure,
and stopped at logout; the rest of this document covers that.

## macOS (launchd)

```bash
cp dist/launchd/org.antiphon.antiphond.plist ~/Library/LaunchAgents/
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/org.antiphon.antiphond.plist
```

Stop with `launchctl bootout gui/$(id -u)/org.antiphon.antiphond`.
Edit the `ProgramArguments` path if antiphond is not at
`/usr/local/bin/antiphond` (Homebrew on Apple silicon installs
under `/opt/homebrew/bin`, cargo under `~/.cargo/bin`).

## systemd (GNOME, KDE and most distributions)

```bash
mkdir -p ~/.config/systemd/user
cp dist/systemd/antiphond.service ~/.config/systemd/user/
systemctl --user enable --now antiphond
```

Logs land in `journalctl --user -u antiphond`. GNOME and KDE
both run a systemd user session, so this is the native route
there; it also survives logout when lingering is enabled
(`loginctl enable-linger`).

## dinit (Artix, Chimera)

```bash
mkdir -p ~/.config/dinit.d
cp dist/dinit/antiphond ~/.config/dinit.d/
dinitctl --user enable antiphond
```

Requires a user dinit instance (Artix and Chimera start one
for login sessions; elsewhere run `dinit` from your session
startup).

## runit (Void)

```bash
mkdir -p ~/sv
cp -r dist/runit/antiphond ~/sv/
```

Point a user-level `runsvdir` at `~/sv`. On Void the packaged
pattern is a system service supervising your user directory
(see the Void handbook on per-user services); anywhere else,
`runsvdir ~/sv` from your session startup does the same.
Control with `SVDIR=~/sv sv start antiphond` and friends.

## OpenRC, s6 and everything else

OpenRC's user services (0.56+) and s6 both supervise a plain
foreground command; point them at `antiphond` the same way.
On any setup without a user supervisor, fall back to one of
the session hooks below or to the client's autostart.

## Desktop autostart (KDE, XFCE, LXQt, GNOME)

Every XDG desktop honours the autostart directory:

```bash
mkdir -p ~/.config/autostart
cp dist/autostart/antiphond.desktop ~/.config/autostart/
```

This starts antiphond at login but does not restart it on
failure; prefer the systemd/dinit/runit routes where you have
them.

## Minimal sessions

- X11 (`startx`/xinit): add `antiphond &` to `~/.xinitrc`
  before the window manager line.
- sway: `exec antiphond` in `~/.config/sway/config`.
- Hyprland: `exec-once = antiphond` in `hyprland.conf`.
- river: `riverctl spawn antiphond` in `~/.config/river/init`.
- labwc: `antiphond &` in `~/.config/labwc/autostart`.

## Stopping

Any of SIGHUP, SIGINT or SIGTERM stops antiphond gracefully:
it finishes the loop iteration, seals the vault, and exits.
Supervisors' default stop signals are all fine. `antiphon
doctor` shows whether a daemon is reachable and how many
operations it is holding.
