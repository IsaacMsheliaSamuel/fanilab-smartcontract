#!/bin/bash

# FaniLab Smart Contracts - Single Contract Initialization Script
# Usage: ./initialize-contract.sh <contract_name> [network]

set -e

CONTRACT_NAME=$1
NETWORK=${2:-testnet}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CONTRACT_IDS_FILE="$PROJECT_ROOT/contract-ids-$NETWORK.json"

SUPPORTED_CONTRACTS=("escrow_contract" "delivery_contract")

if [ -z "$CONTRACT_NAME" ]; then
    echo "❌ Usage: $0 <contract_name> [network]"
    echo "   Supported contract names: ${SUPPORTED_CONTRACTS[*]}"
    exit 1
fi

echo "🔧 Initializing FaniLab Smart Contract"
echo "======================================"
echo "Contract: $CONTRACT_NAME"
echo "Network: $NETWORK"
echo ""

# Check if contract IDs file exists
if [ ! -f "$CONTRACT_IDS_FILE" ]; then
    echo "❌ Contract IDs file not found: $CONTRACT_IDS_FILE"
    echo "Please deploy the contract first using deploy-contract.sh or deploy-all-contracts.sh"
    exit 1
fi

# Load environment variables
if [ -f "$PROJECT_ROOT/.env" ]; then
    source "$PROJECT_ROOT/.env"
else
    echo "⚠️  No .env file found. Using defaults."
fi

get_contract_id() {
    grep -o "\"$1\": *\"[^\"]*\"" "$CONTRACT_IDS_FILE" | grep -o '"[^"]*"$' | tr -d '"'
}

case "$CONTRACT_NAME" in
    escrow_contract)
        ESCROW_ID=$(get_contract_id "escrow_contract")
        if [ -z "$ESCROW_ID" ]; then
            echo "❌ escrow_contract ID not found in $CONTRACT_IDS_FILE"
            exit 1
        fi
        if [ -z "$ADMIN_ADDRESS" ] || [ -z "$TOKEN_ADDRESS" ]; then
            echo "❌ ADMIN_ADDRESS and TOKEN_ADDRESS must be set (in .env or the environment)"
            exit 1
        fi

        echo "Escrow Contract: $ESCROW_ID"
        echo ""
        echo "Initializing Escrow Contract..."
        stellar contract invoke \
            --id "$ESCROW_ID" \
            --source deployer \
            --network "$NETWORK" \
            -- init \
            --admin "$ADMIN_ADDRESS" \
            --token "$TOKEN_ADDRESS" \
            --platform_fee_bps "${PLATFORM_FEE_BPS:-250}"

        echo "✓ Escrow initialized"
        ;;
    delivery_contract)
        DELIVERY_ID=$(get_contract_id "delivery_contract")
        ESCROW_ID=$(get_contract_id "escrow_contract")
        if [ -z "$DELIVERY_ID" ]; then
            echo "❌ delivery_contract ID not found in $CONTRACT_IDS_FILE"
            exit 1
        fi
        if [ -z "$ESCROW_ID" ]; then
            echo "❌ escrow_contract must be deployed first (its ID was not found in $CONTRACT_IDS_FILE)"
            exit 1
        fi
        if [ -z "$ADMIN_ADDRESS" ]; then
            echo "❌ ADMIN_ADDRESS must be set (in .env or the environment)"
            exit 1
        fi

        echo "Delivery Contract: $DELIVERY_ID"
        echo "Escrow Contract: $ESCROW_ID"
        echo ""
        echo "Initializing Delivery Contract..."
        stellar contract invoke \
            --id "$DELIVERY_ID" \
            --source deployer \
            --network "$NETWORK" \
            -- init \
            --admin "$ADMIN_ADDRESS" \
            --escrow_contract "$ESCROW_ID"

        echo "✓ Delivery initialized"
        ;;
    *)
        echo "❌ Initialization for '$CONTRACT_NAME' is not yet supported by this script."
        echo "   Supported contract names: ${SUPPORTED_CONTRACTS[*]}"
        exit 1
        ;;
esac

echo ""
echo "✅ $CONTRACT_NAME initialized successfully!"
exit 0
