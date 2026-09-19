//! Protocol torture + fuzz tests (FR-9.4, FR-9.5).
//!
//! These run in internal CI. They exercise the SSE framer and the outbound
//! adapters against adversarial transport chunking and malformed input, with
//! bounded resources (no unbounded buffers, no panics).

#![cfg(test)]

use crate::adapters::Adapter;
use crate::sse::SseFramer;
use crate::types::StreamEvent;

/// Feed `input` to `framer` in fixed-size chunks and collect all frames.
fn frames_in_chunks(input: &str, size: usize) -> Vec<String> {
    let mut f = SseFramer::new();
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    for chunk in bytes.chunks(size.max(1)) {
        out.extend(f.push(chunk));
    }
    out
}

fn gemini_events(input: &str, size: usize) -> Vec<StreamEvent> {
    let adapter = crate::adapters::gemini::GeminiAdapter;
    let mut evs = Vec::new();
    for frame in frames_in_chunks(input, size) {
        if let Some(payload) = crate::sse::extract_data(&frame) {
            evs.extend(adapter.parse_stream_chunk(&payload).unwrap_or_default());
        }
    }
    evs
}

fn openai_events(input: &str, size: usize) -> Vec<StreamEvent> {
    let adapter = crate::adapters::openai::OpenAiAdapter;
    let mut evs = Vec::new();
    for frame in frames_in_chunks(input, size) {
        if let Some(payload) = crate::sse::extract_data(&frame) {
            evs.extend(adapter.parse_stream_chunk(&payload).unwrap_or_default());
        }
    }
    evs
}

#[test]
fn gemini_text_survives_every_chunk_boundary() {
    let input = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello world\"}]}}]}\n\n\
                 data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"!\"}]},\"finishReason\":\"STOP\"}],\
                 \"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":3}}\n\n";
    // The reassembled text must be identical regardless of transport chunking.
    for size in 1..=64 {
        let evs = gemini_events(input, size);
        let text: String = evs
            .iter()
            .filter_map(|e| match e {
                StreamEvent::TextDelta(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello world!", "chunk size {size}");
    }
}

#[test]
fn gemini_tool_call_args_object() {
    // Gemini delivers functionCall args as a JSON object; the event stream must
    // carry the name and a faithful serialization of the args regardless of
    // transport chunking.
    let input = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}}]}}]}\n\n";
    for size in [1usize, 3, 7, 64] {
        let evs = gemini_events(input, size);
        let mut name = String::new();
        let mut args = String::new();
        for e in &evs {
            match e {
                StreamEvent::ToolCallStart { name: n, .. } => name = n.clone(),
                StreamEvent::ToolCallArgsDelta { args: a, .. } => args = a.clone(),
                _ => {}
            }
        }
        assert_eq!(name, "get_weather", "chunk size {size}");
        let parsed: serde_json::Value = serde_json::from_str(&args).expect("args is JSON");
        assert_eq!(parsed["city"], "Paris", "chunk size {size}");
    }
}

#[test]
fn gemini_usage_only_in_final_event() {
    // No usageMetadata until the very last frame; the framer must still deliver
    // the final usage exactly once.
    let input = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}\n\n\
                 data: {\"candidates\":[{\"content\":{\"parts\":[]},\"finishReason\":\"STOP\"}],\
                 \"usageMetadata\":{\"promptTokenCount\":11,\"candidatesTokenCount\":2,\"thoughtsTokenCount\":1}}\n\n";
    for size in [1usize, 5, 200] {
        let evs = gemini_events(input, size);
        let usage = evs.iter().find_map(|e| match e {
            StreamEvent::Usage(u) => Some(u.clone()),
            _ => None,
        });
        let u = usage.expect("usage present");
        assert_eq!(u.input, Some(11));
        assert_eq!(u.output, Some(2));
        assert_eq!(u.thinking, Some(1));
    }
}

#[test]
fn malformed_and_unknown_fields_do_not_panic() {
    let inputs = [
        "data: not json at all\n\n",
        "data: {\"candidates\":[{}]}\n\n",
        "data: {}\n\n",
        "data: [DONE]\n\n",
        ": keepalive\n\n",
        "event: weird\ndata: {\"unknown\":true,\"extra\":[1,2,3]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"unknownPart\":{\"x\":1}}]}}]}\n\n",
    ];
    for input in inputs {
        for size in [1usize, 2, 100] {
            // Must not panic; unknown frames simply yield no events.
            let _ = gemini_events(input, size);
            let _ = openai_events(input, size);
        }
    }
}

#[test]
fn openai_tool_args_split_across_frames() {
    let input = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"f\",\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
                 data: [DONE]\n\n";
    for size in 1..=48 {
        let evs = openai_events(input, size);
        let mut args = String::new();
        for e in &evs {
            if let StreamEvent::ToolCallArgsDelta { args: delta, .. } = e {
                args.push_str(delta);
            }
        }
        assert_eq!(args, "{\"a\":1}", "chunk size {size}");
    }
}

/// Bounded pseudo-random fuzz: random bytes and random re-chunking of a valid
/// stream must never panic or grow unboundedly.
#[test]
fn fuzz_bounded_resources() {
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    // 1. Random garbage bytes must not panic.
    for _ in 0..500 {
        let len = (next() % 256) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| (next() % 256) as u8).collect();
        let mut f = SseFramer::new();
        let _ = f.push(&bytes);
        // Buffer must never retain more than the last (incomplete) frame.
        assert!(f.pending() as usize <= 1);
    }

    // 2. Random re-chunking of a valid stream must reassemble deterministically.
    let valid = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"abcdefghij\"}]}}]}\n\n\
                 data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"klmnopqrst\"}]},\"finishReason\":\"STOP\"}]}\n\n";
    let expected_text = "abcdefghijklmnopqrst";
    for _ in 0..300 {
        let mut f = SseFramer::new();
        let bytes = valid.as_bytes();
        let mut i = 0;
        let mut frames = Vec::new();
        while i < bytes.len() {
            let step = (next() % 5 + 1) as usize;
            let end = (i + step).min(bytes.len());
            frames.extend(f.push(&bytes[i..end]));
            i = end;
        }
        let adapter = crate::adapters::gemini::GeminiAdapter;
        let mut text = String::new();
        for frame in frames {
            if let Some(p) = crate::sse::extract_data(&frame) {
                for ev in adapter.parse_stream_chunk(&p).unwrap_or_default() {
                    if let StreamEvent::TextDelta(t) = ev {
                        text.push_str(&t);
                    }
                }
            }
        }
        assert_eq!(text, expected_text);
    }
}
