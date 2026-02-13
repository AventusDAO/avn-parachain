#!/usr/bin/env bash

PALLETS=(
  pallets/authors-manager
  pallets/avn
  pallets/avn-anchor
  pallets/avn-offence-handler
  pallets/avn-proxy
  pallets/avn-transaction-payment
  pallets/cross-chain-voting
  pallets/eth-bridge
  pallets/ethereum-events
  pallets/nft-manager
  pallets/parachain-staking
  pallets/summary
  pallets/token-manager
  pallets/validators-manager
)

for p in "${PALLETS[@]}"
do
  folder=$(basename "$p")
  crate="pallet_$(echo "$folder" | tr '-' '_')"
  echo "🔧 Benchmarking $crate ..."

  target/release/avn-parachain-collator benchmark pallet \
    --chain dev \
    --pallet "$crate" \
    --extrinsic "*" \
    --steps 50 \
    --repeat 20 \
    --wasm-execution compiled \
    --heap-pages 4096 \
    --output "$p/src/default_weights.rs" \
    --template ./.maintain/frame-weight-template.hbs

  echo "✅ Done $crate"
  echo "---------------------------------------------"
done
