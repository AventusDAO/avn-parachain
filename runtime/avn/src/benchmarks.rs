// This file is part of Aventus.
// Copyright (C) 2026 Aventus Network Services (UK) Ltd.

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

frame_benchmarking::define_benchmarks!(
    [frame_system, SystemBench::<Runtime>]
    [pallet_assets, Assets]
    [pallet_balances, Balances]
    [pallet_avn_offence_handler, AvnOffenceHandler]
    [pallet_avn_proxy, AvnProxy]
    [pallet_avn, Avn]
    [pallet_eth_bridge, EthBridge]
    [pallet_ethereum_events, EthereumEvents]
    [pallet_nft_manager, NftManager]
    [pallet_summary, Summary]
    [pallet_token_manager, TokenManager]
    [pallet_validators_manager, ValidatorsManager]
    [pallet_avn_transaction_payment, AvnTransactionPayment]
    [pallet_node_manager, NodeManager]
    [pallet_watchtower, Watchtower]
    [pallet_session, SessionBench::<Runtime>]
    [pallet_timestamp, Timestamp]
    [pallet_message_queue, MessageQueue]
    [pallet_utility, Utility]
    [pallet_parachain_staking, ParachainStaking]
    [pallet_avn_anchor, AvnAnchor]
    [cumulus_pallet_parachain_system, ParachainSystem]
    [cumulus_pallet_xcmp_queue, XcmpQueue]
);
