use pi_model::ModelServiceErrorCategory;
use pi_provider::{SseDecoder, SseEvent};

#[test]
fn decodes_comments_multiline_data_and_all_line_endings_across_byte_splits() {
    let source = b": keepalive\r\nevent: message\r\ndata: first\r\ndata: second\r\n\r\nevent: done\rdata: final\r\r";
    let mut decoder = SseDecoder::new(1_024);
    let mut events = Vec::new();
    for byte in source {
        events.extend(decoder.push(&[*byte]).expect("byte split must decode"));
    }
    events.extend(decoder.finish().expect("EOF must flush"));

    assert_eq!(
        events,
        [
            SseEvent {
                event: Some("message".to_owned()),
                data: "first\nsecond".to_owned(),
            },
            SseEvent {
                event: Some("done".to_owned()),
                data: "final".to_owned(),
            },
        ]
    );
}

#[test]
fn preserves_utf8_split_across_chunks_and_flushes_trailing_partial_event() {
    let source = "data: 你好".as_bytes();
    let mut decoder = SseDecoder::new(128);
    for byte in source {
        assert!(
            decoder
                .push(&[*byte])
                .expect("split UTF-8 is buffered")
                .is_empty()
        );
    }
    assert_eq!(
        decoder.finish().expect("trailing event must flush"),
        [SseEvent {
            event: None,
            data: "你好".to_owned(),
        }]
    );
}

#[test]
fn rejects_invalid_utf8_and_oversized_events() {
    let mut invalid_utf8 = SseDecoder::new(64);
    let error = invalid_utf8
        .push(&[b'd', b'a', b't', b'a', b':', b' ', 0xff, b'\n'])
        .expect_err("invalid UTF-8 must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
    assert!(!error.retryable);

    let mut oversized = SseDecoder::new(8);
    let error = oversized
        .push(b"data: payload")
        .expect_err("oversized event must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
}

#[test]
fn ignores_unknown_fields_and_comment_only_events() {
    let mut decoder = SseDecoder::new(128);
    let events = decoder
        .push(b": ping\nid: 1\nretry: 10\n\ndata\n\n")
        .expect("metadata fields are ignorable");
    assert_eq!(
        events,
        [SseEvent {
            event: None,
            data: String::new(),
        }]
    );
}

#[test]
fn accepts_one_large_chunk_containing_multiple_bounded_events() {
    let mut decoder = SseDecoder::new(8);
    let events = decoder
        .push(b"data: a\n\ndata: b\n\n")
        .expect("the bound applies per event, not per transport chunk");
    assert_eq!(
        events,
        [
            SseEvent {
                event: None,
                data: "a".to_owned(),
            },
            SseEvent {
                event: None,
                data: "b".to_owned(),
            },
        ]
    );
}
