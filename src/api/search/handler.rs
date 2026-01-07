// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Search API endpoint handler

use axum::{extract::State, http::StatusCode, Json};
use tracing::{debug, info, warn};

use super::request::SearchApiRequest;
use super::response::SearchApiResponse;
use crate::api::http_server::AppState;
use crate::search::SearchError;

/// POST /v1/search - Perform web search
///
/// # Request
/// - `query`: Search query string (required, max 500 chars)
/// - `numResults`: Number of results (1-20, default 10)
/// - `chainId`: Chain ID for billing (default 84532)
/// - `requestId`: Optional request ID for tracking
///
/// # Response
/// - `query`: Original search query
/// - `results`: Array of search results with title, url, snippet
/// - `resultCount`: Number of results returned
/// - `searchTimeMs`: Time taken for search
/// - `provider`: Search provider used
/// - `cached`: Whether result was from cache
/// - `chainId`, `chainName`: Chain context
///
/// # Errors
/// - 400 Bad Request: Invalid query or parameters
/// - 429 Too Many Requests: Rate limited
/// - 503 Service Unavailable: Search disabled or no providers
/// - 500 Internal Server Error: Search failed
pub async fn search_handler(
    State(state): State<AppState>,
    Json(request): Json<SearchApiRequest>,
) -> Result<Json<SearchApiResponse>, (StatusCode, String)> {
    debug!("Search request: {:?}", request.query);

    // Validate request
    if let Err(e) = request.validate() {
        warn!("Search validation failed: {}", e);
        return Err((StatusCode::BAD_REQUEST, e));
    }

    // Get search service from state
    let search_service = state.search_service.read().await;
    let search_service = search_service.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Search service not available".to_string(),
        )
    })?;

    if !search_service.is_enabled() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Web search is disabled on this host".to_string(),
        ));
    }

    // Perform search
    let result = search_service
        .search(&request.query, Some(request.num_results))
        .await
        .map_err(|e| match &e {
            SearchError::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, e.to_string()),
            SearchError::SearchDisabled => (StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
            SearchError::InvalidQuery { .. } => (StatusCode::BAD_REQUEST, e.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    info!(
        "Search complete: {} results for '{}' in {}ms (cached: {})",
        result.result_count, request.query, result.search_time_ms, result.cached
    );

    Ok(Json(SearchApiResponse::new(
        result.query,
        result.results,
        result.search_time_ms,
        result.provider,
        result.cached,
        request.chain_id,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_exists() {
        // Verify the handler compiles
        let _ = search_handler;
    }
}
