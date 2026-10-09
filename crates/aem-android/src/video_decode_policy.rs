//! Pure decisions shared by the decoder and host regression tests.
pub const SUPERSEDED: &str = "video target superseded";

pub fn needs_seek(last_output: Option<i64>, wanted: u64, submitted_until: Option<u64>) -> bool {
    let Some(last) = last_output else {
        return true;
    };
    let last = last.max(0) as u64;
    if wanted <= last {
        return true;
    }
    // A delayed presentation may request a frame already submitted to the
    // codec. Drain that pipeline instead of discarding it and decoding again.
    wanted.saturating_sub(last) > 500_000 && submitted_until.is_none_or(|queued| wanted > queued)
}

pub fn decoder_error(error: String, context: impl FnOnce() -> String) -> String {
    if error == SUPERSEDED {
        error
    } else {
        format!("{}: {error}", context())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forward_misses_drain_submitted_frames_but_real_seeks_still_reset() {
        assert!(needs_seek(None, 0, None));
        assert!(!needs_seek(Some(0), 33_333, Some(266_664)));
        assert!(!needs_seek(Some(0), 900_000, Some(1_000_000)));
        assert!(needs_seek(Some(0), 900_000, Some(500_000)));
        assert!(needs_seek(Some(900_000), 33_333, Some(1_000_000)));
        assert!(needs_seek(Some(900_000), 900_000, Some(1_000_000)));
    }
    #[test]
    fn cancelled_requests_keep_the_worker_marker_and_real_errors_keep_context() {
        assert_eq!(
            decoder_error(SUPERSEDED.into(), || panic!("must not wrap cancellation")),
            SUPERSEDED
        );
        assert_eq!(
            decoder_error("invalid YUV".into(), || "codec example".into()),
            "codec example: invalid YUV"
        );
    }
}
