//! COBOL FFI bridge
//!
//! Rust bindings to the COBOL `core_banking.cbl` routine, loaded at run time
//! from a shared library. The call itself is `unsafe`: raw pointers cross into
//! COBOL, and the GnuCOBOL runtime is not thread-safe, so calls are serialized.

use anyhow::Result;
use libloading::{Library, Symbol};
use std::os::raw::{c_char, c_int};
use std::sync::Mutex;
use thiserror::Error;

/// GnuCOBOL exports `ENTRY "PROCESS-TX"` as the C symbol `PROCESS__TX`:
/// a hyphen in a COBOL name becomes a double underscore.
const PROCESS_TX_SYMBOL: &[u8] = b"PROCESS__TX\0";

/// The GnuCOBOL runtime must be initialized once before any COBOL entry point runs.
const COB_INIT_SYMBOL: &[u8] = b"cob_init\0";

type ProcessTxFn = unsafe extern "C" fn(*mut CobolRequest, *mut CobolResponse) -> c_int;
type CobInitFn = unsafe extern "C" fn(c_int, *mut *mut c_char);

/// COBOL FFI errors
#[derive(Error, Debug)]
pub enum CobolError {
    #[error("Failed to load COBOL library: {0}")]
    LibraryLoad(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Transaction processing failed: {0}")]
    ProcessingFailed(String),

    #[error("Invalid response from COBOL: {0}")]
    #[allow(dead_code)]
    InvalidResponse(String),
}

/// Request structure matching COBOL LS-TX-REQUEST layout
#[repr(C, packed)]
pub struct CobolRequest {
    /// Transaction amount (PIC 9(12)V99) - stored as i64 cents
    pub amount: [u8; 14],
    /// Transaction ID (PIC X(32))
    pub tx_id: [u8; 32],
    /// From account (PIC X(16))
    pub from_account: [u8; 16],
    /// To account (PIC X(16))
    pub to_account: [u8; 16],
}

/// Response structure matching COBOL LS-TX-RESPONSE layout
#[repr(C, packed)]
pub struct CobolResponse {
    /// Status code (PIC XX)
    pub status_code: [u8; 2],
    /// Status message (PIC X(80))
    pub status_message: [u8; 80],
    /// Processed timestamp (PIC X(26))
    pub timestamp: [u8; 26],
    /// Transaction hash (PIC X(64))
    pub tx_hash: [u8; 64],
}

/// Parsed response from COBOL processor
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TransactionResponse {
    pub status_code: String,
    pub message: String,
    pub timestamp: String,
    pub tx_hash: String,
}

/// Bridge to COBOL core_banking library
pub struct CobolBridge {
    _library: Library,
    /// The GnuCOBOL runtime is not thread-safe, so calls are serialized.
    call_lock: Mutex<()>,
}

impl CobolBridge {
    /// Create a new COBOL bridge by loading the shared library
    pub fn new(lib_path: &str) -> Result<Self, CobolError> {
        // Attempt to load the COBOL shared library
        let library = unsafe {
            Library::new(lib_path)
                .map_err(|e| CobolError::LibraryLoad(format!("{}: {}", lib_path, e)))?
        };

        unsafe {
            // Verify the PROCESS-TX entry point exists
            let _: Symbol<ProcessTxFn> = library
                .get(PROCESS_TX_SYMBOL)
                .map_err(|e| CobolError::SymbolNotFound(format!("PROCESS__TX: {}", e)))?;

            // Start the GnuCOBOL runtime before the first call
            let cob_init: Symbol<CobInitFn> = library
                .get(COB_INIT_SYMBOL)
                .map_err(|e| CobolError::SymbolNotFound(format!("cob_init: {}", e)))?;
            cob_init(0, std::ptr::null_mut());
        }

        Ok(Self {
            _library: library,
            call_lock: Mutex::new(()),
        })
    }

    /// Process a transaction through the COBOL processor
    pub fn process_transaction(
        &self,
        amount: f64,
        tx_id: &str,
        from_account: &str,
        to_account: &str,
    ) -> Result<TransactionResponse, CobolError> {
        // Prepare request structure
        let mut request = CobolRequest {
            amount: [b'0'; 14],
            tx_id: [b' '; 32],
            from_account: [b' '; 16],
            to_account: [b' '; 16],
        };

        // Format amount as COBOL decimal (remove decimal point, pad with zeros)
        let amount_cents = (amount * 100.0) as i64;
        let amount_str = format!("{:014}", amount_cents);
        request.amount.copy_from_slice(amount_str.as_bytes());

        // Copy string fields (left-justified, space-padded)
        Self::copy_to_fixed(&mut request.tx_id, tx_id);
        Self::copy_to_fixed(&mut request.from_account, from_account);
        Self::copy_to_fixed(&mut request.to_account, to_account);

        // Prepare response structure
        let mut response = CobolResponse {
            status_code: [b'9'; 2],
            status_message: [b' '; 80],
            timestamp: [b' '; 26],
            tx_hash: [b'0'; 64],
        };

        // Call COBOL processor, one call at a time
        {
            let _guard = self
                .call_lock
                .lock()
                .map_err(|_| CobolError::ProcessingFailed("COBOL call lock poisoned".into()))?;
            unsafe {
                let func: Symbol<ProcessTxFn> = self
                    ._library
                    .get(PROCESS_TX_SYMBOL)
                    .map_err(|e| CobolError::SymbolNotFound(format!("PROCESS__TX: {}", e)))?;

                func(
                    &mut request as *mut CobolRequest,
                    &mut response as *mut CobolResponse,
                );
            }
        }

        // Parse response
        Ok(TransactionResponse {
            status_code: Self::parse_fixed(&response.status_code),
            message: Self::parse_fixed(&response.status_message),
            timestamp: Self::parse_fixed(&response.timestamp),
            tx_hash: Self::parse_fixed(&response.tx_hash),
        })
    }

    /// Copy a Rust string to a fixed-size COBOL field (left-justified, space-padded)
    fn copy_to_fixed(dest: &mut [u8], src: &str) {
        let bytes = src.as_bytes();
        let len = std::cmp::min(bytes.len(), dest.len());
        dest[..len].copy_from_slice(&bytes[..len]);
        // Remaining bytes already space-padded from initialization
    }

    /// Parse a fixed-size COBOL field to a trimmed Rust string
    fn parse_fixed(src: &[u8]) -> String {
        String::from_utf8_lossy(src).trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_to_fixed() {
        let mut field = [b' '; 16];
        CobolBridge::copy_to_fixed(&mut field, "ACC-001");
        assert_eq!(&field[..7], b"ACC-001");
        assert_eq!(&field[7..], b"         "); // 9 spaces
    }

    #[test]
    fn test_parse_fixed() {
        let field = *b"Hello World     "; // 16 chars, space-padded
        let result = CobolBridge::parse_fixed(&field);
        assert_eq!(result, "Hello World");
    }
}
