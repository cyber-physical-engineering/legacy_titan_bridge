//! Policy Engine for Transaction Risk Assessment
//!
//! Implements configurable risk assessment rules for financial transactions.
//! Flags high-value transactions and applies business policy rules.

use tracing::info;

/// Risk levels for transactions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    /// Normal transaction, no special handling
    Low,
    /// Elevated risk, requires monitoring
    Medium,
    /// High risk, requires review
    High,
    /// Critical risk, requires immediate attention
    Critical,
}

/// Result of a risk assessment
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RiskAssessment {
    /// Assigned risk level
    pub level: RiskLevel,
    /// Human-readable reason for the risk level
    pub reason: Option<String>,
    /// Numeric risk score (0-100)
    pub score: u8,
    /// List of triggered rules
    pub triggered_rules: Vec<String>,
}

/// Configuration for the policy engine
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PolicyConfig {
    /// Amount threshold for HIGH risk flag (default: $10,000)
    pub high_risk_threshold: f64,
    /// Amount threshold for CRITICAL risk flag (default: $100,000)
    pub critical_threshold: f64,
    /// Amount threshold for MEDIUM risk flag (default: $5,000)
    pub medium_threshold: f64,
    /// Enable velocity checks (multiple transactions in short time)
    pub enable_velocity_check: bool,
    /// Maximum transactions per minute before flagging
    pub max_tx_per_minute: u32,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            high_risk_threshold: std::env::var("RISK_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000.0),
            critical_threshold: 100_000.0,
            medium_threshold: 5_000.0,
            enable_velocity_check: true,
            max_tx_per_minute: 100,
        }
    }
}

/// Policy engine for transaction risk assessment
pub struct PolicyEngine {
    config: PolicyConfig,
}

impl PolicyEngine {
    /// Create a new policy engine with default configuration
    pub fn new() -> Self {
        Self {
            config: PolicyConfig::default(),
        }
    }

    /// Create a new policy engine with custom configuration
    #[allow(dead_code)]
    pub fn with_config(config: PolicyConfig) -> Self {
        Self { config }
    }

    /// Assess the risk of a transaction
    pub fn assess_transaction(
        &self,
        tx_id: &str,
        amount: f64,
        from_account: &str,
        to_account: &str,
    ) -> RiskAssessment {
        let mut score: u8 = 0;
        let mut triggered_rules = Vec::new();
        let mut reasons = Vec::new();

        // Rule 1: Amount thresholds
        if amount >= self.config.critical_threshold {
            score = score.saturating_add(50);
            triggered_rules.push("AMOUNT_CRITICAL".to_string());
            reasons.push(format!(
                "Amount ${:.2} exceeds critical threshold ${:.2}",
                amount, self.config.critical_threshold
            ));
        } else if amount >= self.config.high_risk_threshold {
            score = score.saturating_add(30);
            triggered_rules.push("AMOUNT_HIGH".to_string());
            reasons.push(format!(
                "Amount ${:.2} exceeds high-risk threshold ${:.2}",
                amount, self.config.high_risk_threshold
            ));
        } else if amount >= self.config.medium_threshold {
            score = score.saturating_add(15);
            triggered_rules.push("AMOUNT_MEDIUM".to_string());
        }

        // Rule 2: Round number detection (potential structuring)
        if Self::is_suspicious_round_number(amount) {
            score = score.saturating_add(10);
            triggered_rules.push("ROUND_NUMBER".to_string());
            reasons.push("Suspiciously round amount detected".to_string());
        }

        // Rule 3: Same prefix accounts (internal transfers may need scrutiny)
        if from_account.len() >= 4 && to_account.len() >= 4 && from_account[..4] == to_account[..4]
        {
            score = score.saturating_add(5);
            triggered_rules.push("SAME_PREFIX_ACCOUNTS".to_string());
        }

        // Determine risk level based on score
        let level = match score {
            0..=10 => RiskLevel::Low,
            11..=25 => RiskLevel::Medium,
            26..=50 => RiskLevel::High,
            _ => RiskLevel::Critical,
        };

        let reason = if reasons.is_empty() {
            None
        } else {
            Some(reasons.join("; "))
        };

        info!(
            tx_id = %tx_id,
            amount = %amount,
            risk_level = ?level,
            score = %score,
            "Risk assessment completed"
        );

        RiskAssessment {
            level,
            reason,
            score,
            triggered_rules,
        }
    }

    /// Check if amount is a suspicious round number (potential structuring)
    fn is_suspicious_round_number(amount: f64) -> bool {
        // Check for amounts that are exact thousands (e.g., $9,000, $9,500)
        let cents = (amount * 100.0) as i64;

        // Exact thousands
        if cents % 100_000 == 0 && amount >= 1000.0 {
            return true;
        }

        // Just under reporting thresholds (common structuring pattern)
        let just_under_10k = (9_000.0..=9_999.0).contains(&amount);
        let exact_500 = cents % 50_000 == 0;

        just_under_10k && exact_500
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_risk_transaction() {
        let engine = PolicyEngine::new();
        let assessment = engine.assess_transaction("TX-001", 500.0, "ACC-001", "ACC-002");
        assert_eq!(assessment.level, RiskLevel::Low);
    }

    #[test]
    fn test_high_risk_transaction() {
        let engine = PolicyEngine::new();
        let assessment = engine.assess_transaction("TX-002", 15_000.0, "ACC-001", "ACC-002");
        assert_eq!(assessment.level, RiskLevel::High);
        assert!(assessment.reason.is_some());
        assert!(assessment
            .triggered_rules
            .contains(&"AMOUNT_HIGH".to_string()));
    }

    #[test]
    fn test_critical_transaction() {
        let engine = PolicyEngine::new();
        let assessment = engine.assess_transaction("TX-003", 150_000.0, "ACC-001", "ACC-002");
        assert_eq!(assessment.level, RiskLevel::Critical);
    }

    #[test]
    fn test_round_number_detection() {
        assert!(PolicyEngine::is_suspicious_round_number(9_500.0));
        assert!(!PolicyEngine::is_suspicious_round_number(9_523.47));
    }
}
