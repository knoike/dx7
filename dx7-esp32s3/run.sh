#!/bin/bash
set -e

# Ensure xtensa GCC is on PATH
XTENSA_BIN=$(find ~/.rustup/toolchains/esp -name 'xtensa-esp32s3-elf-gcc' -printf '%h' 2>/dev/null)
[ -n "$XTENSA_BIN" ] && export PATH="$XTENSA_BIN:$PATH"

QEMU=$(find ~/.espressif/tools/qemu-xtensa -name 'qemu-system-xtensa' 2>/dev/null | head -1)
ELF=target/xtensa-esp32s3-none-elf/release/dx7-esp32s3
BIN=bin/dx7-esp32s3.bin

cargo +esp build --release --features qemu

mkdir -p bin
esptool --chip esp32s3 elf2image --ram-only-header -fs 4MB -fm dio -ff 40m -o bin/dx7.bin "$ELF" 2>&1 | tail -1
esptool --chip esp32s3 merge-bin --pad-to-size 4MB -fm dio -ff 40m --output "$BIN" 0x0000 bin/dx7.bin 2>&1 | tail -1
rm bin/dx7.bin

echo "Image: $BIN ($(stat -c%s "$BIN") bytes)"

if [ "$1" = "--build-only" ]; then
    exit 0
fi

timeout 15 "$QEMU" -machine esp32s3 -nographic -no-reboot -serial mon:stdio \
    -drive file="$BIN",if=mtd,format=raw 2>/dev/null || true
