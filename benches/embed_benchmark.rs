// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Embedding Performance Benchmarks (Sub-phase 8.1)
//!
//! Comprehensive benchmark suite for embedding generation using Criterion.
//!
//! Benchmark Categories:
//! 1. End-to-End Performance: Single text, batches of 10 and 96
//! 2. Component Performance: Tokenization, ONNX inference, mean pooling
//! 3. Concurrency: Parallel request handling
//! 4. Memory: Model loading and request processing overhead
//!
//! Performance Targets:
//! - Single embedding: <50ms (CPU), <20ms (GPU)
//! - Batch 10: <200ms (CPU), <80ms (GPU)
//! - Batch 96: <3s (CPU), <1s (GPU)
//! - Memory: <300MB total

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use fabstir_llm_node::embeddings::OnnxEmbeddingModel;
use std::sync::Arc;
use std::sync::Once;
use tokio::runtime::Runtime;

static INIT: Once = Once::new();

/// Initialize tracing for benchmarks (only once)
fn init_tracing() {
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_target(false)
            .init();
        eprintln!("\n📊 Tracing initialized for benchmarks\n");
    });
}

// Model paths for benchmarking
const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

/// Setup helper: Create embedding model for benchmarks
fn setup_model(rt: &Runtime) -> Arc<OnnxEmbeddingModel> {
    // Initialize tracing to see CUDA logs
    init_tracing();

    rt.block_on(async {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load embedding model for benchmarks");

        Arc::new(model)
    })
}

/// Generate sample texts of various lengths
fn generate_sample_texts(count: usize, words_per_text: usize) -> Vec<String> {
    let words = vec![
        "machine",
        "learning",
        "artificial",
        "intelligence",
        "neural",
        "network",
        "deep",
        "transformer",
        "embedding",
        "vector",
        "semantic",
        "representation",
        "model",
        "training",
        "inference",
        "optimization",
        "gradient",
        "descent",
    ];

    (0..count)
        .map(|i| {
            let text: Vec<&str> = (0..words_per_text)
                .map(|j| words[(i + j) % words.len()])
                .collect();
            text.join(" ")
        })
        .collect()
}

//
// CATEGORY 1: End-to-End Performance Benchmarks
//

/// Benchmark: Single text embedding with varying text lengths
///
/// Target: <50ms (CPU), <20ms (GPU)
/// Tests text lengths: 10, 50, 200 words
fn bench_single_embedding(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let mut group = c.benchmark_group("single_embedding");

    for words in [10, 50, 200].iter() {
        let texts = generate_sample_texts(1, *words);
        let text = &texts[0];

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_words", words)),
            text,
            |b, text| {
                b.iter(|| {
                    rt.block_on(async {
                        let result = model.embed(black_box(text)).await;
                        assert!(result.is_ok());
                        result.unwrap()
                    })
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Batch of 10 texts
///
/// Target: <200ms (CPU), <80ms (GPU)
fn bench_batch_10_embeddings(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let texts = generate_sample_texts(10, 50);

    c.bench_function("batch_10_embeddings", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = model.embed_batch(black_box(&texts)).await;
                assert!(result.is_ok());
                let embeddings = result.unwrap();
                assert_eq!(embeddings.len(), 10);
                embeddings
            })
        });
    });
}

/// Benchmark: Maximum batch size (96 texts)
///
/// Target: <3s (CPU), <1s (GPU)
fn bench_batch_96_embeddings(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let texts = generate_sample_texts(96, 50);

    c.bench_function("batch_96_embeddings", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = model.embed_batch(black_box(&texts)).await;
                assert!(result.is_ok());
                let embeddings = result.unwrap();
                assert_eq!(embeddings.len(), 96);
                embeddings
            })
        });
    });
}

//
// CATEGORY 2: Component-Level Benchmarks
//

/// Benchmark: Tokenization only
///
/// Isolates tokenizer.encode() performance
fn bench_tokenization(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let text = "Machine learning is a subset of artificial intelligence that focuses on training algorithms";

    c.bench_function("tokenization", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Use count_tokens which only does tokenization
                let result = model.count_tokens(black_box(text)).await;
                assert!(result.is_ok());
                result.unwrap()
            })
        });
    });
}

/// Benchmark: Full inference pipeline (tokenization + ONNX + pooling)
///
/// Measures end-to-end embedding generation for component analysis
fn bench_inference_pipeline(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let texts = generate_sample_texts(1, 50);
    let text = &texts[0];

    c.bench_function("inference_pipeline", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = model.embed(black_box(text)).await;
                assert!(result.is_ok());
                let embedding = result.unwrap();
                assert_eq!(embedding.len(), 384);
                embedding
            })
        });
    });
}

//
// CATEGORY 3: Concurrency Benchmarks
//

/// Benchmark: Concurrent requests (10 parallel)
///
/// Tests thread-safe access to ONNX model
/// Measures throughput under parallel load
fn bench_concurrent_10_requests(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let texts = generate_sample_texts(10, 50);

    c.bench_function("concurrent_10_requests", |b| {
        b.iter(|| {
            rt.block_on(async {
                let model = Arc::clone(&model);
                let mut handles = vec![];

                for text in &texts {
                    let model = Arc::clone(&model);
                    let text = text.clone();
                    let handle = tokio::spawn(async move { model.embed(&text).await.unwrap() });
                    handles.push(handle);
                }

                let results: Vec<_> = futures::future::join_all(handles)
                    .await
                    .into_iter()
                    .map(|r| r.unwrap())
                    .collect();

                assert_eq!(results.len(), 10);
                results
            })
        });
    });
}

/// Benchmark: Concurrent requests (50 parallel)
///
/// Stress test with higher concurrency
fn bench_concurrent_50_requests(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let texts = generate_sample_texts(50, 30);

    c.bench_function("concurrent_50_requests", |b| {
        b.iter(|| {
            rt.block_on(async {
                let model = Arc::clone(&model);
                let mut handles = vec![];

                for text in &texts {
                    let model = Arc::clone(&model);
                    let text = text.clone();
                    let handle = tokio::spawn(async move { model.embed(&text).await.unwrap() });
                    handles.push(handle);
                }

                let results: Vec<_> = futures::future::join_all(handles)
                    .await
                    .into_iter()
                    .map(|r| r.unwrap())
                    .collect();

                assert_eq!(results.len(), 50);
                results
            })
        });
    });
}

//
// CATEGORY 4: Batch Size Scaling
//

/// Benchmark: Batch size scaling from 1 to 96
///
/// Measures how performance scales with batch size
fn bench_batch_size_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let mut group = c.benchmark_group("batch_size_scaling");

    for batch_size in [1, 5, 10, 20, 50, 96].iter() {
        let texts = generate_sample_texts(*batch_size, 50);

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &texts,
            |b, texts| {
                b.iter(|| {
                    rt.block_on(async {
                        let result = model.embed_batch(black_box(texts)).await;
                        assert!(result.is_ok());
                        let embeddings = result.unwrap();
                        assert_eq!(embeddings.len(), *batch_size);
                        embeddings
                    })
                });
            },
        );
    }

    group.finish();
}

//
// CATEGORY 5: Text Length Scaling
//

/// Benchmark: Text length scaling from 10 to 512 tokens
///
/// Measures how performance scales with input text length
/// Note: Model has 128 token limit, but tests various lengths to see truncation impact
fn bench_text_length_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let model = setup_model(&rt);

    let mut group = c.benchmark_group("text_length_scaling");

    for word_count in [10, 25, 50, 100, 200, 400].iter() {
        let texts = generate_sample_texts(1, *word_count);
        let text = &texts[0];

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_words", word_count)),
            text,
            |b, text| {
                b.iter(|| {
                    rt.block_on(async {
                        let result = model.embed(black_box(text)).await;
                        assert!(result.is_ok());
                        let embedding = result.unwrap();
                        assert_eq!(embedding.len(), 384);
                        embedding
                    })
                });
            },
        );
    }

    group.finish();
}

//
// Criterion Configuration
//

criterion_group!(
    benches,
    bench_single_embedding,
    bench_batch_10_embeddings,
    bench_batch_96_embeddings,
    bench_tokenization,
    bench_inference_pipeline,
    bench_concurrent_10_requests,
    bench_concurrent_50_requests,
    bench_batch_size_scaling,
    bench_text_length_scaling,
);

criterion_main!(benches);
