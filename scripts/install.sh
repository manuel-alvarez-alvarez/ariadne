#!/usr/bin/env bash
# Ariadne installer: installs the binaries, registers the daemon as a user
# service (launchd on macOS, systemd --user on Linux), installs bash/zsh
# completions, installs the "Ariadne Desktop" app - registering it with
# GNOME on Linux - and has the user trust Ariadne's Codex hooks.
#
# The binaries and the app come from a GitHub release by default, and from a
# local build with --build-from-source. Release assets are unsigned; what they
# carry is a build provenance attestation, so every downloaded file is checked
# with `gh attestation verify` before anything is installed - which makes the
# GitHub CLI a hard requirement of the default flow - and the macOS quarantine
# attribute is cleared from what we install.
#
# Idempotent: safe to re-run after upgrades or config changes; every step
# replaces what a previous run installed. What was installed where is
# recorded in ~/.ariadne/install.env, which uninstall.sh reads.
#
# Output is a numbered step list; noisy subcommands (cargo, npm, launchctl,
# systemctl) go to ~/.ariadne/install.log and are only shown when a step fails.
#
# The options are in usage() below, which is what --help prints; they are not
# repeated here so the two cannot drift apart.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib.sh
. "$REPO_DIR/scripts/lib.sh"

usage() {
    cat <<'EOF'
Ariadne installer - installs ariadne + ariadned from a verified GitHub release.

Usage: scripts/install.sh [options]

  --build-from-source, --build
                     compile with cargo/npm instead of downloading a release
  --version vX.Y.Z   release to install (default: the latest one); download
                     mode only
  --prefix DIR       install binaries into DIR (default: ~/.local/bin)
  --no-service       skip daemon service registration (launchd / systemd --user)
  --no-completions   skip shell completion installation
  --no-codex-hooks   skip the Codex hook trust prompt
  --no-ui            skip installing the "Ariadne Desktop" app
  --verbose          stream subcommand output instead of capturing it
  --quiet            print errors and the final summary only
  --dry-run          print the steps that would run, change nothing
  --yes, -y          non-interactive: skip anything that would ask
  --help, -h         show this help

Downloaded assets are verified with `gh attestation verify` against the
release repository (the checkout's origin remote), so the GitHub CLI must be
installed and logged in; --build-from-source needs neither.

Environment: PREFIX, ARIADNE_HOME, NO_COLOR.
EOF
}

PREFIX="${PREFIX:-$HOME/.local/bin}"
BUILD_FROM_SOURCE=0
RELEASE_TAG=""
WITH_SERVICE=1
WITH_COMPLETIONS=1
WITH_CODEX_HOOKS=1
WITH_UI=1
while [ $# -gt 0 ]; do
    if ui_common_flag "$1"; then shift; continue; fi
    case "$1" in
        --build-from-source|--build) BUILD_FROM_SOURCE=1; shift ;;
        --version) RELEASE_TAG="$2"; shift 2 ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        --no-service) WITH_SERVICE=0; shift ;;
        --no-completions) WITH_COMPLETIONS=0; shift ;;
        --no-codex-hooks) WITH_CODEX_HOOKS=0; shift ;;
        --no-ui) WITH_UI=0; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; echo >&2; usage >&2; exit 2 ;;
    esac
done
if [ "$BUILD_FROM_SOURCE" = 1 ] && [ -n "$RELEASE_TAG" ]; then
    echo "--version names a published release; it cannot be combined with --build-from-source" >&2
    exit 2
fi
# Releases are tagged v<semver>; take the bare version too.
case "$RELEASE_TAG" in
    [0-9]*) RELEASE_TAG="v$RELEASE_TAG" ;;
esac
ui_init
trap 'ui_on_err $?' ERR

# Where everything goes, and the names the manifest records them under.
ui_locations
LOG_FILE="$ARIADNE_HOME/install.log"
APP_NAME="Ariadne Desktop"
APP_SRC_DIR="$REPO_DIR/ui"
APP_TARGET_DIR="$APP_SRC_DIR/src-tauri/target/release"

# Download-mode state; all empty when building from source.
TARGET=""          # the release target triple this machine runs
BIN_ASSET=""       # the tarball with ariadne + ariadned
APP_ASSET=""       # the desktop bundle (.app.tar.gz on macOS, .AppImage on Linux)
RELEASE_REPO=""    # owner/repo the release and its attestations come from
RESOLVED_TAG=""    # the tag actually installed, once gh has told us
STAGE_DIR=""       # scratch directory the assets are downloaded into

# Only a label for the step title and the summary; an OS with no service of
# ours still gets everything before that step, and fails inside it.
SERVICE_DESC=""
if [ "$WITH_SERVICE" = 1 ]; then
    SERVICE_DESC="$(ui_service_desc "unsupported on $OS")"
fi

# npm, run from ui/: the Tauri CLI resolves src-tauri/ relative to the cwd.
app_npm() {
    ( cd "$APP_SRC_DIR" && run_logged npm "$@" )
}

# --- release downloads ---------------------------------------------------------

# The target triple naming the release assets for this machine. Only the four
# triples the release workflow builds exist; anything else has to be compiled.
detect_target() {
    case "$OS/$(uname -m)" in
        Darwin/arm64|Darwin/aarch64) TARGET="aarch64-apple-darwin" ;;
        Darwin/x86_64) TARGET="x86_64-apple-darwin" ;;
        Linux/x86_64|Linux/amd64) TARGET="x86_64-unknown-linux-gnu" ;;
        Linux/aarch64|Linux/arm64) TARGET="aarch64-unknown-linux-gnu" ;;
        *) ui_die "no release is published for $OS $(uname -m) - re-run with --build-from-source" ;;
    esac
}

# gh is not a convenience here: it is what verifies the attestations, and an
# unverified install is not on offer.
require_gh() {
    command -v gh > /dev/null 2>&1 \
        || ui_die "the GitHub CLI (gh) is required to install a release - install it from https://cli.github.com, or re-run with --build-from-source"
    run_logged gh auth status \
        || ui_die "gh is not logged in - run 'gh auth login', or re-run with --build-from-source"
}

# owner/repo for `gh release download` and `gh attestation verify`, read off
# the checkout the installer was run from. Handles the https, ssh and scp-like
# forms of a remote URL.
resolve_release_repo() {
    local url owner name
    url="$(git -C "$REPO_DIR" remote get-url origin 2>/dev/null)" || url=""
    [ -n "$url" ] \
        || ui_die "no 'origin' remote in $(ui_tilde "$REPO_DIR") to take the release repository from - re-run with --build-from-source"
    url="${url%.git}"
    url="${url%/}"
    name="${url##*/}"
    owner="${url%/*}"
    owner="${owner##*[:/]}"
    [ -n "$owner" ] && [ -n "$name" ] && [ "$owner" != "$url" ] \
        || ui_die "could not read owner/repo out of the origin remote ($url) - re-run with --build-from-source"
    RELEASE_REPO="$owner/$name"
}

# Release assets are unsigned and arrive over the network, so macOS parks a
# com.apple.quarantine attribute on them and Gatekeeper then refuses to run
# what we installed. Nothing to clear elsewhere, or when it is already absent.
clear_quarantine() {
    [ "$OS" = Darwin ] || return 0
    local path
    for path in "$@"; do
        xattr -r -d com.apple.quarantine "$path" > /dev/null 2>&1 || true
    done
    return 0
}

# Put a built or downloaded desktop bundle in place, setting APP_PATH to where
# it landed: the .app in /Applications on macOS, the AppImage (or plain binary)
# as $PREFIX/ariadne-desktop on Linux.
install_app_bundle() {
    local bundle="$1"
    case "$OS" in
        Darwin)
            # /Applications is writable by admin users; anyone else gets
            # the per-user one, which Finder and Spotlight treat the same.
            APP_INSTALL_DIR="/Applications"
            [ -w "$APP_INSTALL_DIR" ] || APP_INSTALL_DIR="$HOME/Applications"
            mkdir -p "$APP_INSTALL_DIR"
            APP_PATH="$APP_INSTALL_DIR/$APP_NAME.app"
            rm -rf "$APP_PATH"
            # ditto, not cp: it is what copies a bundle whole, extended
            # attributes and code signature included.
            ditto "$bundle" "$APP_PATH"
            ;;
        Linux)
            mkdir -p "$PREFIX"
            APP_PATH="$PREFIX/ariadne-desktop"
            install -m 755 "$bundle" "$APP_PATH"
            ;;
        *)
            ui_die "unsupported OS for the desktop app: $OS (use --no-ui)"
            ;;
    esac
}

# Extracts the icon bundled in an AppImage into $2. --appimage-extract needs
# no FUSE, unlike running the AppImage itself. Best-effort: returns 1 and
# copies nothing if extraction fails or no icon is found inside.
extract_appimage_icon() {
    local appimage="$1" dest="$2" extract_root icon
    extract_root="$(mktemp -d "${TMPDIR:-/tmp}/ariadne-appimage-icon.XXXXXX")"
    if ! ( cd "$extract_root" && run_logged "$appimage" --appimage-extract ); then
        rm -rf "$extract_root"
        return 1
    fi
    # A missing squashfs-root (an AppImage runtime that behaves unexpectedly)
    # would make `find` fail; every step here is best-effort, so none of it
    # is allowed to take the installer down with it.
    icon="$(find "$extract_root/squashfs-root" -maxdepth 1 -name '*.png' -print -quit 2>/dev/null)" || true
    if [ -z "$icon" ]; then
        icon="$(find "$extract_root/squashfs-root/usr/share/icons" -name '*.png' -print -quit 2>/dev/null)" || true
    fi
    [ -n "$icon" ] && { cp "$icon" "$dest" || true; }
    rm -rf "$extract_root"
    [ -n "$icon" ] && [ -f "$dest" ]
}

# The "<width>x<height>" a PNG's own header reports, for picking its hicolor
# theme subdirectory. Falls back to 256x256 when it cannot be read.
icon_size() {
    local dims
    # `file` missing entirely is as harmless as it not recognizing the image:
    # either way this falls back to the default size, not to a dead install.
    dims="$(file -b "$1" 2>/dev/null | sed -n 's/.*, \([0-9][0-9]*\) x \([0-9][0-9]*\).*/\1x\2/p')" || true
    printf '%s' "${dims:-256x256}"
}

# Registers "Ariadne Desktop" with GNOME (and anything else reading the
# freedesktop menu/icon specs): a .desktop entry pointing at $APP_PATH, plus
# $1 installed as its icon under the hicolor theme. $1 may be empty or
# missing - the entry still works, just with no icon. Sets ARIADNE_DESKTOP_ICON
# to where the icon landed, or "" if none was installed.
install_desktop_entry() {
    local icon_src="$1" size
    mkdir -p "$(dirname "$ARIADNE_DESKTOP_ENTRY")"
    cat > "$ARIADNE_DESKTOP_ENTRY" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=$APP_NAME
Comment=A docker-style orchestrator for AI coding agents
Exec="$APP_PATH" %U
Icon=$ARIADNE_DESKTOP_ID
Terminal=false
Categories=Development;
EOF

    ARIADNE_DESKTOP_ICON=""
    if [ -n "$icon_src" ] && [ -f "$icon_src" ]; then
        size="$(icon_size "$icon_src")"
        ARIADNE_DESKTOP_ICON="$ARIADNE_ICON_BASE/$size/apps/$ARIADNE_DESKTOP_ID.png"
        mkdir -p "$(dirname "$ARIADNE_DESKTOP_ICON")"
        cp "$icon_src" "$ARIADNE_DESKTOP_ICON"
    fi

    # Neither tool is required; GNOME picks new entries and icons up on its
    # own eventually, these just make it immediate. Missing or failing is
    # never a reason to fail the install.
    command -v update-desktop-database > /dev/null 2>&1 \
        && run_logged update-desktop-database "$(dirname "$ARIADNE_DESKTOP_ENTRY")" || true
    command -v gtk-update-icon-cache > /dev/null 2>&1 \
        && run_logged gtk-update-icon-cache -f -t "$ARIADNE_ICON_BASE" || true
}

# What is being installed and where it comes from, for the plan, the header
# and the summary. Deciding the target now keeps an unsupported machine from
# getting as far as a plan it could never carry out.
RELEASE_DESC=""
SOURCE_DESC="repo    $REPO_DIR"
if [ "$BUILD_FROM_SOURCE" = 0 ]; then
    detect_target
    BIN_ASSET="ariadne-$TARGET.tar.gz"
    case "$OS" in
        Darwin) APP_ASSET="ariadne-desktop-$TARGET.app.tar.gz" ;;
        *) APP_ASSET="ariadne-desktop-$TARGET.AppImage" ;;
    esac
    if [ -n "$RELEASE_TAG" ]; then
        RELEASE_DESC="release $RELEASE_TAG"
    else
        RELEASE_DESC="the latest release"
    fi
    SOURCE_DESC="release ${RELEASE_TAG:-latest} ($TARGET)"
fi

# --- the plan ------------------------------------------------------------------
# One plan_add per step, in execution order; the step count adapts to the flags.
if [ "$BUILD_FROM_SOURCE" = 1 ]; then
    plan_add "Building release binaries"
else
    plan_add "Downloading $RELEASE_DESC for $TARGET"
    plan_add "Verifying the build provenance (gh attestation verify)"
fi
plan_add "Stopping any running daemon"
plan_add "Installing binaries $UI_ARROW $(ui_tilde "$PREFIX")"
if [ "$WITH_UI" = 1 ]; then
    if [ "$BUILD_FROM_SOURCE" = 1 ]; then
        plan_add "Building and installing $APP_NAME"
    else
        plan_add "Installing $APP_NAME"
    fi
    [ "$OS" = Linux ] && plan_add "Registering $APP_NAME with GNOME"
fi
[ "$WITH_COMPLETIONS" = 1 ] && plan_add "Registering shell completions"
if [ "$WITH_SERVICE" = 1 ]; then
    plan_add "Registering the daemon service ($SERVICE_DESC)"
    plan_add "Waiting for the daemon"
fi
[ "$WITH_CODEX_HOOKS" = 1 ] && plan_add "Trusting Ariadne's Codex hooks"
plan_add "Writing the install manifest $UI_ARROW $(ui_tilde "$ARIADNE_MANIFEST")"
plan_add "Checking the installation (ariadne doctor)"
ui_start

ui_header "Ariadne installer" \
    "$SOURCE_DESC" \
    "prefix  $(ui_tilde "$PREFIX")" \
    "log     $(ui_tilde "$LOG_FILE")"

if [ "$UI_DRY_RUN" = 1 ]; then
    plan_print
    exit 0
fi

ui_log_init "$LOG_FILE"

# --- previous install (for cross-prefix idempotency) --------------------------
OLD_PREFIX=""
if [ -f "$ARIADNE_MANIFEST" ]; then
    # shellcheck disable=SC1090
    OLD_PREFIX="$(. "$ARIADNE_MANIFEST" && echo "${ARIADNE_PREFIX:-}")"
fi

# --- build, or download and verify ---------------------------------------------
# Where the binaries to install come from: the release build in this checkout,
# or the assets unpacked out of the staging directory.
BIN_SRC_DIR="$REPO_DIR/target/release"
if [ "$BUILD_FROM_SOURCE" = 1 ]; then
    step_begin
    run_logged cargo build --release --manifest-path "$REPO_DIR/Cargo.toml" \
        || ui_die "cargo build failed"
    step_ok
else
    step_begin
    require_gh
    resolve_release_repo
    # Ask which tag is being installed before downloading anything: with no
    # --version that is whatever gh calls the latest release, and the step
    # note, the summary and the error messages all want its name.
    if [ -n "$RELEASE_TAG" ]; then
        RESOLVED_TAG="$(gh release view "$RELEASE_TAG" --repo "$RELEASE_REPO" \
            --json tagName --jq .tagName 2>> "$LOG_FILE")" \
            || ui_die "no release $RELEASE_TAG in $RELEASE_REPO"
    else
        RESOLVED_TAG="$(gh release view --repo "$RELEASE_REPO" \
            --json tagName --jq .tagName 2>> "$LOG_FILE")" \
            || ui_die "$RELEASE_REPO has no published release yet - use --build-from-source"
    fi

    STAGE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ariadne-install.XXXXXX")"
    trap 'rm -rf "$STAGE_DIR"' EXIT
    BIN_SRC_DIR="$STAGE_DIR"
    run_logged gh release download "$RESOLVED_TAG" --repo "$RELEASE_REPO" \
        --dir "$STAGE_DIR" --pattern "$BIN_ASSET" \
        || ui_die "$RESOLVED_TAG has no $BIN_ASSET - use --build-from-source"
    if [ "$WITH_UI" = 1 ]; then
        run_logged gh release download "$RESOLVED_TAG" --repo "$RELEASE_REPO" \
            --dir "$STAGE_DIR" --pattern "$APP_ASSET" \
            || ui_die "$RESOLVED_TAG has no $APP_ASSET (--no-ui installs the CLI and daemon only)"
    fi
    step_ok "$RESOLVED_TAG"

    # Nothing downloaded is touched until GitHub has confirmed it was built by
    # the release workflow of this very repository, from this very tag.
    step_begin
    for _asset in "$STAGE_DIR"/*; do
        run_logged gh attestation verify "$_asset" --repo "$RELEASE_REPO" \
            || ui_die "$(basename "$_asset") failed attestation verification - nothing was installed"
    done
    step_ok "$RELEASE_REPO"
fi

# --- stop whatever is currently running ------------------------------------------
# The service stays registered: this only frees the binaries to be replaced.
step_begin
ui_stop_daemon
sleep 1
step_ok

# --- binaries --------------------------------------------------------------------
step_begin
if [ "$BUILD_FROM_SOURCE" = 0 ]; then
    # Both binaries sit at the root of the tarball.
    run_logged tar -xzf "$STAGE_DIR/$BIN_ASSET" -C "$STAGE_DIR" \
        || ui_die "could not unpack $BIN_ASSET"
    clear_quarantine "$STAGE_DIR/ariadne" "$STAGE_DIR/ariadned"
fi
mkdir -p "$PREFIX"
install -m 755 "$BIN_SRC_DIR/ariadne" "$PREFIX/ariadne"
install -m 755 "$BIN_SRC_DIR/ariadned" "$PREFIX/ariadned"
[ "$BUILD_FROM_SOURCE" = 1 ] || clear_quarantine "$PREFIX/ariadne" "$PREFIX/ariadned"
if [ -n "$OLD_PREFIX" ] && [ "$OLD_PREFIX" != "$PREFIX" ]; then
    rm -f "$OLD_PREFIX/ariadne" "$OLD_PREFIX/ariadned"
    step_ok "previous prefix $(ui_tilde "$OLD_PREFIX") cleaned"
else
    step_ok
fi

# --- desktop app --------------------------------------------------------------------
# Downloaded, the app is one more verified asset and installs like the
# binaries. Built, it is optional and best-effort: the Tauri app in ui/ needs
# a Node toolchain, and a machine without one still deserves a complete
# install. The Tauri CLI itself comes from ui/'s devDependencies, so npm is
# the only thing we ask for.
APP_PATH=""
APP_STATE="not installed (--no-ui)"
ARIADNE_DESKTOP_ICON=""
if [ "$WITH_UI" = 1 ] && [ "$BUILD_FROM_SOURCE" = 0 ]; then
    step_begin
    case "$OS" in
        Darwin)
            # --mac-metadata: the archive carries the bundle's extended
            # attributes and symlinks, and the .app needs them to stay loadable.
            run_logged tar --mac-metadata -xzf "$STAGE_DIR/$APP_ASSET" -C "$STAGE_DIR" \
                || ui_die "could not unpack $APP_ASSET"
            APP_BUNDLE="$STAGE_DIR/$APP_NAME.app"
            [ -d "$APP_BUNDLE" ] || ui_die "$APP_ASSET holds no $APP_NAME.app"
            ;;
        # Any other OS died in detect_target long before this; install_app_bundle
        # is the one place that refuses it, here and in the build branch below.
        *) APP_BUNDLE="$STAGE_DIR/$APP_ASSET" ;;
    esac
    install_app_bundle "$APP_BUNDLE"
    clear_quarantine "$APP_PATH"
    APP_STATE="$(ui_tilde "$APP_PATH")"
    step_ok "$(ui_tilde "$APP_PATH")"
elif [ "$WITH_UI" = 1 ]; then
    step_begin
    if ! command -v npm > /dev/null 2>&1; then
        APP_STATE="skipped - npm not found"
        step_skip "npm not found - skipping $APP_NAME; install Node and re-run"
    elif [ ! -f "$APP_SRC_DIR/package.json" ]; then
        APP_STATE="skipped - no ui/ in this checkout"
        step_skip "no $(ui_tilde "$APP_SRC_DIR/package.json") - skipping $APP_NAME"
    else
        # ci is reproducible but demands a lockfile in sync with package.json;
        # a stale or missing one is no reason to fail the whole install.
        app_npm ci || app_npm install \
            || ui_die "npm install in ui/ failed (--no-ui skips the app)"
        app_npm run tauri build \
            || ui_die "npm run tauri build failed (--no-ui skips the app)"

        case "$OS" in
            Darwin)
                APP_BUNDLE="$APP_TARGET_DIR/bundle/macos/$APP_NAME.app"
                [ -d "$APP_BUNDLE" ] || ui_die "the build produced no $APP_NAME.app"
                ;;
            Linux)
                # The AppImage when its tooling produced one; the plain
                # binary otherwise, named after the crate - or after
                # productName, depending on the Tauri version, so try both.
                APP_BUNDLE=""
                for _candidate in "$APP_TARGET_DIR/bundle/appimage/"*.AppImage \
                                  "$APP_TARGET_DIR/ariadne-ui" \
                                  "$APP_TARGET_DIR/$APP_NAME"; do
                    if [ -f "$_candidate" ]; then
                        APP_BUNDLE="$_candidate"
                        break
                    fi
                done
                [ -n "$APP_BUNDLE" ] || ui_die "the build produced no AppImage and no binary"
                ;;
            *) APP_BUNDLE="" ;;
        esac
        install_app_bundle "$APP_BUNDLE"
        APP_STATE="$(ui_tilde "$APP_PATH")"
        step_ok "$(ui_tilde "$APP_PATH")"
    fi
fi

# --- GNOME desktop entry (Linux only) ---------------------------------------------
GNOME_STATE=""
if [ "$WITH_UI" = 1 ] && [ "$OS" = Linux ]; then
    step_begin
    if [ -n "$APP_PATH" ]; then
        # Cross-run idempotency, the same way OLD_PREFIX works for the
        # binaries: an icon size that changed since the last run would
        # otherwise leave the old one behind alongside the new one.
        OLD_DESKTOP_ICON=""
        if [ -f "$ARIADNE_MANIFEST" ]; then
            # shellcheck disable=SC1090
            OLD_DESKTOP_ICON="$(. "$ARIADNE_MANIFEST" && echo "${ARIADNE_DESKTOP_ICON:-}")"
        fi

        ICON_TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ariadne-icon.XXXXXX")"
        ICON_SRC=""
        case "$APP_BUNDLE" in
            *.AppImage)
                # $APP_PATH, not $APP_BUNDLE: install_app_bundle already made
                # it executable (mode 755), which a freshly downloaded asset
                # is not guaranteed to be.
                extract_appimage_icon "$APP_PATH" "$ICON_TMP_DIR/icon.png" \
                    && ICON_SRC="$ICON_TMP_DIR/icon.png"
                ;;
        esac
        if [ -z "$ICON_SRC" ] && [ -f "$APP_SRC_DIR/src-tauri/icons/icon.png" ]; then
            ICON_SRC="$APP_SRC_DIR/src-tauri/icons/icon.png"
        fi

        install_desktop_entry "$ICON_SRC"
        rm -rf "$ICON_TMP_DIR"

        if [ -n "$OLD_DESKTOP_ICON" ] && [ "$OLD_DESKTOP_ICON" != "$ARIADNE_DESKTOP_ICON" ]; then
            rm -f "$OLD_DESKTOP_ICON"
        fi

        if [ -n "$ARIADNE_DESKTOP_ICON" ]; then
            GNOME_STATE="$(ui_tilde "$ARIADNE_DESKTOP_ENTRY")"
            step_ok "$GNOME_STATE"
        else
            GNOME_STATE="$(ui_tilde "$ARIADNE_DESKTOP_ENTRY") - no icon found"
            step_ok "no icon found"
        fi
    else
        GNOME_STATE="not registered - $APP_NAME was not installed"
        step_skip "$APP_NAME was not installed"
    fi
fi

# --- completions ------------------------------------------------------------------
COMPLETION_SHELLS=""
if [ "$WITH_COMPLETIONS" = 1 ]; then
    step_begin
    # Completions are dynamic: the shell sources a shim that calls back into
    # the ariadne binary on TAB, which queries the daemon for live candidates
    # (task/goal/session ids, profile names). Remove static files from older
    # installs so they cannot shadow the dynamic registration.
    rm -f "$ARIADNE_BASH_COMPLETION" "$ARIADNE_ZSH_COMPLETION"

    # The block itself is written by the binary being installed
    # (`ariadne completions install`), so the rc lines have one author and a
    # user can add or repair them the same way later. Only the shells that
    # already have a startup file here are registered, as before.
    if [ -f "$ARIADNE_BASHRC" ]; then
        run_logged "$PREFIX/ariadne" completions install --shell bash \
            && COMPLETION_SHELLS="bash"
    fi

    if [ -f "$ARIADNE_ZSHRC" ]; then
        run_logged "$PREFIX/ariadne" completions install --shell zsh \
            && COMPLETION_SHELLS="${COMPLETION_SHELLS:+$COMPLETION_SHELLS, }zsh"
    fi

    if [ -n "$COMPLETION_SHELLS" ]; then
        step_ok "$COMPLETION_SHELLS - new shells only"
    else
        step_skip "no ~/.bashrc or ~/.zshrc found"
    fi
fi

# --- daemon service -----------------------------------------------------------------
DAEMON_STATE="not started"
if [ "$WITH_SERVICE" = 1 ]; then
    step_begin
    mkdir -p "$ARIADNE_HOME"
    case "$OS" in
        Darwin)
            mkdir -p "$(dirname "$ARIADNE_PLIST")"
            cat > "$ARIADNE_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>$ARIADNE_PLIST_LABEL</string>
    <key>ProgramArguments</key>
    <array><string>$PREFIX/ariadned</string></array>
    <key>EnvironmentVariables</key>
    <dict>
        <!-- launchd services get a bare PATH; the daemon needs tmux, git and
             the agent CLIs, so bake in the installing user's PATH. -->
        <key>PATH</key><string>$PATH</string>
    </dict>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key>
    <dict><key>SuccessfulExit</key><false/></dict>
    <key>ThrottleInterval</key><integer>10</integer>
    <key>StandardOutPath</key><string>$ARIADNE_HOME/ariadned.log</string>
    <key>StandardErrorPath</key><string>$ARIADNE_HOME/ariadned.log</string>
</dict>
</plist>
EOF
            run_logged launchctl bootstrap "gui/$(id -u)" "$ARIADNE_PLIST" \
                || ui_die "launchctl bootstrap failed"
            ;;
        Linux)
            mkdir -p "$(dirname "$ARIADNE_UNIT")"
            cat > "$ARIADNE_UNIT" <<EOF
[Unit]
Description=Ariadne coding-agent orchestrator daemon

[Service]
ExecStart=$PREFIX/ariadned
# systemd user services also get a minimal PATH.
Environment="PATH=$PATH"
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
EOF
            run_logged systemctl --user daemon-reload \
                || ui_die "systemctl --user daemon-reload failed"
            run_logged systemctl --user enable ariadned.service || true
            run_logged systemctl --user restart ariadned.service \
                || ui_die "systemctl --user restart ariadned.service failed"
            ;;
        *)
            ui_die "unsupported OS for service setup: $OS (use --no-service and run ariadned yourself)"
            ;;
    esac
    step_ok

    step_begin
    for _ in $(seq 1 30); do
        if run_logged "$PREFIX/ariadne" daemon status; then
            DAEMON_STATE="running"
            break
        fi
        sleep 1
    done
    if [ "$DAEMON_STATE" = "running" ]; then
        step_ok
    else
        ui_die "no answer after 30s - see $(ui_tilde "$ARIADNE_HOME/ariadned.log")"
    fi
fi

# --- codex hooks -----------------------------------------------------------------------
# Codex carries its hooks per session, but only runs them once the user has
# trusted them — and it asks at the start of a session. The last step of the
# install therefore opens one, with the very flags the daemon will spawn with,
# so the user can answer. Nothing is written to ~/.codex by us.
CODEX_STATE="skipped"
if [ "$WITH_CODEX_HOOKS" = 1 ]; then
    step_begin
    if [ "$UI_YES" = 1 ]; then
        step_skip "--yes: run 'ariadne setup codex-hooks' when convenient"
    else
        run_interactive "$PREFIX/ariadne" setup codex-hooks --cli-bin "$PREFIX/ariadne" || true
        CODEX_STATE="prompted"
        step_ok
    fi
fi

# --- manifest (read by uninstall.sh) ---------------------------------------------------
step_begin
mkdir -p "$ARIADNE_HOME"
cat > "$ARIADNE_MANIFEST" <<EOF
# Written by scripts/install.sh — read by scripts/uninstall.sh.
ARIADNE_PREFIX="$PREFIX"
ARIADNE_BASH_COMPLETION="$ARIADNE_BASH_COMPLETION"
ARIADNE_ZSH_COMPLETION="$ARIADNE_ZSH_COMPLETION"
ARIADNE_PLIST="$ARIADNE_PLIST"
ARIADNE_UNIT="$ARIADNE_UNIT"
EOF
# Absent when the app was skipped; uninstall.sh then has nothing to remove.
[ -n "$APP_PATH" ] && printf 'ARIADNE_APP="%s"\n' "$APP_PATH" >> "$ARIADNE_MANIFEST"
# Absent unless this run registered the GNOME entry; the icon line is only
# added once one was actually found and installed.
if [ -n "$GNOME_STATE" ] && [ -n "$APP_PATH" ] && [ "$OS" = Linux ]; then
    printf 'ARIADNE_DESKTOP_ENTRY="%s"\n' "$ARIADNE_DESKTOP_ENTRY" >> "$ARIADNE_MANIFEST"
    [ -n "$ARIADNE_DESKTOP_ICON" ] \
        && printf 'ARIADNE_DESKTOP_ICON="%s"\n' "$ARIADNE_DESKTOP_ICON" >> "$ARIADNE_MANIFEST"
fi
step_ok

# --- checkup ----------------------------------------------------------------------------
# Everything is installed and the daemon has answered, so the install itself
# has succeeded; this is the report on what the finished machine looks like.
# doctor exits 1 when it finds something broken — a missing agent CLI, an
# unwritable directory — and none of that unmakes the install, so its verdict
# is shown and never allowed to fail the script. The binary is the one just
# installed, not whatever an older PATH entry answers to.
DOCTOR_STATE="ok"
step_begin
if [ "$UI_QUIET" = 1 ]; then
    # --quiet is errors and the summary only: the report goes to the log.
    run_logged "$PREFIX/ariadne" doctor || DOCTOR_STATE="reported problems"
else
    run_interactive "$PREFIX/ariadne" doctor || DOCTOR_STATE="reported problems"
fi
if [ "$DOCTOR_STATE" = "ok" ]; then
    step_ok
else
    step_ok "$DOCTOR_STATE - see the report above"
fi

# --- summary ----------------------------------------------------------------------------
printf '\n%sAriadne installed.%s\n\n' "$UI_B$UI_GREEN" "$UI_R"
if [ "$BUILD_FROM_SOURCE" = 1 ]; then
    ui_field "source" "built from $(ui_tilde "$REPO_DIR")"
else
    ui_field "source" "release $RESOLVED_TAG ($TARGET), attestation verified"
fi
ui_field "binaries" "$(ui_tilde "$PREFIX")/{ariadne,ariadned}"
if [ "$WITH_SERVICE" = 1 ]; then
    ui_field "service" "$SERVICE_DESC - daemon $DAEMON_STATE"
else
    ui_field "service" "not registered (--no-service) - run ariadned yourself"
fi
if [ "$WITH_COMPLETIONS" = 1 ]; then
    ui_field "completions" "${COMPLETION_SHELLS:-none - no ~/.bashrc or ~/.zshrc}"
else
    ui_field "completions" "not installed (--no-completions)"
fi
ui_field "desktop app" "$APP_STATE"
[ -n "$GNOME_STATE" ] && ui_field "gnome entry" "$GNOME_STATE"
[ "$WITH_CODEX_HOOKS" = 1 ] && ui_field "codex hooks" "$CODEX_STATE"
ui_field "checkup" "ariadne doctor - $DOCTOR_STATE"
ui_field "manifest" "$(ui_tilde "$ARIADNE_MANIFEST")"
ui_field "log" "$(ui_tilde "$LOG_FILE")"
printf '\n'

case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *) ui_warn "$PREFIX is not on your PATH - add: export PATH=\"$PREFIX:\$PATH\"" ;;
esac

if [ "$WITH_SERVICE" = 1 ]; then
    case "$OS" in
        Darwin) STOP_HINT="launchctl bootout gui/\$(id -u)/$ARIADNE_PLIST_LABEL" ;;
        Linux) STOP_HINT="systemctl --user stop ariadned" ;;
    esac
    printf '  %sthe daemon restarts on failure; stop it with: %s%s\n' \
        "$UI_D" "$STOP_HINT" "$UI_R"
fi
printf "  %sTry:%s %sariadne goal create --title '...' --repo /path/to/repo%s\n\n" \
    "$UI_D" "$UI_R" "$UI_CYAN" "$UI_R"
