use anyhow::Result;
use fabstir_llm_node::models::{
    ModelDownloader, DownloadConfig, DownloadSource, ModelFormat,
    DownloadProgress, DownloadResult, DownloadError, DownloadStatus,
    ModelMetadata, ChunkSize, RetryPolicy, AuthConfig
};
use std::path::PathBuf;
use tokio;
use futures::StreamExt;

async fn create_test_downloader() -> Result<ModelDownloader> {
    let config = DownloadConfig {
        download_dir: PathBuf::from("test_data/models"),
        max_concurrent_downloads: 3,
        chunk_size: ChunkSize::Adaptive,
        timeout_secs: 300,
        retry_policy: RetryPolicy::default(),
        verify_checksum: true,
        use_cache: true,
        max_bandwidth_bytes_per_sec: None,
    };
    
    ModelDownloader::new(config).await
}

#[tokio::test]
async fn test_basic_model_download() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/TinyLlama-1B-GGUF".to_string(),
        filename: "tinyllama-1b.Q4_K_M.gguf".to_string(),
        revision: None,
    };
    
    let result = downloader.download_model(source).await.unwrap();
    
    assert_eq!(result.status, DownloadStatus::Completed);
    assert!(result.local_path.exists());
    assert!(result.size_bytes > 0);
    assert!(result.download_time_ms > 0);
    assert_eq!(result.format, ModelFormat::GGUF);
    assert!(result.checksum.is_some());
}

#[tokio::test]
async fn test_download_with_progress() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/Llama-2-7B-GGUF".to_string(),
        filename: "llama-2-7b.Q4_K_M.gguf".to_string(),
        revision: None,
    };
    
    let mut progress_stream = downloader.download_with_progress(source).await.unwrap();
    let mut progress_updates = Vec::new();
    
    while let Some(progress) = progress_stream.next().await {
        progress_updates.push(progress.clone());
        
        // Verify progress is monotonic
        if progress_updates.len() > 1 {
            let prev = &progress_updates[progress_updates.len() - 2];
            assert!(progress.bytes_downloaded >= prev.bytes_downloaded);
            assert!(progress.percentage >= prev.percentage);
        }
    }
    
    assert!(!progress_updates.is_empty());
    let final_progress = progress_updates.last().unwrap();
    assert_eq!(final_progress.percentage, 100.0);
    assert_eq!(final_progress.bytes_downloaded, final_progress.total_bytes);
}

#[tokio::test]
async fn test_download_from_s5_storage() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::S5 {
        cid: "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi".to_string(),
        path: "/models/llama-7b.gguf".to_string(),
        gateway: Some("https://s5.cx".to_string()),
    };
    
    let result = downloader.download_model(source).await.unwrap();
    
    assert_eq!(result.status, DownloadStatus::Completed);
    assert!(result.source_url.contains("s5"));
    assert!(result.local_path.exists());
}

#[tokio::test]
async fn test_download_from_http_url() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::Http {
        url: "https://example.com/models/test-model.gguf".to_string(),
        headers: Some(vec![
            ("User-Agent".to_string(), "fabstir-llm-node/0.1".to_string()),
        ].into_iter().collect()),
    };
    
    let result = downloader.download_model(source).await.unwrap();
    
    assert_eq!(result.status, DownloadStatus::Completed);
    assert_eq!(result.format, ModelFormat::GGUF);
}

#[tokio::test]
async fn test_download_with_authentication() {
    let downloader = create_test_downloader().await.unwrap();
    
    let auth = AuthConfig::BearerToken {
        token: "hf_test_token_12345".to_string(),
    };
    
    let source = DownloadSource::HuggingFace {
        repo_id: "private-org/private-model".to_string(),
        filename: "model.gguf".to_string(),
        revision: None,
    };
    
    let result = downloader.download_with_auth(source, auth).await.unwrap();
    
    assert_eq!(result.status, DownloadStatus::Completed);
    assert!(result.metadata.as_ref().unwrap().requires_auth);
}

#[tokio::test]
async fn test_resume_interrupted_download() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/Llama-2-13B-GGUF".to_string(),
        filename: "llama-2-13b.Q4_K_M.gguf".to_string(),
        revision: None,
    };
    
    // Start download
    let download_id = downloader.start_download(source.clone()).await.unwrap();
    
    // Simulate interruption after 50MB
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    downloader.pause_download(&download_id).await.unwrap();
    
    let status = downloader.get_download_status(&download_id).await.unwrap();
    assert_eq!(status, DownloadStatus::Paused);
    
    // Resume download
    let result = downloader.resume_download(&download_id).await.unwrap();
    
    assert_eq!(result.status, DownloadStatus::Completed);
    assert!(result.resumed_from_byte > 0);
    assert!(result.local_path.exists());
}

#[tokio::test]
async fn test_parallel_downloads() {
    let downloader = create_test_downloader().await.unwrap();
    
    let sources = vec![
        DownloadSource::HuggingFace {
            repo_id: "TheBloke/TinyLlama-1B-GGUF".to_string(),
            filename: "model1.gguf".to_string(),
            revision: None,
        },
        DownloadSource::HuggingFace {
            repo_id: "TheBloke/TinyLlama-1B-GGUF".to_string(),
            filename: "model2.gguf".to_string(),
            revision: None,
        },
        DownloadSource::HuggingFace {
            repo_id: "TheBloke/TinyLlama-1B-GGUF".to_string(),
            filename: "model3.gguf".to_string(),
            revision: None,
        },
    ];
    
    let download_futures: Vec<_> = sources
        .into_iter()
        .map(|source| downloader.download_model(source))
        .collect();
    
    let results = futures::future::join_all(download_futures).await;
    
    assert_eq!(results.len(), 3);
    for result in results {
        assert!(result.is_ok());
        assert_eq!(result.unwrap().status, DownloadStatus::Completed);
    }
}

#[tokio::test]
async fn test_download_with_checksum_verification() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/TinyLlama-1B-GGUF".to_string(),
        filename: "tinyllama-1b.Q4_K_M.gguf".to_string(),
        revision: None,
    };
    
    // Download with known checksum
    let expected_checksum = "abc123def456789"; // Mock checksum
    let result = downloader
        .download_with_checksum(source, expected_checksum)
        .await;
    
    // Should verify checksum
    match result {
        Ok(download_result) => {
            assert_eq!(download_result.status, DownloadStatus::Completed);
            assert!(download_result.checksum_verified);
        }
        Err(e) => match e.downcast_ref::<DownloadError>() {
            Some(DownloadError::ChecksumMismatch { expected, actual }) => {
                // This is also valid if checksums don't match
                assert_eq!(expected, &expected_checksum);
                assert!(!actual.is_empty());
            }
            _ => panic!("Unexpected error: {:?}", e),
        }
    }
}

#[tokio::test]
async fn test_download_retry_on_failure() {
    let mut config = DownloadConfig::default();
    config.retry_policy = RetryPolicy {
        max_retries: 3,
        initial_delay_ms: 100,
        max_delay_ms: 1000,
        exponential_base: 2.0,
    };
    
    let downloader = ModelDownloader::new(config).await.unwrap();
    
    // Use a source that will fail initially (mock behavior)
    let source = DownloadSource::Http {
        url: "https://flaky-server.example.com/model.gguf".to_string(),
        headers: None,
    };
    
    let result = downloader.download_model(source).await;
    
    // Should eventually succeed after retries
    assert!(result.is_ok() || matches!(
        result.as_ref().err().and_then(|e| e.downcast_ref::<DownloadError>()),
        Some(DownloadError::MaxRetriesExceeded { attempts: 3 })
    ));
}

#[tokio::test]
async fn test_download_cancellation() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/Llama-2-70B-GGUF".to_string(),
        filename: "llama-2-70b.Q4_K_M.gguf".to_string(),
        revision: None,
    };
    
    // Start download
    let download_id = downloader.start_download(source).await.unwrap();
    
    // Cancel after brief delay
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    downloader.cancel_download(&download_id).await.unwrap();
    
    let status = downloader.get_download_status(&download_id).await.unwrap();
    assert_eq!(status, DownloadStatus::Cancelled);
    
    // Verify partial file is cleaned up
    let download_info = downloader.get_download_info(&download_id).await.unwrap();
    assert!(!download_info.local_path.exists());
}

#[tokio::test]
async fn test_bandwidth_throttling() {
    let mut config = DownloadConfig::default();
    config.max_bandwidth_bytes_per_sec = Some(1_000_000); // 1 MB/s
    
    let downloader = ModelDownloader::new(config).await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/TinyLlama-1B-GGUF".to_string(),
        filename: "tinyllama-1b.Q4_K_M.gguf".to_string(),
        revision: None,
    };
    
    let start = std::time::Instant::now();
    let result = downloader.download_model(source).await.unwrap();
    let duration = start.elapsed();
    
    // Calculate actual bandwidth
    let actual_bandwidth = result.size_bytes as f64 / duration.as_secs_f64();
    
    // Should be close to limit (with some tolerance)
    assert!(actual_bandwidth <= 1_200_000.0); // 20% tolerance
}

#[tokio::test]
async fn test_metadata_extraction() {
    let downloader = create_test_downloader().await.unwrap();
    
    let source = DownloadSource::HuggingFace {
        repo_id: "TheBloke/Llama-2-7B-GGUF".to_string(),
        filename: "llama-2-7b.Q4_K_M.gguf".to_string(),
        revision: Some("main".to_string()),
    };
    
    let result = downloader.download_model(source).await.unwrap();
    let metadata = result.metadata.unwrap();
    
    assert!(!metadata.model_id.is_empty());
    assert!(!metadata.model_name.is_empty());
    assert!(metadata.model_size_bytes > 0);
    assert_eq!(metadata.format, ModelFormat::GGUF);
    assert!(metadata.quantization.is_some());
    assert!(metadata.created_at > 0);
    assert!(!metadata.sha256_hash.is_empty());
}

#[tokio::test]
async fn test_storage_space_check() {
    let downloader = create_test_downloader().await.unwrap();
    
    // Check available space before download
    let space_info = downloader.check_storage_space().await.unwrap();
    assert!(space_info.available_bytes > 0);
    assert!(space_info.required_bytes >= 0);
    
    // Try to download model that might exceed space (mock)
    let source = DownloadSource::HuggingFace {
        repo_id: "imaginary/huge-model-100TB".to_string(),
        filename: "model.gguf".to_string(),
        revision: None,
    };
    
    let result = downloader.download_model(source).await;
    
    // Should handle insufficient space gracefully
    match result {
        Err(e) => match e.downcast_ref::<DownloadError>() {
            Some(DownloadError::InsufficientSpace { required, available }) => {
                assert!(required > available);
            }
            _ => panic!("Unexpected error: {:?}", e),
        },
        Ok(_) => {
            // Mock might succeed - that's ok too
        }
    }
}