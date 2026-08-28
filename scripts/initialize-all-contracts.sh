#!/bin/bash

# FaniLab Smart Contracts - Initialization Script
# Usage: ./initialize-all-contracts.sh [testnet|mainnet]

set -e

NETWORK=${1:-testnet}
# Name of the Stellar CLI identity every deploy/initialize script signs with.
# Overridable, but the default must match the identity provisioned by
# .github/workflows/deploy-testnet.yml and documented in docs/DEPLOYMENT.md.
DEPLOYER_IDENTITY="${DEPLOYER_IDENTITY:-deployer}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CONTRACT_IDS_FILE="$PROJECT_ROOT/contract-ids-$NETWORK.json"

echo "🔧 Initializing FaniLab Smart Contracts"
echo "======================================"
echo "Network: $NETWORK"
echo ""

# Check if contract IDs file exists
if [ ! -f "$CONTRACT_IDS_FILE" ]; then
    echo "❌ Contract IDs file not found: $CONTRACT_IDS_FILE"
    echo "Please deploy contracts first using deploy-all-contracts.sh"
    exit 1
fi

# Load environment variables
if [ -f "$PROJECT_ROOT/.env" ]; then
    source "$PROJECT_ROOT/.env"
else
    echo "⚠️  No .env file found. Using defaults."
fi

# Verify the signing identity exists before any invoke is attempted.
if ! stellar keys address "$DEPLOYER_IDENTITY" &> /dev/null; then
    echo "❌ Stellar CLI identity '$DEPLOYER_IDENTITY' not found."
    echo "   Every deploy/initialize script signs with it (--source $DEPLOYER_IDENTITY)."
    echo "   • CI: the 'Configure deployer identity' step in"
    echo "     .github/workflows/deploy-testnet.yml provisions it from the"
    echo "     CONTRACT_DEPLOYER_SECRET secret (environment secret"
    echo "     TESTNET_DEPLOYER_SECRET)."
    echo "   • Local: create it once with"
    echo "       stellar keys generate $DEPLOYER_IDENTITY --network $NETWORK"
    echo "     or import an existing key with"
    echo "       stellar keys add $DEPLOYER_IDENTITY --secret-key"
    exit 1
fi

# Parse contract IDs from JSON
ESCROW_ID=$(grep -o '"escrow_contract": "[^"]*' "$CONTRACT_IDS_FILE" | grep -o '[^"]*$')
DELIVERY_ID=$(grep -o '"delivery_contract": "[^"]*' "$CONTRACT_IDS_FILE" | grep -o '[^"]*$')

echo "Escrow Contract: $ESCROW_ID"
echo "Delivery Contract: $DELIVERY_ID"
echo ""

# Initialize Escrow Contract
echo "Initializing Escrow Contract..."
stellar contract invoke \
    --id "$ESCROW_ID" \
    --source "$DEPLOYER_IDENTITY" \
    --network "$NETWORK" \
    -- init \
    --admin "$ADMIN_ADDRESS" \
    --token "$TOKEN_ADDRESS" \
    --platform_fee_bps 250

echo "✓ Escrow initialized"
echo ""

# Initialize Delivery Contract
echo "Initializing Delivery Contract..."
stellar contract invoke \
    --id "$DELIVERY_ID" \
    --source "$DEPLOYER_IDENTITY" \
    --network "$NETWORK" \
    -- init \
    --admin "$ADMIN_ADDRESS" \
    --escrow_contract "$ESCROW_ID"

echo "✓ Delivery initialized"
echo ""

echo "✅ All contracts initialized successfully!"
exit 0
