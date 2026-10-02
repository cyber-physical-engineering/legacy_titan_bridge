//! Ethereum Blockchain Integration
//!
//! Provides connectivity to Ethereum for submitting Merkle roots
//! to the Anchor.sol smart contract.

use anyhow::Result;
use ethers::{
    prelude::*,
    types::{Address, H256, U256},
};
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

/// Blockchain-specific errors
#[derive(Error, Debug)]
pub enum BlockchainError {
    #[error("Not connected to Ethereum node")]
    NotConnected,

    #[error("Contract not configured")]
    ContractNotConfigured,

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Provider error: {0}")]
    #[allow(dead_code)]
    ProviderError(String),
}

/// Receipt from a successful blockchain transaction
#[derive(Debug, Clone)]
pub struct TransactionReceipt {
    /// Transaction hash
    pub tx_hash: Option<String>,
    /// Gas used
    pub gas_used: Option<u64>,
    /// Block number
    #[allow(dead_code)]
    pub block_number: Option<u64>,
}

/// Ethereum client for interacting with the Anchor contract
pub struct EthereumClient {
    provider: Option<Provider<Http>>,
    contract_address: Option<Address>,
    wallet: Option<LocalWallet>,
    chain_id: u64,
}

// ABI for the Anchor contract (minimal interface)
abigen!(
    AnchorContract,
    r#"[
        function submitDailyRoot(bytes32 root, uint256 leafCount) external
        function dailyRoots(uint256 index) external view returns (bytes32)
        function getRootsCount() external view returns (uint256)
        event RootSubmitted(bytes32 indexed root, uint256 timestamp, uint256 leafCount)
    ]"#
);

impl EthereumClient {
    /// Create a new Ethereum client
    pub async fn new(
        rpc_url: &str,
        contract_address: Option<String>,
        private_key: Option<String>,
    ) -> Result<Self> {
        // Try to connect to the provider
        let provider = match Provider::<Http>::try_from(rpc_url) {
            Ok(p) => {
                // Verify connection
                match p.get_chainid().await {
                    Ok(chain_id) => {
                        info!(chain_id = %chain_id, rpc_url = %rpc_url, "Connected to Ethereum node");
                        Some(p)
                    }
                    Err(e) => {
                        warn!(
                            "Failed to connect to Ethereum node: {}. Running in offline mode.",
                            e
                        );
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Invalid RPC URL: {}. Running in offline mode.", e);
                None
            }
        };

        // Parse contract address
        let contract_addr = contract_address.and_then(|addr| {
            addr.parse::<Address>()
                .inspect_err(|&e| {
                    warn!("Invalid contract address: {}", e);
                })
                .ok()
        });

        // Parse wallet
        let wallet = private_key.and_then(|key| {
            key.parse::<LocalWallet>()
                .map_err(|e| {
                    warn!("Invalid private key: {}", e);
                    e
                })
                .ok()
        });

        // Get chain ID
        let chain_id = if let Some(ref p) = provider {
            p.get_chainid().await.unwrap_or(U256::from(1)).as_u64()
        } else {
            1 // Default to mainnet
        };

        Ok(Self {
            provider,
            contract_address: contract_addr,
            wallet,
            chain_id,
        })
    }

    /// Check if connected to an Ethereum node
    pub async fn is_connected(&self) -> bool {
        if let Some(ref provider) = self.provider {
            provider.get_chainid().await.is_ok()
        } else {
            false
        }
    }

    /// Submit a Merkle root to the Anchor contract
    pub async fn submit_root(
        &self,
        root: &[u8; 32],
        leaf_count: usize,
    ) -> Result<TransactionReceipt, BlockchainError> {
        // Check prerequisites
        let provider = self
            .provider
            .as_ref()
            .ok_or(BlockchainError::NotConnected)?;

        let contract_address = self
            .contract_address
            .ok_or(BlockchainError::ContractNotConfigured)?;

        let wallet = self
            .wallet
            .as_ref()
            .ok_or(BlockchainError::TransactionFailed(
                "No wallet configured".to_string(),
            ))?;

        info!(
            root = %hex::encode(root),
            leaf_count = %leaf_count,
            contract = %contract_address,
            "Submitting Merkle root to blockchain"
        );

        // Create signed client
        let wallet = wallet.clone().with_chain_id(self.chain_id);
        let client = SignerMiddleware::new(provider.clone(), wallet);
        let client = Arc::new(client);

        // Create contract instance
        let contract = AnchorContract::new(contract_address, client);

        // Convert root to H256
        let root_h256 = H256::from_slice(root);

        // Submit transaction - bind the call to extend lifetime
        let call = contract.submit_daily_root(root_h256.into(), U256::from(leaf_count));
        let pending_tx = call
            .send()
            .await
            .map_err(|e| BlockchainError::TransactionFailed(e.to_string()))?;

        info!(tx_hash = %pending_tx.tx_hash(), "Transaction submitted, waiting for confirmation");

        // Wait for confirmation
        let receipt = pending_tx
            .await
            .map_err(|e| BlockchainError::TransactionFailed(e.to_string()))?
            .ok_or(BlockchainError::TransactionFailed("No receipt".to_string()))?;

        info!(
            tx_hash = %receipt.transaction_hash,
            block = ?receipt.block_number,
            gas_used = ?receipt.gas_used,
            "Transaction confirmed"
        );

        Ok(TransactionReceipt {
            tx_hash: Some(format!("{:?}", receipt.transaction_hash)),
            gas_used: receipt.gas_used.map(|g| g.as_u64()),
            block_number: receipt.block_number.map(|b| b.as_u64()),
        })
    }

    /// Get the number of roots stored in the contract
    #[allow(dead_code)]
    pub async fn get_roots_count(&self) -> Result<u64, BlockchainError> {
        let provider = self
            .provider
            .as_ref()
            .ok_or(BlockchainError::NotConnected)?;

        let contract_address = self
            .contract_address
            .ok_or(BlockchainError::ContractNotConfigured)?;

        let contract = AnchorContract::new(contract_address, Arc::new(provider.clone()));

        let count = contract
            .get_roots_count()
            .call()
            .await
            .map_err(|e| BlockchainError::ProviderError(e.to_string()))?;

        Ok(count.as_u64())
    }

    /// Get a specific root by index
    #[allow(dead_code)]
    pub async fn get_root(&self, index: u64) -> Result<[u8; 32], BlockchainError> {
        let provider = self
            .provider
            .as_ref()
            .ok_or(BlockchainError::NotConnected)?;

        let contract_address = self
            .contract_address
            .ok_or(BlockchainError::ContractNotConfigured)?;

        let contract = AnchorContract::new(contract_address, Arc::new(provider.clone()));

        let root = contract
            .daily_roots(U256::from(index))
            .call()
            .await
            .map_err(|e| BlockchainError::ProviderError(e.to_string()))?;

        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_offline_client() {
        // Should create client even with invalid URL
        let client = EthereumClient::new("http://invalid-url:8545", None, None)
            .await
            .unwrap();

        assert!(!client.is_connected().await);
    }
}
