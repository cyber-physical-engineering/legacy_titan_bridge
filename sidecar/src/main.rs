//! # Legacy Titan Bridge: the sidecar
//!
//! A demo web service. It takes a bank transfer as JSON, scores it with a
//! small risk policy, runs it through a COBOL routine over FFI, and adds the
//! transfer's SHA-256 hash to a Merkle tree. `/commit-batch` submits the
//! tree's root to the `Anchor.sol` contract when an Ethereum node is reachable.
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  COBOL routine  │────▶│  Rust sidecar    │────▶│   Ethereum      │
//! │  (core_banking) │ FFI │  (policy+Merkle) │ RPC │  (Anchor.sol)   │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//! ```

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod blockchain;
mod ffi;
mod merkle;
mod policy;

use merkle::MerkleAccumulator;
use policy::{PolicyEngine, RiskLevel};

// ============================================================================
// Application State
// ============================================================================

/// Shared application state across all handlers
#[derive(Clone)]
pub struct AppState {
    /// Merkle tree accumulator for batching transactions
    merkle: Arc<MerkleAccumulator>,
    /// Policy engine for risk assessment
    policy: Arc<PolicyEngine>,
    /// Blockchain client for Ethereum integration
    blockchain: Arc<blockchain::EthereumClient>,
    /// COBOL FFI interface (optional - disabled if library not found)
    cobol: Option<Arc<ffi::CobolBridge>>,
}

// ============================================================================
// API Request/Response Types
// ============================================================================

/// Transfer request from API clients
#[derive(Debug, Deserialize)]
pub struct TransferRequest {
    /// Unique transaction identifier
    pub transaction_id: String,
    /// Transfer amount in dollars
    pub amount: f64,
    /// Source account identifier
    pub from_account: String,
    /// Destination account identifier
    pub to_account: String,
}

/// Transfer response indicating pending proof status
#[derive(Debug, Serialize)]
pub struct TransferResponse {
    pub status: String,
    pub transaction_id: String,
    pub risk_level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_reason: Option<String>,
    pub tree_position: usize,
    pub cobol_status: String,
    pub cobol_message: String,
    pub tx_hash: String,
}

/// Batch commit response after Merkle root submission
#[derive(Debug, Serialize)]
pub struct CommitResponse {
    pub status: String,
    pub merkle_root: String,
    pub leaf_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub cobol_available: bool,
    pub blockchain_connected: bool,
}

/// Tree status response
#[derive(Debug, Serialize)]
pub struct TreeStatusResponse {
    pub leaf_count: usize,
    pub pending_commits: usize,
    pub last_root: Option<String>,
    pub last_commit_time: Option<String>,
}

// ============================================================================
// API Handlers
// ============================================================================

/// POST /transfer - Process a transaction through COBOL and add to Merkle tree
async fn handle_transfer(
    State(state): State<AppState>,
    Json(request): Json<TransferRequest>,
) -> impl IntoResponse {
    info!(
        tx_id = %request.transaction_id,
        amount = %request.amount,
        "Processing transfer request"
    );

    // Step 1: Policy check
    let risk_assessment = state.policy.assess_transaction(
        &request.transaction_id,
        request.amount,
        &request.from_account,
        &request.to_account,
    );

    // Step 2: Call COBOL processor (if available)
    let (cobol_status, cobol_message) = match &state.cobol {
        Some(cobol) => {
            match cobol.process_transaction(
                request.amount,
                &request.transaction_id,
                &request.from_account,
                &request.to_account,
            ) {
                Ok(response) => (response.status_code, response.message),
                Err(e) => ("99".to_string(), format!("COBOL error: {}", e)),
            }
        }
        None => {
            // Simulate successful COBOL processing when library not available
            (
                "00".to_string(),
                "SUCCESS: Simulated COBOL processing".to_string(),
            )
        }
    };

    // Step 3: Generate transaction hash and add to Merkle tree
    let tx_hash = merkle::hash_transaction(
        &request.transaction_id,
        request.amount,
        &request.from_account,
        &request.to_account,
    );

    let tree_position = state.merkle.add_leaf(&tx_hash);

    // Step 4: Build response
    let response = TransferResponse {
        status: "pending_proof".to_string(),
        transaction_id: request.transaction_id,
        risk_level: match risk_assessment.level {
            RiskLevel::Low => "LOW".to_string(),
            RiskLevel::Medium => "MEDIUM".to_string(),
            RiskLevel::High => "HIGH".to_string(),
            RiskLevel::Critical => "CRITICAL".to_string(),
        },
        risk_reason: risk_assessment.reason,
        tree_position,
        cobol_status,
        cobol_message,
        tx_hash: hex::encode(tx_hash),
    };

    (StatusCode::OK, Json(response))
}

/// POST /commit-batch - Submit current Merkle root to blockchain
async fn handle_commit_batch(State(state): State<AppState>) -> impl IntoResponse {
    info!("Processing batch commit request");

    // Get current tree state
    let (root, leaf_count) = match state.merkle.compute_root() {
        Some((root, count)) => (root, count),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CommitResponse {
                    status: "error".to_string(),
                    merkle_root: String::new(),
                    leaf_count: 0,
                    tx_hash: None,
                    gas_used: None,
                    error: Some("No transactions to commit".to_string()),
                }),
            );
        }
    };

    let root_hex = hex::encode(root);
    info!(merkle_root = %root_hex, leaf_count = %leaf_count, "Committing Merkle root");

    // Submit to blockchain
    match state.blockchain.submit_root(&root, leaf_count).await {
        Ok(receipt) => {
            // Reset tree after successful commit
            state.merkle.reset();

            (
                StatusCode::OK,
                Json(CommitResponse {
                    status: "proof_anchored".to_string(),
                    merkle_root: root_hex,
                    leaf_count,
                    tx_hash: receipt.tx_hash,
                    gas_used: receipt.gas_used,
                    error: None,
                }),
            )
        }
        Err(e) => {
            // Don't reset tree on failure - allow retry
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CommitResponse {
                    status: "error".to_string(),
                    merkle_root: root_hex,
                    leaf_count,
                    tx_hash: None,
                    gas_used: None,
                    error: Some(format!("Blockchain submission failed: {}", e)),
                }),
            )
        }
    }
}

/// GET /health - Health check endpoint
async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let blockchain_connected = state.blockchain.is_connected().await;

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        cobol_available: state.cobol.is_some(),
        blockchain_connected,
    })
}

/// GET /tree-status - Current Merkle tree status
async fn handle_tree_status(State(state): State<AppState>) -> impl IntoResponse {
    let status = state.merkle.get_status();

    Json(TreeStatusResponse {
        leaf_count: status.leaf_count,
        pending_commits: status.pending_commits,
        last_root: status.last_root.map(hex::encode),
        last_commit_time: status.last_commit_time,
    })
}

// ============================================================================
// Application Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "titan_sidecar=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting the Legacy Titan Bridge sidecar");

    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize components
    let merkle = Arc::new(MerkleAccumulator::new());
    let policy = Arc::new(PolicyEngine::new());

    // Initialize blockchain client
    let eth_rpc_url =
        std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());
    let contract_address = std::env::var("ANCHOR_CONTRACT").ok();
    let private_key = std::env::var("PRIVATE_KEY").ok();

    let blockchain = Arc::new(
        blockchain::EthereumClient::new(&eth_rpc_url, contract_address, private_key).await?,
    );

    // Try to load COBOL library
    let cobol_lib_path = std::env::var("COBOL_LIB_PATH")
        .unwrap_or_else(|_| "/usr/lib/libcorebanking.so".to_string());

    let cobol = match ffi::CobolBridge::new(&cobol_lib_path) {
        Ok(bridge) => {
            info!("COBOL library loaded from {}", cobol_lib_path);
            Some(Arc::new(bridge))
        }
        Err(e) => {
            info!(
                "COBOL library not available: {}. Transfers will return a simulated COBOL result.",
                e
            );
            None
        }
    };

    // Build application state
    let state = AppState {
        merkle,
        policy,
        blockchain,
        cobol,
    };

    // Build router
    let app = Router::new()
        .route("/transfer", post(handle_transfer))
        .route("/commit-batch", post(handle_commit_batch))
        .route("/health", get(handle_health))
        .route("/tree-status", get(handle_tree_status))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Start server
    let port = std::env::var("SIDECAR_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Listening on http://0.0.0.0:{}", port);
    info!("Endpoints:");
    info!("   POST /transfer     - Process transaction");
    info!("   POST /commit-batch - Submit Merkle root");
    info!("   GET  /health       - Health check");
    info!("   GET  /tree-status  - Tree status");

    axum::serve(listener, app).await?;

    Ok(())
}
