#!/bin/bash
# ==============================================================================
# Legacy Titan Bridge - Interactive Demo Script
# ==============================================================================
# Demonstrates the full transaction flow: COBOL -> Rust -> Merkle -> Blockchain
# ==============================================================================

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

API_URL="${API_URL:-http://localhost:3000}"

echo ""
echo -e "${BLUE}LEGACY TITAN BRIDGE - DEMO${NC}"
echo -e "${BLUE}COBOL -> Rust sidecar -> Merkle tree -> Ethereum (when a node is up)${NC}"
echo ""

# Check if server is running
echo -e "${YELLOW}[1/5] Checking server health...${NC}"
HEALTH=$(curl -s "$API_URL/health" 2>/dev/null || echo '{"error":"not running"}')

if echo "$HEALTH" | grep -q "healthy"; then
    echo -e "${GREEN}✓ Server is healthy${NC}"
    echo "$HEALTH" | jq .
else
    echo -e "${RED}✗ Server not running. Start with: make run${NC}"
    exit 1
fi

echo ""
echo -e "${YELLOW}[2/5] Simulating low-risk transaction (\$500)...${NC}"
TX1=$(curl -s -X POST "$API_URL/transfer" \
    -H "Content-Type: application/json" \
    -d '{
        "transaction_id": "TX-DEMO-001",
        "amount": 500.00,
        "from_account": "ACCT-SAVINGS-001",
        "to_account": "ACCT-CHECKING-002"
    }')
echo "$TX1" | jq .
RISK1=$(echo "$TX1" | jq -r '.risk_level')
echo -e "${GREEN}✓ Transaction processed, Risk Level: ${RISK1}${NC}"

echo ""
echo -e "${YELLOW}[3/5] Simulating HIGH-RISK transaction (\$15,000)...${NC}"
TX2=$(curl -s -X POST "$API_URL/transfer" \
    -H "Content-Type: application/json" \
    -d '{
        "transaction_id": "TX-DEMO-002",
        "amount": 15000.00,
        "from_account": "ACCT-CORP-001",
        "to_account": "ACCT-VENDOR-003"
    }')
echo "$TX2" | jq .
RISK2=$(echo "$TX2" | jq -r '.risk_level')
echo -e "${RED}⚠ Transaction flagged, Risk Level: ${RISK2}${NC}"

echo ""
echo -e "${YELLOW}[4/5] Checking Merkle tree status...${NC}"
TREE=$(curl -s "$API_URL/tree-status")
echo "$TREE" | jq .
LEAVES=$(echo "$TREE" | jq -r '.leaf_count')
echo -e "${GREEN}✓ Merkle tree has ${LEAVES} leaves pending commit${NC}"

echo ""
echo -e "${YELLOW}[5/5] Attempting batch commit to blockchain...${NC}"
COMMIT=$(curl -s -X POST "$API_URL/commit-batch")
echo "$COMMIT" | jq .
STATUS=$(echo "$COMMIT" | jq -r '.status')

if [ "$STATUS" = "proof_anchored" ]; then
    ROOT=$(echo "$COMMIT" | jq -r '.merkle_root')
    echo -e "${GREEN}✓ Merkle root anchored to blockchain!${NC}"
    echo -e "${GREEN}  Root: ${ROOT}${NC}"
else
    echo -e "${YELLOW}! Commit returned: $STATUS${NC}"
    echo -e "${YELLOW}  (Expected if no Ethereum node configured)${NC}"
fi

echo ""
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  DEMO COMPLETE${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""
echo "What was demonstrated:"
echo "  1. ✓ Health check endpoint"
echo "  2. ✓ Low-risk transaction processing"
echo "  3. ✓ High-risk transaction flagging (>\$10,000)"
echo "  4. ✓ Merkle tree accumulation"
echo "  5. ✓ Batch commit (an error is expected with no Ethereum node)"
echo ""
echo "With an Ethereum node and a deployed contract configured, step 5 submits"
echo "the Merkle root to Anchor.sol. One root stands for the whole batch."
echo ""
