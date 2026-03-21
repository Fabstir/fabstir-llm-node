use fabstir_llm_node::transcoder::{
    TranscodeStatusResponse, TranscodeSubmitResponse, TranscodeTaskState, VideoFormat,
};

#[test]
fn test_video_format_full_serialization() {
    let fmt = VideoFormat {
        id: 1,
        ext: "mp4".into(),
        label: Some("1080p".into()),
        type_: Some("video".into()),
        vcodec: Some("av1_nvenc".into()),
        acodec: Some("aac".into()),
        preset: Some("p4".into()),
        profile: None,
        ch: Some(2),
        vf: Some("scale=1920:1080".into()),
        b_v: Some("5M".into()),
        ar: Some("48k".into()),
        b_a: None,
        c_a: None,
        minrate: Some("4M".into()),
        maxrate: Some("6M".into()),
        bufsize: Some("12M".into()),
        gpu: Some(true),
        compression_level: None,
        dest: Some("s5".into()),
        encrypt: Some(false),
        trim_percent: None,
    };
    let json = serde_json::to_value(&fmt).unwrap();
    assert_eq!(json["id"], 1);
    assert_eq!(json["ext"], "mp4");
    assert_eq!(json["type"], "video");
    assert_eq!(json["b_v"], "5M");
    assert_eq!(json["vcodec"], "av1_nvenc");
    assert!(json.get("profile").is_none());
}

#[test]
fn test_video_format_minimal_serialization() {
    let fmt = VideoFormat {
        id: 2,
        ext: "webm".into(),
        label: None,
        type_: None,
        vcodec: None,
        acodec: None,
        preset: None,
        profile: None,
        ch: None,
        vf: None,
        b_v: None,
        ar: None,
        b_a: None,
        c_a: None,
        minrate: None,
        maxrate: None,
        bufsize: None,
        gpu: None,
        compression_level: None,
        dest: None,
        encrypt: None,
        trim_percent: None,
    };
    let json = serde_json::to_value(&fmt).unwrap();
    assert_eq!(json["id"], 2);
    assert_eq!(json["ext"], "webm");
    assert!(json.get("vcodec").is_none());
    assert!(json.get("dest").is_none());
}

#[test]
fn test_video_format_deserialize_from_transcoder_json() {
    let raw = r#"{
        "id": 1, "ext": "mp4", "type": "video",
        "vcodec": "h264_nvenc", "acodec": "aac",
        "preset": "fast", "vf": "scale=1280:720",
        "b_v": "2M", "ar": "48k", "b_a": "129k",
        "dest": "s5", "encrypt": true
    }"#;
    let fmt: VideoFormat = serde_json::from_str(raw).unwrap();
    assert_eq!(fmt.id, 1);
    assert_eq!(fmt.type_, Some("video".into()));
    assert_eq!(fmt.b_v, Some("2M".into()));
    assert_eq!(fmt.ar, Some("48k".into()));
    assert_eq!(fmt.b_a, Some("129k".into()));
    assert_eq!(fmt.encrypt, Some(true));
    assert!(fmt.profile.is_none());
}

#[test]
fn test_submit_response_deserialize() {
    let raw = r#"{"status_code":200,"message":"ok","task_id":"abc123"}"#;
    let resp: TranscodeSubmitResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.task_id, "abc123");
}

#[test]
fn test_status_response_deserialize() {
    let raw = r#"{"status_code":200,"metadata":"[]","progress":50,"duration":120.5}"#;
    let resp: TranscodeStatusResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(resp.progress, 50);
    assert_eq!(resp.duration, Some(120.5));
}

#[test]
fn test_status_response_deserialize_no_duration() {
    let raw = r#"{"status_code":200,"metadata":"[]","progress":25}"#;
    let resp: TranscodeStatusResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(resp.progress, 25);
    assert_eq!(resp.duration, None);
}

#[test]
fn test_transcode_task_state_variants() {
    let pending = TranscodeTaskState::Pending;
    let in_progress = TranscodeTaskState::InProgress { progress: 42 };
    let completed = TranscodeTaskState::Completed {
        metadata: serde_json::json!([{"id": 1, "ext": "mp4", "cid": "uEiBk"}]),
        duration: 60.0,
    };
    let failed = TranscodeTaskState::Failed {
        error: "timeout".into(),
    };

    assert!(matches!(pending, TranscodeTaskState::Pending));
    assert!(matches!(
        in_progress,
        TranscodeTaskState::InProgress { progress: 42 }
    ));
    if let TranscodeTaskState::Completed { metadata, duration } = &completed {
        assert_eq!(metadata.as_array().unwrap().len(), 1);
        assert_eq!(*duration, 60.0);
    } else {
        panic!("expected Completed");
    }
    assert!(matches!(failed, TranscodeTaskState::Failed { .. }));
}
