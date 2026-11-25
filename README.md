# telemetry-frame

Encode and recover binary telemetry frames on a UART byte stream. The Rust
library uses `no_std`, fixed-size storage, and caller-provided output slices.
It has zero dependencies.

## Run the UART simulation

```sh
cargo run --quiet --example uart_stream
```

The simulation feeds noise, a frame with a corrupt CRC, and a valid frame split
across three reads. It checks that exactly one payload, `imu_ax=0.12`, is accepted.

## Wire format

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 2 | Sync marker `A5 5A` |
| 2 | 1 | Protocol version `01` |
| 3 | 2 | Payload length, unsigned little-endian, 0 to 128 |
| 5 | N | Payload bytes |
| 5 + N | 2 | CRC-16/CCITT-FALSE, little-endian |

The CRC covers the version, length, and payload. It uses polynomial `0x1021`,
initial value `0xFFFF`, no reflection, and no final XOR. The check value for
ASCII `123456789` is `0x29B1`. [AUTOSAR CRC specification, §7.2.2.1](https://www.autosar.org/fileadmin/standards/R21-11/CP/AUTOSAR_SWS_CRCLibrary.pdf)

The reference frame below carries payload `00 A5 5A`, including the sync
pattern as ordinary payload data. The CRC of `01 03 00 00 A5 5A` is `0xBC56`.
The tests check these exact wire bytes against both the encoder and the decoder.

```text
A5 5A | 01 | 03 00 | 00 A5 5A | 56 BC
```

## Receive loop

1. Feed bytes from a UART read into `Decoder::push`. The 270-byte ring holds
   two maximum-size frames. A push that exceeds available space leaves the ring
   unchanged. Use `available()` to split a larger read and drain between chunks.
2. Call `next_frame` with a payload buffer. Process each `Ok(Some(len))`, then
   keep draining. `Ok(None)` means the decoder needs more bytes. A zero-length
   payload is returned as `Ok(Some(0))`.
3. For an unsupported version, out-of-range length, or bad CRC, call
   `next_frame` again to resume searching. The decoder advances one byte past
   the rejected candidate's start, allowing it to find a later sync marker
   even inside the rejected candidate. `OutputTooSmall` keeps a valid frame
   queued for retry with a larger slice.

The ring tracks a read offset and a byte count. Appending across the end of the
array uses two slice copies. Consuming bytes moves the read offset without
shifting the remaining bytes.

A truncated frame with a plausible length can leave the decoder waiting for
more bytes. If the transport detects a gap or an inter-byte timeout, use
`reset()` to discard pending bytes before feeding new data. The UART driver
owns read timing and timeout policy.

## Verify

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
rustup target add thumbv6m-none-eabi
cargo build --locked --lib --target thumbv6m-none-eabi
```

The tests cover the reference CRC, fragmented reads at every ring position,
two maximum-size frames in one read, resynchronization, truncation, and
overflow recovery. The cross-build targets bare-metal Cortex-M0/M0+.
The host example simulates UART input. [Rust target documentation](https://doc.rust-lang.org/stable/rustc/platform-support/thumbv6m-none-eabi.html)
