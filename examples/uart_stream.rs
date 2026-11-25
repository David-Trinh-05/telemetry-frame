use telemetry_frame::{encode, DecodeError, Decoder, MAX_FRAME_LEN, MAX_PAYLOAD_LEN};

fn main() {
    let message = b"imu_ax=0.12";
    let mut wire = [0; MAX_FRAME_LEN];
    let encoded = encode(message, &mut wire).expect("encode example payload");
    let mut corrupted = wire;
    corrupted[encoded - 1] ^= 0x01;

    let reads: [(&str, &[u8]); 5] = [
        ("noise", &[0x00, 0xFF, 0xA5]),
        ("corrupt frame", &corrupted[..encoded]),
        ("valid frame, read 1/3", &wire[..1]),
        ("valid frame, read 2/3", &wire[1..8]),
        ("valid frame, read 3/3", &wire[8..encoded]),
    ];

    let mut decoder = Decoder::new();
    let mut output = [0; MAX_PAYLOAD_LEN];
    let mut accepted = 0;
    let mut rejected = 0;
    for (label, bytes) in reads {
        println!("Read {label} ({} bytes)", bytes.len());
        decoder.push(bytes).expect("example chunk fits");
        loop {
            match decoder.next_frame(&mut output) {
                Ok(Some(len)) => {
                    assert_eq!(&output[..len], message);
                    println!("  accepted {}", String::from_utf8_lossy(&output[..len]));
                    accepted += 1;
                }
                Ok(None) => break,
                Err(DecodeError::BadCrc { received, computed }) => {
                    println!("  rejected CRC {received:04X}, expected {computed:04X}");
                    rejected += 1;
                }
                Err(error) => panic!("unexpected decode error {error:?}"),
            }
        }
    }
    assert_eq!((accepted, rejected), (1, 1));
}
