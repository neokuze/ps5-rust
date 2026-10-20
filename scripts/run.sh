#!/usr/bin/env bash
# Sends a payload to the PS5's ELF loader and stays on the connection: the
# loader gives it to the payload as stdin/stdout/stderr, so println! output
# shows up here and what you type goes to the payload's stdin.
#
# Usage: scripts/run.sh [-h HOST] [-p PORT] PAYLOAD.elf
#   HOST defaults to $PS5_HOST, PORT to $PS5_PORT or 9021.
#
# Unlike `prospero-deploy -i`, this returns as soon as the payload closes
# the connection (exits), instead of waiting for one more key press.
# Uses bash's /dev/tcp, so it needs no socat.
set -euo pipefail

host="${PS5_HOST:-}"
port="${PS5_PORT:-9021}"
while getopts "h:p:" o; do
  case "$o" in
    h) host="$OPTARG" ;;
    p) port="$OPTARG" ;;
    *) exit 2 ;;
  esac
done
shift $((OPTIND - 1))
elf="${1:-}"
[ -n "$host" ] && [ -f "$elf" ] || { echo "usage: $0 [-h HOST] [-p PORT] PAYLOAD.elf (or set PS5_HOST)" >&2; exit 2; }

exec 3<>"/dev/tcp/$host/$port" || { echo "can't connect to $host:$port (is the ELF loader running?)" >&2; exit 1; }
cat "$elf" >&3

keys=""
trap '[ -n "$keys" ] && kill "$keys" 2>/dev/null; exit' EXIT INT TERM
# Keyboard -> payload stdin. A background job's stdin is /dev/null in a
# script, so the terminal is opened explicitly.
if [ -t 0 ]; then
  cat < /dev/tty >&3 &
  keys=$!
fi
# Payload stdout/stderr -> here, until the payload exits and the loader's
# connection closes.
cat <&3
