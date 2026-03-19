use fabstir_llm_node::transcoder::billing::{
    calculate_transcode_units, codec_factor, resolution_factor_from_vf, TranscodingTracker,
};

#[test]
fn test_resolution_factor_1080p() {
    assert_eq!(resolution_factor_from_vf("scale=1920:1080"), 1.0);
}

#[test]
fn test_resolution_factor_4k() {
    assert_eq!(resolution_factor_from_vf("scale=3840:2160"), 2.0);
}

#[test]
fn test_resolution_factor_720p() {
    assert_eq!(resolution_factor_from_vf("scale=1280:720"), 0.5);
}

#[test]
fn test_resolution_factor_480p() {
    assert_eq!(resolution_factor_from_vf("scale=854:480"), 0.25);
}

#[test]
fn test_resolution_factor_no_vf() {
    assert_eq!(resolution_factor_from_vf(""), 1.0);
}

#[test]
fn test_codec_factor_h264() {
    assert_eq!(codec_factor("h264_nvenc"), 1.0);
}

#[test]
fn test_codec_factor_av1() {
    assert_eq!(codec_factor("av1_nvenc"), 1.5);
}

#[test]
fn test_calculate_units_1080p_h264_60s() {
    assert_eq!(calculate_transcode_units(60.0, 1.0, 1.0, false), 60.0);
}

#[test]
fn test_calculate_units_4k_av1_60s_encrypted() {
    let units = calculate_transcode_units(60.0, 2.0, 1.5, true);
    assert!(
        (units - 198.0).abs() < 0.01,
        "expected 198.0, got {}",
        units
    );
}

#[test]
fn test_units_to_tokens_conversion() {
    let units = 60.0_f64;
    let tokens = (units * 1000.0).ceil() as u64;
    assert_eq!(tokens, 60000);
}

#[tokio::test]
async fn test_tracker_track_and_get() {
    let tracker = TranscodingTracker::new();
    tracker.track(1, Some("session-1".into()), 100.0).await;
    let info = tracker.get_job_info(1).await;
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.job_id, 1);
    assert!((info.total_units - 100.0).abs() < 0.01);
}

#[tokio::test]
async fn test_tracker_accumulates_across_formats() {
    let tracker = TranscodingTracker::new();
    tracker.track(1, Some("session-1".into()), 50.0).await;
    tracker.track(1, Some("session-1".into()), 75.0).await;
    let info = tracker.get_job_info(1).await.unwrap();
    assert!((info.total_units - 125.0).abs() < 0.01);
    assert_eq!(info.format_count, 2);
}
