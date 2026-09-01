#!/usr/bin/env bash
# Flash a Pico de Gallo image and PROVE it took.
#
# Written after a Task 10 run in which three consecutive flashes silently
# produced the same firmware, because BOOTSEL must be re-entered for EVERY
# flash and a `picotool load` outside BOOTSEL just prints an error that is
# easy to miss in scrollback.
#
# Every step here fails loudly. Nothing is assumed.
#
# Usage:  ./flash.sh /tmp/pdg-159/dirty.uf2

set -euo pipefail

IMG=${1:?usage: flash.sh <file.uf2>}
REPO=/home/balbi/workspace/pico-de-gallo

[ -f "$IMG" ] || { echo "FATAL: no such file: $IMG" >&2; exit 1; }

# The identity we expect the board to report afterwards, read out of the
# image itself rather than passed in by hand.
EXPECT=$(python3 - "$IMG" <<'PY'
import re, sys
d = open(sys.argv[1], 'rb').read()
p = b''.join(d[i+32:i+32+256] for i in range(0, len(d), 512))
m = re.findall(rb'firmware-v[0-9A-Za-z.\-]{0,60}', p)
if len(m) != 1:
    sys.exit(f"expected exactly one identity string, found {len(m)}")
# Trim the adjacent string that follows it in .rodata (no NUL terminator).
print(re.match(rb'firmware-v[0-9A-Za-z.]+(?:-\d+-g[0-9a-f]+)?(?:-dirty)?', m[0]).group().decode())
PY
)

echo "image    : $IMG"
echo "sha256   : $(sha256sum "$IMG" | cut -d' ' -f1)"
echo "identity : $EXPECT"
echo

echo ">>> Put the board in BOOTSEL now:"
echo "      unplug  ->  hold BOOTSEL  ->  plug in  ->  release BOOTSEL"
echo "    Waiting for a device in BOOTSEL mode (Ctrl-C to abort)..."

for _ in $(seq 1 120); do
    if picotool info >/dev/null 2>&1; then
        FOUND=1; break
    fi
    sleep 1
done
[ "${FOUND:-0}" = 1 ] || { echo "FATAL: no device entered BOOTSEL within 120 s" >&2; exit 1; }
echo "    BOOTSEL detected."
echo

# Load WITHOUT -x, so the board stays in BOOTSEL and can be verified.
echo ">>> Loading..."
picotool load "$IMG"

# This is the step that makes a silent no-op impossible.
echo ">>> Verifying device contents against the file..."
picotool verify "$IMG"

echo ">>> Rebooting into the application..."
picotool reboot || true
sleep 3
echo

echo ">>> Reading back over USB (branch-built gallo)..."
cd "$REPO"
OUT=$(timeout 60 cargo run -q -p gallo --locked -- version 2>/dev/null)
echo "$OUT"
echo

GOT=$(printf '%s\n' "$OUT" | sed -n 's/.*│ Build *│ *\([^ ]*\) *│.*/\1/p')
if [ -z "$GOT" ]; then
    echo "RESULT: FAIL — no Build row. Either the flash did not take, or the" >&2
    echo "        board is running firmware without build identity." >&2
    exit 1
fi
if [ "$GOT" = "$EXPECT" ]; then
    echo "RESULT: PASS — board reports '$GOT', matching the image."
else
    echo "RESULT: FAIL — board reports '$GOT' but the image contains '$EXPECT'." >&2
    echo "        picotool verify passed, so the flash DID take. This is a real" >&2
    echo "        firmware bug, not a flashing mistake. Report it." >&2
    exit 1
fi
