#!/bin/bash
# VoxSplit __VERSION__ installer for macOS (Apple silicon, macOS 14+).
#
#   curl -fsSL https://voxsplit.triplejmusic.art/install.sh | bash
#       The app, the VST3/AU plugins and the `voxsplit` command (asks for your password).
#   curl -fsSL https://voxsplit.triplejmusic.art/install.sh | bash -s -- --cli
#       Only the `voxsplit` command, in your home folder (no password).
#   curl -fsSL https://voxsplit.triplejmusic.art/install.sh | bash -s -- --uninstall
#       Remove VoxSplit (your separated stems in ~/Music/VoxSplit are kept).
#
# Source: https://github.com/BadMask121/voxsplit

# Everything runs from main(), so a partly downloaded script does nothing.
main() {
  set -euo pipefail

  local version="__VERSION__"
  local url="__URL__"
  local sha256="__SHA256__"
  local size="__SIZE__"
  local mode=full

  for arg in "$@"; do
    case "$arg" in
      --cli) mode=cli ;;
      --uninstall) mode=uninstall ;;
      -h | --help)
        usage
        return 0
        ;;
      *) fail "unknown option: $arg (try --help)" ;;
    esac
  done

  if [[ "$mode" == uninstall ]]; then
    uninstall
    return 0
  fi

  check_mac

  local tmp
  tmp="$(mktemp -d)"
  # Expanded now: `tmp` is local to main and gone by the time the EXIT trap runs.
  trap "rm -rf '$tmp'" EXIT

  step "Downloading VoxSplit $version ($size)"
  curl -fL --progress-bar --retry 3 -o "$tmp/VoxSplit.pkg" "$url"
  local actual
  actual="$(shasum -a 256 "$tmp/VoxSplit.pkg" | cut -d' ' -f1)"
  [[ "$actual" == "$sha256" ]] || fail "the download is damaged (SHA-256 mismatch). Run the command again."
  ok "Verified (SHA-256 ${sha256:0:12}…)"

  if [[ "$mode" == cli ]]; then
    install_cli "$tmp"
  else
    install_all "$tmp"
  fi
}

install_all() {
  local tmp="$1"
  step "Installing the app, the plugins and the voxsplit command"
  note "macOS asks for your password to install into /Applications and /Library."
  sudo installer -pkg "$tmp/VoxSplit.pkg" -target / >/dev/null </dev/tty \
    || fail "the installer did not finish."
  ok "Installed"
  cat <<EOF

  ${BOLD}App${RESET}        open VoxSplit from Applications and drop a song on it
  ${BOLD}FL Studio${RESET}  Options → Manage plugins → Find more plugins, then put VoxSplit on a mixer insert
  ${BOLD}Terminal${RESET}   voxsplit separate song.mp3

  Setup help: https://voxsplit.triplejmusic.art/#setup
EOF
}

install_cli() {
  local tmp="$1"
  local dest="$HOME/Library/Application Support/VoxSplit"
  local bin="$HOME/.local/bin"
  step "Installing the voxsplit command"
  pkgutil --expand-full "$tmp/VoxSplit.pkg" "$tmp/expanded" >/dev/null
  local payload="$tmp/expanded/VoxSplit-component.pkg/Payload/Library/Application Support/VoxSplit"
  [[ -x "$payload/voxsplit-engine" ]] || fail "the package is missing the engine."
  rm -rf "$dest/voxsplit-engine" "$dest/models"
  mkdir -p "$dest" "$bin"
  ditto "$payload" "$dest"
  ln -sfn "$dest/voxsplit-engine" "$bin/voxsplit"
  ok "Installed to $dest"
  "$bin/voxsplit" --version >/dev/null || fail "voxsplit does not run on this Mac."

  case ":$PATH:" in
    *":$bin:"*)
      echo
      echo "  Try it: ${BOLD}voxsplit separate song.mp3${RESET}"
      ;;
    *)
      echo
      echo "  Add ~/.local/bin to your PATH, then open a new Terminal window:"
      echo "    echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc"
      echo "  Then: ${BOLD}voxsplit separate song.mp3${RESET}"
      ;;
  esac
}

uninstall() {
  step "Removing VoxSplit"
  local user_dir="$HOME/Library/Application Support/VoxSplit"
  rm -rf "$user_dir" "$HOME/Library/Caches/com.voxsplit.engine"
  if [[ -L "$HOME/.local/bin/voxsplit" ]]; then rm -f "$HOME/.local/bin/voxsplit"; fi

  local system=(/Applications/VoxSplit.app /Library/Audio/Plug-Ins/VST3/VoxSplit.vst3
    /Library/Audio/Plug-Ins/Components/VoxSplit.component "/Library/Application Support/VoxSplit")
  local found=()
  for path in "${system[@]}"; do [[ -e "$path" ]] && found+=("$path"); done
  if [[ -L /usr/local/bin/voxsplit ]]; then found+=(/usr/local/bin/voxsplit); fi
  if ((${#found[@]})); then
    note "macOS asks for your password to remove the system-wide install."
    sudo rm -rf "${found[@]}" </dev/tty
    sudo pkgutil --forget com.voxsplit.pkg >/dev/null 2>&1 </dev/tty || true
  fi
  ok "VoxSplit removed. Your stems in ~/Music/VoxSplit were kept."
}

usage() {
  cat <<'USAGE'
VoxSplit installer (Apple silicon, macOS 14+)

  curl -fsSL https://voxsplit.triplejmusic.art/install.sh | bash
      The app, the VST3/AU plugins and the `voxsplit` command (asks for your password).
  ... | bash -s -- --cli
      Only the `voxsplit` command, in your home folder (no password).
  ... | bash -s -- --uninstall
      Remove VoxSplit (your stems in ~/Music/VoxSplit are kept).
USAGE
}

check_mac() {
  [[ "$(uname -s)" == Darwin ]] || fail "VoxSplit runs on macOS only."
  # Rosetta shells report x86_64; the hardware is what matters.
  [[ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" == 1 ]] \
    || fail "VoxSplit needs a Mac with Apple silicon (M1 or later)."
  local major
  major="$(sw_vers -productVersion | cut -d. -f1)"
  ((major >= 14)) || fail "VoxSplit needs macOS 14 Sonoma or later (this Mac has $(sw_vers -productVersion))."
}

if [[ -t 2 && -z "${NO_COLOR:-}" ]]; then
  BOLD=$'\033[1m' DIM=$'\033[2m' GREEN=$'\033[32m' RED=$'\033[31m' RESET=$'\033[0m'
else
  BOLD="" DIM="" GREEN="" RED="" RESET=""
fi
step() { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$1" >&2; }
ok() { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$1" >&2; }
note() { printf '  %s%s%s\n' "$DIM" "$1" "$RESET" >&2; }
fail() {
  printf '%serror:%s %s\n' "$RED" "$RESET" "$1" >&2
  exit 1
}

main "$@"
