#!/bin/bash
# Update chain specs with fresh lightSyncState from RPC endpoints
#
# ONLY updates the lightSyncState field in existing specs, preserving
# all other fields (bootNodes, genesis, etc.)
#
# Note: The lightSyncState checkpoints returned by RPC nodes are embedded
# in their binaries and may not be as recent as the current chain head.
# Light clients will sync forward from the checkpoint.

set -e
cd "$(dirname "$0")"

echo "=== Chain Spec Updater ==="
echo ""

# Function to update lightSyncState in existing spec
update_light_sync_state() {
    local name=$1
    local rpc=$2
    local file=$3

    echo "Updating $name..."

    if [ ! -f "$file" ]; then
        echo "  ERROR: $file not found"
        return 1
    fi

    # Get fresh spec from RPC to extract lightSyncState
    local fresh_light_sync=$(curl -s -X POST "$rpc" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","id":1,"method":"sync_state_genSyncSpec","params":[true]}' | \
        jq -r '.result.lightSyncState')

    if [ -z "$fresh_light_sync" ] || [ "$fresh_light_sync" = "null" ]; then
        echo "  ERROR: Failed to fetch lightSyncState from $rpc"
        return 1
    fi

    # Extract checkpoint info
    local old_checkpoint=$(jq -r '.lightSyncState.babeFinalizedBlockWeight // "N/A"' "$file")
    local new_checkpoint=$(echo "$fresh_light_sync" | jq -r '.babeFinalizedBlockWeight // "N/A"')

    if [ "$old_checkpoint" = "$new_checkpoint" ]; then
        echo "  Checkpoint unchanged: $old_checkpoint"
        return 0
    fi

    echo "  Checkpoint: $old_checkpoint -> $new_checkpoint"

    # Update only lightSyncState in existing spec
    local temp_file="${file}.tmp"
    jq --argjson newLightSync "$fresh_light_sync" \
        '.lightSyncState = $newLightSync' "$file" > "$temp_file"
    mv "$temp_file" "$file"

    local size=$(wc -c < "$file" | tr -d ' ')
    echo "  Size: ${size} bytes"
    echo "  OK"
}

# Update relay chain specs
update_light_sync_state "Kusama" "https://kusama-rpc.polkadot.io" "kusama.json"
update_light_sync_state "Polkadot" "https://rpc.polkadot.io" "polkadot.json"
update_light_sync_state "Westend" "https://westend-rpc.polkadot.io" "westend.json"

# Paseo
echo ""
echo "Updating Paseo..."
update_light_sync_state "Paseo" "https://rpc.ibp.network/paseo" "paseo.json" 2>/dev/null || \
    echo "  Skipped (RPC may not support sync_state_genSyncSpec)"

echo ""
echo "=== Summary ==="
echo "Current checkpoints:"
for f in kusama.json polkadot.json westend.json paseo.json; do
    if [ -f "$f" ]; then
        checkpoint=$(jq -r '.lightSyncState.babeFinalizedBlockWeight // "N/A"' "$f" 2>/dev/null || echo "N/A")
        size=$(wc -c < "$f" 2>/dev/null | tr -d ' ' || echo "0")
        echo "  $f: block $checkpoint (${size} bytes)"
    fi
done

echo ""
echo "NOTE: Parachain specs sync via relay chain - no lightSyncState update needed."
