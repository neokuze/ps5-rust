#!/usr/bin/env bash
# Sets up a Linux / WSL machine to build ps5-rust payloads:
#   1. system packages: clang + lld + llvm (the SDK's compiler and tools),
#      libclang (bindgen), socat (prospero-deploy), make, wget, unzip
#   2. Rust via rustup, with the nightly + rust-src toolchain the repo pins
#   3. the PS5 Payload SDK (latest release) in /opt/ps5-payload-sdk
#   4. PS5_PAYLOAD_SDK and PS5_HOST in your shell's startup file
#
# Usage: scripts/setup.sh [--yes] [--update-sdk]
#   --yes         don't ask; keep the detected shell and skip the PS5 address
#   --update-sdk  reinstall the SDK even if it's already there
# Asks before each sudo step. Safe to run again: it skips what's done and
# replaces its own block in the startup file instead of adding another.
set -euo pipefail

SDK_DIR=/opt/ps5-payload-sdk
SDK_URL=https://github.com/ps5-payload-dev/sdk/releases/latest/download/ps5-payload-sdk.zip
MARK_BEGIN="# >>> ps5-rust >>>"
MARK_END="# <<< ps5-rust <<<"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

YES=0
UPDATE_SDK=0
for a in "$@"; do
  case "$a" in
    --yes|-y) YES=1 ;;
    --update-sdk) UPDATE_SDK=1 ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

say()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ask "question" default(y/n) -> 0 for yes
ask() {
  local q="$1" def="$2" r
  if [ "$YES" = 1 ]; then [ "$def" = y ]; return; fi
  read -r -p "$q [$( [ "$def" = y ] && echo Y/n || echo y/N )] " r || true
  r="${r:-$def}"
  [[ "$r" =~ ^[YySs] ]]
}

# --- 1. system packages ------------------------------------------------------
say "System packages"
if command -v pacman >/dev/null; then
  PKGS=(bash clang lld llvm socat make wget unzip curl)
  INSTALL=(sudo pacman -S --needed --noconfirm)
elif command -v apt-get >/dev/null; then
  PKGS=(bash clang lld llvm libclang-dev socat make wget unzip curl)
  INSTALL=(sudo apt-get install -y)
elif command -v dnf >/dev/null; then
  PKGS=(bash clang clang-devel lld llvm llvm-devel socat make wget unzip curl)
  INSTALL=(sudo dnf install -y)
else
  PKGS=()
  warn "unknown package manager; install clang, lld, llvm, libclang, socat, make, wget and unzip yourself"
fi
if [ ${#PKGS[@]} -gt 0 ]; then
  missing=()
  for c in clang ld.lld llvm-objcopy socat make wget unzip curl; do
    command -v "$c" >/dev/null || missing+=("$c")
  done
  if [ ${#missing[@]} -eq 0 ]; then
    echo "already installed"
  elif ask "Missing: ${missing[*]}. Run '${INSTALL[*]} ${PKGS[*]}'?" y; then
    [ "${INSTALL[1]}" = apt-get ] && sudo apt-get update
    "${INSTALL[@]}" "${PKGS[@]}"
  else
    warn "skipped; builds need: ${missing[*]}"
  fi
fi

# --- 2. Rust -----------------------------------------------------------------
say "Rust"
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
if ! command -v rustup >/dev/null && [ -x "$CARGO_BIN/rustup" ]; then
  export PATH="$CARGO_BIN:$PATH"
fi
if ! command -v rustup >/dev/null; then
  if ask "rustup not found. Install it from https://rustup.rs (for your user, no sudo)?" y; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
    export PATH="$CARGO_BIN:$PATH"
  else
    die "Rust is required"
  fi
fi
# rust-toolchain.toml pins nightly + rust-src; this installs it now instead
# of on the first build.
# A rustup outside ~/.cargo (e.g. ~/.cargo-linux) needs CARGO_HOME to find
# itself; skip its self-update check either way.
export CARGO_HOME="${CARGO_HOME:-$(dirname "$(dirname "$(command -v rustup)")")}"
(cd "$REPO" && rustup toolchain install nightly --component rust-src --profile minimal --no-self-update >/dev/null && rustc --version)

# --- 3. PS5 Payload SDK ------------------------------------------------------
say "PS5 Payload SDK"
if [ -x "$SDK_DIR/bin/prospero-clang" ] && [ "$UPDATE_SDK" = 0 ]; then
  echo "already in $SDK_DIR (--update-sdk to reinstall)"
elif ask "Download the latest SDK release and unzip it to $SDK_DIR (sudo)?" y; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  wget -q --show-progress -O "$tmp/sdk.zip" "$SDK_URL"
  unzip -q "$tmp/sdk.zip" -d "$tmp"
  [ -x "$tmp/ps5-payload-sdk/bin/prospero-clang" ] || die "unexpected zip layout (no ps5-payload-sdk/bin/prospero-clang)"
  sudo rm -rf "$SDK_DIR"
  sudo mv "$tmp/ps5-payload-sdk" "$SDK_DIR"
  echo "installed in $SDK_DIR"
else
  warn "skipped; set PS5_PAYLOAD_SDK to your SDK's directory"
fi

# --- 4. environment ----------------------------------------------------------
say "Environment"
shell_name="$(basename "${SHELL:-bash}")"
if [ "$YES" = 0 ]; then
  read -r -p "Shell to configure (bash/zsh/fish) [$shell_name]: " s || true
  shell_name="${s:-$shell_name}"
fi
case "$shell_name" in
  bash) rc="$HOME/.bashrc" ;;
  zsh)  rc="${ZDOTDIR:-$HOME}/.zshrc" ;;
  fish) rc="$HOME/.config/fish/conf.d/ps5-rust.fish" ;;
  *) die "unsupported shell '$shell_name'; set PS5_PAYLOAD_SDK=$SDK_DIR and PS5_HOST yourself" ;;
esac

host=""
if [ "$YES" = 0 ]; then
  read -r -p "PS5 IP address for 'make test' / 'make std-test' (empty to skip): " host || true
fi

if [ "$shell_name" = fish ]; then
  block="$MARK_BEGIN
set -gx PS5_PAYLOAD_SDK $SDK_DIR${host:+
set -gx PS5_HOST $host}
$MARK_END"
else
  block="$MARK_BEGIN
export PS5_PAYLOAD_SDK=$SDK_DIR${host:+
export PS5_HOST=$host}
$MARK_END"
fi

mkdir -p "$(dirname "$rc")"
touch "$rc"
# Other PS5_PAYLOAD_SDK settings would override or fight with ours.
others="$(sed "/^$MARK_BEGIN\$/,/^$MARK_END\$/d" "$rc" | grep -nE '^[[:space:]]*[^#[:space:]].*PS5_PAYLOAD_SDK' || true)"
if [ -n "$others" ]; then
  warn "$rc already sets PS5_PAYLOAD_SDK elsewhere:"
  echo "$others" | sed 's/^/    /'
  if ask "Comment those lines out?" y; then
    cp "$rc" "$rc.ps5-rust.bak"
    sed -i "/^$MARK_BEGIN\$/,/^$MARK_END\$/!{/^[[:space:]]*#/!{/PS5_PAYLOAD_SDK/s/^/# (ps5-rust setup) /}}" "$rc"
    echo "backup: $rc.ps5-rust.bak"
  fi
fi
# Replace our previous block, if any, then append the new one.
sed -i "/^$MARK_BEGIN\$/,/^$MARK_END\$/d" "$rc"
[ -z "$(tail -n1 "$rc")" ] || echo >> "$rc"
printf '%s\n' "$block" >> "$rc"
echo "wrote to $rc:"
echo "$block" | sed 's/^/    /'

say "Done. Open a new terminal (or: source $rc), then:"
echo "    make std                                # build the std examples"
echo "    make std-test STD_EXAMPLE=hello_std     # build + send one to the PS5"
[ -n "$host" ] || echo "    (add PS5_HOST=<ps5-ip> to the make line, or run this script again to save it)"
