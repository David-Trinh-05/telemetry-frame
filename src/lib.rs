#![no_std]
#![forbid(unsafe_code)]
//! Allocation-free framing for serial byte streams. See [`Decoder`] for receive behavior.
//!
//! Frame layout: `A5 5A | version (01) | payload length (u16 LE) | payload | CRC (u16 LE)`.
//! CRC-16/CCITT-FALSE covers the version, length, and payload.

mod ring;
#[cfg(test)]
mod tests;

use ring::Ring;

const SYNC: [u8; 2] = [0xA5, 0x5A];
const VERSION: u8 = 1;
const HEADER_LEN: usize = 5;
const CRC_LEN: usize = 2;

/// Maximum payload length on the wire.
pub const MAX_PAYLOAD_LEN: usize = 128;
/// Maximum encoded frame length, including the header and CRC.
pub const MAX_FRAME_LEN: usize = HEADER_LEN + MAX_PAYLOAD_LEN + CRC_LEN;
/// Number of bytes the receive buffer can hold.
pub const RX_CAPACITY: usize = 2 * MAX_FRAME_LEN;

/// An input or output bound prevented encoding. The output stays unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    PayloadTooLong { length: usize },
    OutputTooSmall { required: usize },
}

/// The offered chunk did not fit. The receive buffer stays unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferFull {
    pub offered: usize,
    pub available: usize,
}

/// Decoding failed. Only `OutputTooSmall` leaves the candidate in place for retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    UnsupportedVersion { version: u8 },
    PayloadTooLong { length: usize },
    BadCrc { received: u16, computed: u16 },
    OutputTooSmall { required: usize },
}

/// Encode one payload into caller-owned storage and return the number of bytes written.
pub fn encode(payload: &[u8], output: &mut [u8]) -> Result<usize, EncodeError> {
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(EncodeError::PayloadTooLong {
            length: payload.len(),
        });
    }

    let crc_offset = HEADER_LEN + payload.len();
    let required = crc_offset + CRC_LEN;
    if output.len() < required {
        return Err(EncodeError::OutputTooSmall { required });
    }

    output[..2].copy_from_slice(&SYNC);
    output[2] = VERSION;
    output[3..HEADER_LEN].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    output[HEADER_LEN..crc_offset].copy_from_slice(payload);
    let checksum = crc16(output[2..crc_offset].iter().copied());
    output[crc_offset..required].copy_from_slice(&checksum.to_le_bytes());
    Ok(required)
}

/// Incrementally decode frames from a ring buffer owned by the decoder.
pub struct Decoder {
    rx: Ring,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// Create a decoder with an empty receive buffer.
    pub const fn new() -> Self {
        Self { rx: Ring::new() }
    }

    /// Return the number of additional bytes that can be pushed.
    pub fn available(&self) -> usize {
        RX_CAPACITY - self.rx.len()
    }

    /// Append the entire UART chunk, or leave existing bytes intact if it does not fit.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), BufferFull> {
        let available = self.available();
        if bytes.len() > available {
            return Err(BufferFull {
                offered: bytes.len(),
                available,
            });
        }
        self.rx.extend(bytes);
        Ok(())
    }

    /// Drop pending bytes after a transport timeout or known gap. Return the count dropped.
    pub fn reset(&mut self) -> usize {
        let dropped = self.rx.len();
        self.rx.discard(dropped);
        dropped
    }

    /// Copy the next valid payload to `output` and return its length.
    ///
    /// Noise before the sync marker is skipped. `Ok(None)` means more bytes are
    /// needed. Invalid candidates advance the search by one byte, so the caller
    /// can immediately call again to look for another frame. A short output
    /// buffer preserves the valid candidate for retry. Errors leave `output` unchanged.
    pub fn next_frame(&mut self, output: &mut [u8]) -> Result<Option<usize>, DecodeError> {
        self.skip_noise();
        if self.rx.len() < HEADER_LEN {
            return Ok(None);
        }

        let version = self.rx.byte(2);
        if version != VERSION {
            return Err(self.reject(DecodeError::UnsupportedVersion { version }));
        }

        let length = usize::from(u16::from_le_bytes([self.rx.byte(3), self.rx.byte(4)]));
        if length > MAX_PAYLOAD_LEN {
            return Err(self.reject(DecodeError::PayloadTooLong { length }));
        }

        let crc_offset = HEADER_LEN + length;
        let frame_len = crc_offset + CRC_LEN;
        if self.rx.len() < frame_len {
            return Ok(None);
        }

        let received = u16::from_le_bytes([self.rx.byte(crc_offset), self.rx.byte(crc_offset + 1)]);
        let computed = crc16((2..crc_offset).map(|index| self.rx.byte(index)));
        if received != computed {
            return Err(self.reject(DecodeError::BadCrc { received, computed }));
        }
        if output.len() < length {
            return Err(DecodeError::OutputTooSmall { required: length });
        }

        for (index, byte) in output[..length].iter_mut().enumerate() {
            *byte = self.rx.byte(HEADER_LEN + index);
        }
        self.rx.discard(frame_len);
        Ok(Some(length))
    }

    fn skip_noise(&mut self) {
        while self.rx.len() > 0 {
            if self.rx.byte(0) == SYNC[0] && (self.rx.len() == 1 || self.rx.byte(1) == SYNC[1]) {
                return;
            }
            self.rx.discard(1);
        }
    }

    fn reject(&mut self, error: DecodeError) -> DecodeError {
        self.rx.discard(1);
        error
    }
}

fn crc16(bytes: impl IntoIterator<Item = u8>) -> u16 {
    let mut crc = 0xFFFF_u16;
    for byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            let polynomial = if crc & 0x8000 != 0 { 0x1021 } else { 0 };
            crc = (crc << 1) ^ polynomial;
        }
    }
    crc
}
