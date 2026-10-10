/// Extractors may read compressed packets even when only scanning timestamps.
/// Retain a bounded deadline without treating a multi-GiB cold read as a hang.
pub fn timestamp_scan_budget(source_bytes: u64) -> std::time::Duration {
    let chunks = source_bytes
        .min(motion_model::storage::MAX_MEDIA_ASSET)
        .div_ceil(16 * 1024 * 1024);
    std::time::Duration::from_secs(30 + chunks)
}

/// Correct rounded container frame-rate hints using constant-rate source PTS.
/// Variable-rate streams keep a declared nominal rate; their sampling uses PTS.
pub fn source_frame_rate(declared: f64, pts: &[u64], step: u64, variable: bool) -> f64 {
    let valid = declared.is_finite() && declared > 0.0;
    let fallback = if valid {
        declared
    } else {
        1_000_000.0 / step.max(1) as f64
    };
    if variable || pts.len() < 2 {
        return fallback;
    }
    let span = pts.last().unwrap().saturating_sub(pts[0]);
    if span == 0 {
        return fallback;
    }
    let observed = (pts.len() - 1) as f64 * 1_000_000.0 / span as f64;
    // Both endpoints can round by one microsecond. Only round to an integer
    // inside this uncertainty, so 59.94/29.97 never become 60/30 by assumption.
    let uncertainty = observed * 2.0 / span as f64;
    if valid && (declared - observed).abs() <= uncertainty.max(0.00001) {
        return declared;
    }
    if (observed - observed.round()).abs() <= uncertainty {
        observed.round()
    } else {
        observed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamp_scan_has_a_size_sensitive_bounded_deadline() {
        assert_eq!(timestamp_scan_budget(0).as_secs(), 30);
        assert_eq!(timestamp_scan_budget(16 * 1024 * 1024).as_secs(), 31);
        assert_eq!(timestamp_scan_budget(16 * 1024 * 1024 + 1).as_secs(), 32);
        assert_eq!(timestamp_scan_budget(512 * 1024 * 1024).as_secs(), 62);
        assert_eq!(
            timestamp_scan_budget(motion_model::storage::MAX_MEDIA_ASSET).as_secs(),
            158
        );
        assert_eq!(timestamp_scan_budget(u64::MAX).as_secs(), 158);
    }
    fn pts(fps: f64) -> Vec<u64> {
        (0..24)
            .map(|n| (n as f64 * 1_000_000.0 / fps).round() as u64)
            .collect()
    }
    #[test]
    fn rounded_headers_do_not_flatten_fractional_source_rates() {
        for (hint, actual) in [
            (60.0, 60000.0 / 1001.0),
            (30.0, 30000.0 / 1001.0),
            (120.0, 120000.0 / 1001.0),
        ] {
            assert!((source_frame_rate(hint, &pts(actual), 1, false) - actual).abs() < 0.001);
        }
        assert_eq!(source_frame_rate(240.0, &pts(240.0), 4167, false), 240.0);
        assert_eq!(source_frame_rate(0.0, &[0, 4167], 4167, false), 240.0);
        assert_eq!(source_frame_rate(0.0, &pts(300.0), 3333, false), 300.0);
        assert_eq!(
            source_frame_rate(30.0, &[0, 17000, 40000, 100000], 23000, true),
            30.0
        );
    }
}
