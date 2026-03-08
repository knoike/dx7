# dx7-rpi — DX7 FM Synth on Raspberry Pi Pico 2 W

DX7 FM synthesizer running on the RP2350 (Cortex-M33 @ 150 MHz).

## Prerequisites

```bash
# ARM Cortex-M33 target
rustup target add thumbv8m.main-none-eabihf

# Flash tool (pick one)
cargo install probe-rs-tools   # debug probe (SWD)
cargo install elf2uf2-rs       # UF2 drag-and-drop
```

## Build

```bash
cd dx7-rpi

# Benchmark only (no audio)
cargo build --release

# PWM audio demo (hardcoded note)
cargo build --release --features pwm

# USB MIDI synth with PWM audio
cargo build --release --features "usb-midi,pwm"
```

## Flash

### Via debug probe (SWD)

```bash
probe-rs run --chip RP2350 target/thumbv8m.main-none-eabihf/release/dx7-rpi
```

### Via UF2 (hold BOOTSEL + plug USB)

```bash
elf2uf2-rs target/thumbv8m.main-none-eabihf/release/dx7-rpi dx7-rpi.uf2
# Copy dx7-rpi.uf2 to the RPI-RP2 USB drive
```

## Features

| Feature    | Description                    | Audio | MIDI Input |
|------------|--------------------------------|-------|------------|
| `pwm`      | PWM audio on GP15              | Yes   | No (demo)  |
| `usb-midi` | USB MIDI class device          | —     | Yes        |
| `i2s`      | PIO I2S for external DAC       | Yes   | No         |
| `ble-midi` | BLE MIDI via CYW43439          | —     | Yes        |
| `uart-midi`| Classic 31250 baud MIDI        | —     | Yes        |

Typical combinations:
- `--features pwm` — demo playback, no MIDI
- `--features "usb-midi,pwm"` — live synth, plug into DAW

## Pin Mapping

| Function   | GPIO  | Feature     | Notes                          |
|------------|-------|-------------|--------------------------------|
| PWM audio  | GP15  | `pwm`       | RC filter → headphones         |
| I2S BCK    | GP16  | `i2s`       | PCM5102A DAC                   |
| I2S LRCK   | GP17  | `i2s`       |                                |
| I2S DOUT   | GP18  | `i2s`       |                                |
| UART RX    | GP1   | `uart-midi` | DIN-5 / TRS connector          |
| CYW43 SPI  | GP23,24,25,29 | `ble-midi` | Hardwired on Pico 2 W   |
| USB        | —     | `usb-midi`  | Internal USB controller        |

## PWM Audio Wiring

For headphone output from the PWM pin, use a simple RC low-pass filter:

```
GP15 ──[1kΩ]──┬── audio out
              [100nF]
               │
              GND
```

Cutoff frequency: ~1.6 kHz (adequate for demo; use I2S + DAC for quality audio).

## Performance

- RP2350: 150 MHz Cortex-M33, 520 KB SRAM
- Block size: 64 samples @ 48 kHz = 1333 us deadline
- Expected: ~30-35k cycles/voice → 5-6 voices per core
- Dual-core (future): 10-12 voices
