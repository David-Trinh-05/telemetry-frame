use super::*;

// CRC independently computed with Python's binascii.crc_hqx(body, 0xFFFF).
const REFERENCE: [u8; 10] = [0xA5, 0x5A, 0x01, 0x03, 0x00, 0x00, 0xA5, 0x5A, 0x56, 0xBC];
const PAYLOAD: [u8; 3] = [0x00, 0xA5, 0x5A];

#[test]
fn matches_crc_check_value_and_reference_frame() {
    assert_eq!(crc16(*b"123456789"), 0x29B1);
    let mut wire = [0xFF; MAX_FRAME_LEN];
    let len = encode(&PAYLOAD, &mut wire).unwrap();
    assert_eq!(&wire[..len], REFERENCE);
    assert_eq!(wire[len], 0xFF);

    let mut decoder = Decoder::new();
    decoder.push(&REFERENCE).unwrap();
    let mut payload = [0xFF; MAX_PAYLOAD_LEN];
    assert_eq!(decoder.next_frame(&mut payload), Ok(Some(PAYLOAD.len())));
    assert_eq!(&payload[..PAYLOAD.len()], PAYLOAD);
    assert_eq!(payload[PAYLOAD.len()], 0xFF);
    assert_eq!(decoder.next_frame(&mut payload), Ok(None));
}

#[test]
fn reads_three_fragments_from_every_ring_position() {
    let noise = [0; RX_CAPACITY];
    for start in 0..RX_CAPACITY {
        let mut decoder = Decoder::new();
        let mut payload = [0xFF; 3];
        decoder.push(&noise[..start]).unwrap();
        assert_eq!(decoder.next_frame(&mut payload), Ok(None));
        assert_eq!(decoder.available(), RX_CAPACITY);

        decoder.push(&REFERENCE[..1]).unwrap();
        assert_eq!(decoder.next_frame(&mut payload), Ok(None));
        decoder.push(&REFERENCE[1..8]).unwrap();
        assert_eq!(decoder.next_frame(&mut payload), Ok(None));
        assert_eq!(payload, [0xFF; 3]);
        decoder.push(&REFERENCE[8..]).unwrap();
        assert_eq!(
            decoder.next_frame(&mut payload),
            Ok(Some(3)),
            "start {start}"
        );
        assert_eq!(payload, PAYLOAD);
        assert_eq!(decoder.next_frame(&mut payload), Ok(None));
    }
}

#[test]
fn reads_two_maximum_frames_from_one_push() {
    let mut wire = [0; RX_CAPACITY];
    let first = [0xA5; MAX_PAYLOAD_LEN];
    let second = [0x5A; MAX_PAYLOAD_LEN];
    let (first_wire, second_wire) = wire.split_at_mut(MAX_FRAME_LEN);
    assert_eq!(encode(&first, first_wire), Ok(MAX_FRAME_LEN));
    assert_eq!(encode(&second, second_wire), Ok(MAX_FRAME_LEN));

    let mut decoder = Decoder::new();
    decoder.push(&wire).unwrap();
    assert_eq!(decoder.available(), 0);
    let mut output = [0; MAX_PAYLOAD_LEN];
    assert_eq!(decoder.next_frame(&mut output), Ok(Some(MAX_PAYLOAD_LEN)));
    assert_eq!(output, first);
    assert_eq!(decoder.next_frame(&mut output), Ok(Some(MAX_PAYLOAD_LEN)));
    assert_eq!(output, second);
    assert_eq!(decoder.next_frame(&mut output), Ok(None));
    assert_eq!(decoder.available(), RX_CAPACITY);
}

#[test]
fn recovers_from_noise_bad_version_and_oversized_header() {
    let mut decoder = Decoder::new();
    decoder.push(&[0x11, 0xA5, 0xA5, 0x5A, 0x02, 0, 0]).unwrap();
    decoder.push(&[0xA5, 0x5A, 0x01, 0x81, 0]).unwrap();
    decoder.push(&REFERENCE).unwrap();
    let mut output = [0; MAX_PAYLOAD_LEN];
    assert_eq!(
        decoder.next_frame(&mut output),
        Err(DecodeError::UnsupportedVersion { version: 2 })
    );
    assert_eq!(
        decoder.next_frame(&mut output),
        Err(DecodeError::PayloadTooLong { length: 129 })
    );
    assert_eq!(decoder.next_frame(&mut output), Ok(Some(3)));
    assert_eq!(&output[..3], PAYLOAD);
    assert_eq!(decoder.next_frame(&mut output), Ok(None));
}

#[test]
fn finds_a_valid_frame_inside_a_candidate_with_bad_crc() {
    let mut decoder = Decoder::new();
    // The wrong length makes the first candidate cover the following valid frame.
    decoder.push(&[0xA5, 0x5A, 0x01, 8, 0]).unwrap();
    decoder.push(&REFERENCE).unwrap();
    let mut output = [0xFF; MAX_PAYLOAD_LEN];
    assert!(matches!(
        decoder.next_frame(&mut output),
        Err(DecodeError::BadCrc { .. })
    ));
    assert_eq!(output, [0xFF; MAX_PAYLOAD_LEN]);
    assert_eq!(decoder.next_frame(&mut output), Ok(Some(3)));
    assert_eq!(&output[..3], PAYLOAD);
    assert_eq!(decoder.next_frame(&mut output), Ok(None));
}

#[test]
fn overflow_and_small_output_preserve_data_for_retry() {
    let mut decoder = Decoder::new();
    decoder.push(&REFERENCE[..4]).unwrap();
    assert_eq!(
        decoder.push(&[0; RX_CAPACITY]),
        Err(BufferFull {
            offered: RX_CAPACITY,
            available: RX_CAPACITY - 4
        })
    );
    assert_eq!(decoder.available(), RX_CAPACITY - 4);

    decoder.push(&REFERENCE[4..]).unwrap();
    let mut small_output = [0xFF; 2];
    assert_eq!(
        decoder.next_frame(&mut small_output),
        Err(DecodeError::OutputTooSmall { required: 3 })
    );
    assert_eq!(small_output, [0xFF; 2]);
    assert_eq!(decoder.available(), RX_CAPACITY - REFERENCE.len());

    let mut output = [0; 3];
    assert_eq!(decoder.next_frame(&mut output), Ok(Some(3)));
    assert_eq!(output, PAYLOAD);
}

#[test]
fn discards_truncated_frame_on_transport_reset() {
    let mut decoder = Decoder::new();
    decoder.push(&REFERENCE[..9]).unwrap();
    let mut output = [0; 3];
    assert_eq!(decoder.next_frame(&mut output), Ok(None));
    assert_eq!(decoder.reset(), 9);
    assert_eq!(decoder.available(), RX_CAPACITY);

    decoder.push(&REFERENCE).unwrap();
    assert_eq!(decoder.next_frame(&mut output), Ok(Some(3)));
    assert_eq!(output, PAYLOAD);
}

#[test]
fn handles_empty_payload_and_rejects_encoding_errors_without_writing() {
    let mut wire = [0xFF; 7];
    assert_eq!(
        encode(&[0; MAX_PAYLOAD_LEN + 1], &mut wire),
        Err(EncodeError::PayloadTooLong {
            length: MAX_PAYLOAD_LEN + 1
        })
    );
    assert_eq!(
        encode(&[1], &mut wire),
        Err(EncodeError::OutputTooSmall { required: 8 })
    );
    assert_eq!(wire, [0xFF; 7]);

    assert_eq!(encode(&[], &mut wire), Ok(7));
    let mut decoder = Decoder::new();
    decoder.push(&wire).unwrap();
    assert_eq!(decoder.next_frame(&mut []), Ok(Some(0)));
    assert_eq!(decoder.next_frame(&mut []), Ok(None));
}
