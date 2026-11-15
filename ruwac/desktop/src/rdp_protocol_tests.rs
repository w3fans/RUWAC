//! Generated protocol conformance tests; no remote account or authentication.
use super::connector_config;
use ironrdp::connector::{
    ClientConnector, ClientConnectorState, ConnectionFinalizationSequence,
    ConnectionFinalizationState, Sequence,
};
use ironrdp::pdu::rdp::{
    client_info::CompressionType,
    finalization_messages::{FontPdu, MonitorLayoutPdu},
    headers::{
        CompressionFlags, ShareControlHeader, ShareControlPdu, ShareDataHeader, ShareDataPdu,
        StreamPriority,
    },
};
use ironrdp::pdu::{gcc, mcs, nego, x224::X224};
use ironrdp::session::x224::Processor;
use ironrdp_core::{WriteBuf, decode, encode_vec};
use std::borrow::Cow;

fn layout() -> ShareDataPdu {
    ShareDataPdu::MonitorLayout(MonitorLayoutPdu {
        monitors: vec![
            gcc::Monitor {
                left: 0,
                top: 0,
                right: 1279,
                bottom: 719,
                flags: gcc::MonitorFlags::PRIMARY,
            },
            gcc::Monitor {
                left: -1920,
                top: 0,
                right: -1,
                bottom: 1079,
                flags: gcc::MonitorFlags::empty(),
            },
        ],
    })
}

fn server_frame(pdu: ShareDataPdu) -> Vec<u8> {
    let header = ShareControlHeader {
        share_control_pdu: ShareControlPdu::Data(ShareDataHeader {
            share_data_pdu: pdu,
            stream_priority: StreamPriority::Medium,
            compression_flags: CompressionFlags::empty(),
            compression_type: CompressionType::K8,
        }),
        pdu_source: 1001,
        share_id: 0x0001_03ea,
    };
    encode_vec(&X224(mcs::SendDataIndication {
        initiator_id: 1001,
        channel_id: 1003,
        user_data: Cow::Owned(encode_vec(&header).unwrap()),
    }))
    .unwrap()
}

#[test]
fn gcc_wire_advertises_monitor_notifications_independently_of_graphics_pipeline() {
    for graphics in [false, true] {
        let mut config = connector_config("generated-user".into(), "generated-password".into());
        config.support_dyn_vc_gfx_protocol = graphics;
        let mut client = ClientConnector::new(config, "127.0.0.1:0".parse().unwrap());
        // Exercise real GCC/MCS encoding only. This is not TLS or login evidence.
        client.state = ClientConnectorState::BasicSettingsExchangeSendInitial {
            selected_protocol: nego::SecurityProtocol::SSL,
        };
        let mut output = WriteBuf::new();
        client.step(&[], &mut output).unwrap();
        assert_eq!(&output.filled()[4..7], &[2, 0xf0, 0x80]);
        let request: mcs::ConnectInitial = decode(&output.filled()[7..]).unwrap();
        let flags = request
            .conference_create_request
            .gcc_blocks()
            .core
            .optional_data
            .early_capability_flags
            .unwrap();
        assert!(flags.contains(gcc::ClientEarlyCapabilityFlags::SUPPORT_MONITOR_LAYOUT_PDU));
        assert_eq!(
            flags.contains(gcc::ClientEarlyCapabilityFlags::SUPPORT_DYN_VC_GFX_PROTOCOL),
            graphics
        );
    }
}

#[test]
fn monitor_notification_before_font_map_does_not_abort_activation() {
    let mut sequence = ConnectionFinalizationSequence::new(1003, 1001, 0x0001_03ea);
    sequence.state = ConnectionFinalizationState::WaitForResponse;
    let mut output = WriteBuf::new();
    sequence.step(&server_frame(layout()), &mut output).unwrap();
    assert!(matches!(
        sequence.state,
        ConnectionFinalizationState::WaitForResponse
    ));
    assert!(output.filled().is_empty());
    sequence
        .step(
            &server_frame(ShareDataPdu::FontMap(FontPdu::default())),
            &mut output,
        )
        .unwrap();
    assert!(matches!(
        sequence.state,
        ConnectionFinalizationState::Finished
    ));
}

#[test]
fn active_monitor_notification_is_metadata_and_preserves_channel_processing() {
    let mut processor = Processor::new(
        ironrdp::svc::StaticChannelSet::new(),
        1001,
        1003,
        None,
        0x0001_03ea,
    );
    for _ in 0..3 {
        let result = processor.process(&server_frame(layout())).unwrap();
        assert!(
            result.is_empty(),
            "Notification must not reset or replace canonical pixels"
        );
    }
    // A normal subsequent packet still follows the same channel and share.
    assert!(
        processor
            .process(&server_frame(ShareDataPdu::ServerSetErrorInfo(
                ironrdp::pdu::rdp::server_error_info::ServerSetErrorInfoPdu(
                    ironrdp::pdu::rdp::server_error_info::ErrorInfo::ProtocolIndependentCode(
                        ironrdp::pdu::rdp::server_error_info::ProtocolIndependentCode::None
                    )
                )
            )))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn truncated_monitor_notification_is_rejected_during_activation_and_session() {
    let mut frame = server_frame(layout());
    frame.pop();
    let mut sequence = ConnectionFinalizationSequence::new(1003, 1001, 0x0001_03ea);
    sequence.state = ConnectionFinalizationState::WaitForResponse;
    assert!(sequence.step(&frame, &mut WriteBuf::new()).is_err());
    let mut processor = Processor::new(
        ironrdp::svc::StaticChannelSet::new(),
        1001,
        1003,
        None,
        0x0001_03ea,
    );
    assert!(processor.process(&frame).is_err());
}

#[test]
fn clearcodec_microsoft_example_four_short_bar_coordinates() {
    // MS-RDPEGFX 4.1.1.4: the first band's header is 7 by 15 pixels,
    // and short-bar word 0x0f00 means YOn=0 and YOff=15.
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpegfx/266bbde5-a651-41a9-a5b7-721b7c4f2663
    let bands = [
        0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xb6, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xb6, 0x66, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xb6, 0x66, 0xdb, 0x90,
        0x3a, 0xff, 0xff, 0xb6, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x46, 0x91,
        0x47, 0x91, 0x48, 0x91, 0x49, 0x91, 0x4a, 0x91, 0x1b, 0x91,
    ];
    assert_eq!(bands.len(), 70);
    let decoded = ironrdp::pdu::codecs::clearcodec::decode_bands_layer(&bands).unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].vbars.len(), 7);
    match &decoded[0].vbars[0] {
        ironrdp::pdu::codecs::clearcodec::VBar::ShortCacheMiss(bar) => {
            assert_eq!(
                (bar.y_on, bar.y_off_delta, bar.pixel_data.len()),
                (0, 15, 45)
            );
        }
        _ => panic!("Expected the documented short-bar cache miss"),
    }
}

#[test]
fn clearcodec_short_bar_renders_all_rows_with_exact_bgra_pixels() {
    let mut band = Vec::new();
    for value in [0u16, 0, 0, 14] {
        band.extend_from_slice(&value.to_le_bytes());
    }
    band.extend_from_slice(&[0, 0, 0]);
    band.extend_from_slice(&0x0f00u16.to_le_bytes());
    band.extend_from_slice(&[1, 2, 3].repeat(15));
    let mut stream = vec![0, 0];
    stream.extend_from_slice(&0u32.to_le_bytes());
    stream.extend_from_slice(&(band.len() as u32).to_le_bytes());
    stream.extend_from_slice(&0u32.to_le_bytes());
    stream.extend_from_slice(&band);
    let mut decoder = ironrdp::graphics::clearcodec::ClearCodecDecoder::new();
    assert_eq!(
        decoder.decode(&stream, 1, 15).unwrap(),
        [1, 2, 3, 255].repeat(15)
    );
    // Truncating the pixel payload remains an error, never a partial tile.
    assert!(decoder.decode(&stream[..stream.len() - 1], 1, 15).is_err());
}

#[test]
fn srl_complete_zero_chunks_retain_state_across_bands_without_reading_a_nonzero() {
    use ironrdp::graphics::srl::{SrlDecoder, decode_srl};
    // Eight complete zero chunks encode 2+2+4+4+8+8+16+16 coefficients.
    let encoded = [0, 0]; // The second byte is an optional trailing zero byte.
    assert_eq!(decode_srl(&encoded, 60, 1), Ok(vec![0; 60]));
    let mut decoder = SrlDecoder::new(&encoded).unwrap();
    assert_eq!(decoder.decode(1, 1), Ok(vec![0]));
    assert_eq!(decoder.decode(59, 1), Ok(vec![0; 59]));
    // Past the end, bits read as zero chunks, as in FreeRDP.
    assert_eq!(decoder.decode(1, 1), Ok(vec![0]));
}

#[test]
fn srl_streams_from_windows_without_trailing_zero_byte_decode() {
    use ironrdp::graphics::srl::{SrlError, decode_srl};
    assert_eq!(decode_srl(&[0x84, 0], 1, 4), Ok(vec![3]));
    assert_eq!(decode_srl(&[0xa0, 0, 0], 1, 3), Ok(vec![-7]));
    // Windows Progressive tiles: no trailing zero byte, or an empty stream.
    assert_eq!(decode_srl(&[0x84], 1, 4), Ok(vec![3]));
    assert_eq!(decode_srl(&[], 2, 4), Ok(vec![0, 0]));
    // Magnitude bounds remain enforced.
    assert_eq!(decode_srl(&[0x84], 1, 0), Err(SrlError::InvalidBitCount(0)));
}
