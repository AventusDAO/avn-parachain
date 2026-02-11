// This file is part of Aventus.
// Copyright 2026 Aventus DAO Ltd

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

//! token manager pallet benchmarking.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use codec::{Decode, Encode};
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::{pallet_prelude::BlockNumberFor, EventRecord, RawOrigin};
use hex_literal::hex;
use sp_core::{sr25519, ByteArray, H256};
use sp_runtime::RuntimeAppPublic;

use sp_application_crypto::KeyTypeId;
pub const BENCH_KEY_TYPE_ID: KeyTypeId = KeyTypeId(*b"test");
mod app_sr25519 {
    use super::BENCH_KEY_TYPE_ID;
    use sp_application_crypto::{app_crypto, sr25519};
    app_crypto!(sr25519, BENCH_KEY_TYPE_ID);
}
use sp_avn_common::{benchmarking::convert_sr25519_signature, eth::concat_lower_data};

type SignerId = app_sr25519::Public;

pub const AVT_TOKEN_CONTRACT: H160 = H160(hex!("dB1Cff52f66195f0a5Bd3db91137db98cfc54AE6"));

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
    assert_last_nth_event::<T>(generic_event, 1);
}

fn assert_last_nth_event<T: Config>(generic_event: <T as Config>::RuntimeEvent, n: u32) {
    let events = frame_system::Pallet::<T>::events();
    let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len().saturating_sub(n as usize)];
    assert_eq!(event, &system_event);
}

struct Transfer<T: Config> {
    relayer: T::AccountId,
    from: T::AccountId,
    to: T::AccountId,
    token_id: T::TokenId,
    amount: T::TokenBalance,
    nonce: u64,
}

impl<T: Config> Transfer<T> {
    fn new(token_id: H160) -> Self {
        let mnemonic: &str =
            "news slush supreme milk chapter athlete soap sausage put clutch what kitten";
        let key_pair = SignerId::generate_pair(Some(mnemonic.as_bytes().to_vec()));
        let account_id = T::AccountId::decode(&mut Encode::encode(&key_pair).as_slice()).unwrap();

        let index = 2;
        let seed = 2;
        return Transfer {
            relayer: whitelisted_caller(),
            from: account_id,
            to: account("to", index, seed),
            token_id: token_id.into(),
            amount: 1000u32.into(),
            nonce: 0,
        }
    }

    fn setup(self) -> Self {
        Balances::<T>::insert((self.token_id, self.from.clone()), self.amount);
        Nonces::<T>::insert(self.from.clone(), self.nonce);
        return self
    }

    fn generate_signed_transfer_call(&self, signature: &[u8]) -> <T as Config>::RuntimeCall {
        let proof: Proof<T::Signature, T::AccountId> = self.get_proof(&self.relayer, signature);
        return Call::signed_transfer {
            proof,
            from: self.from.clone(),
            to: self.to.clone(),
            token_id: self.token_id,
            amount: self.amount,
        }
        .into()
    }

    fn get_proof(
        &self,
        relayer: &T::AccountId,
        signature: &[u8],
    ) -> Proof<T::Signature, T::AccountId> {
        let signature = sr25519::Signature::from_slice(signature).expect("valid sr25519 signature");
        return Proof {
            signer: self.from.clone(),
            relayer: relayer.clone(),
            signature: convert_sr25519_signature::<T::Signature>(signature),
        }
    }
}

struct Lower<T: Config> {
    from_account_id: T::AccountId,
    lower_account: H256,
    lower_account_id: T::AccountId,
    amount: u32,
    non_avt_token_id: T::TokenId,
    t1_recipient: H160,
    lower_id: u32,
}

impl<T: Config> Lower<T> {
    fn new() -> Self {
        let mnemonic: &str =
            "news slush supreme milk chapter athlete soap sausage put clutch what kitten";
        let key_pair = SignerId::generate_pair(Some(mnemonic.as_bytes().to_vec()));
        let from_account_id =
            T::AccountId::decode(&mut Encode::encode(&key_pair).as_slice()).unwrap();
        let lower_account: H256 =
            H256(hex!("000000000000000000000000000000000000000000000000000000000000dead"));
        let lower_account_id =
            T::AccountId::decode(&mut lower_account.as_bytes()).expect("valid lower account id");
        let non_avt_token_id: T::TokenId =
            H160(hex!("1414141414141414141414141414141414141414")).into();
        let t1_recipient: H160 = H160(hex!("afdf36201bf70F1232111b5c6a9a424558755134"));

        let lower_id = 0;

        Lower {
            from_account_id,
            lower_account,
            lower_account_id,
            amount: 1000,
            non_avt_token_id,
            t1_recipient,
            lower_id,
        }
    }

    fn setup(self) -> Self {
        // setup AVT token contract
        <AVTTokenContract<T>>::put(AVT_TOKEN_CONTRACT);

        // setup non avt balance
        let lower_amount: T::TokenBalance = self.amount.into();
        Balances::<T>::insert((self.non_avt_token_id, self.from_account_id.clone()), lower_amount);

        // setup avt balance
        <T as pallet::Config>::Currency::make_free_balance_be(
            &self.from_account_id,
            self.amount.into(),
        );

        // setup lower account id
        <LowerAccountId<T>>::put(self.lower_account);

        self
    }

    fn get_proof(
        &self,
        relayer_account_id: &T::AccountId,
        signature: &[u8],
    ) -> Proof<T::Signature, T::AccountId> {
        return Proof {
            signer: self.from_account_id.clone(),
            relayer: relayer_account_id.clone(),
            signature: T::Signature::decode(&mut &signature[..]).unwrap(),
        }
    }
}

benchmarks! {
    proxy_with_non_avt_token {
        let signature = &hex!("a6350211fcdf1d7f0c79bf0a9c296de17449ca88a899f0cd19a70b07513fc107b7d34249dba71d4761ceeec2ed6bc1305defeb96418e6869e6b6199ed0de558e");
        let token_id = H160(hex!("1414141414141414141414141414141414141414"));
        let transfer: Transfer<T> = Transfer::new(token_id).setup();
        let call: <T as Config>::RuntimeCall = transfer.generate_signed_transfer_call(signature);
        let boxed_call: Box<<T as Config>::RuntimeCall> = Box::new(call);
        let call_hash: T::Hash = T::Hashing::hash_of(&boxed_call);
    }: proxy(RawOrigin::<T::AccountId>::Signed(transfer.relayer.clone()), boxed_call)
    verify {
        assert_eq!(Balances::<T>::get((transfer.token_id, transfer.from.clone())), 0u32.into());
        assert_eq!(Balances::<T>::get((transfer.token_id, transfer.to.clone())), transfer.amount);
        assert_eq!(Nonces::<T>::get(transfer.from.clone()), transfer.nonce + 1);
        assert_eq!(Nonces::<T>::get(transfer.to.clone()), 0);

        assert_last_event::<T>(Event::<T>::CallDispatched{ relayer: transfer.relayer.clone(), call_hash: call_hash }.into());
        assert_last_nth_event::<T>(Event::<T>::TokenTransferred {
            token_id: transfer.token_id.clone(),
            sender: transfer.from.clone(),
            recipient: transfer.to.clone(),
            token_balance: transfer.amount
        }.into(), 2);
    }

    signed_transfer {
        let signature = &hex!("a875c83f0709276ffd87bf401d1563bd8bcabcfda24ebb51170b72d4cd5edd6e3816f56712fa4df421260447ff483f69bcdb5a55f6356c3ffedace7fee12288e");
        let token_id = H160(hex!("1414141414141414141414141414141414141414"));
        let transfer: Transfer<T> = Transfer::new(token_id).setup();
        let proof: Proof<T::Signature, T::AccountId> = transfer.get_proof(&transfer.from, signature);
    }: _ (
            RawOrigin::<T::AccountId>::Signed(transfer.from.clone()),
            proof,
            transfer.from.clone(),
            transfer.to.clone(),
            transfer.token_id,
            transfer.amount
        )
    verify {
        assert_eq!(Balances::<T>::get((transfer.token_id, transfer.from.clone())), 0u32.into());
        assert_eq!(Balances::<T>::get((transfer.token_id, transfer.to.clone())), transfer.amount);
        assert_eq!(Nonces::<T>::get(transfer.from.clone()), transfer.nonce + 1);
        assert_eq!(Nonces::<T>::get(transfer.to.clone()), 0);

        assert_last_event::<T>(Event::<T>::TokenTransferred {
            token_id: transfer.token_id.clone(),
            sender: transfer.from.clone(),
            recipient: transfer.to.clone(),
            token_balance: transfer.amount
        }.into());
    }

    schedule_direct_lower {
        let lower: Lower<T> = Lower::new().setup();
    }: schedule_direct_lower(
        RawOrigin::<T::AccountId>::Signed(lower.from_account_id.clone()),
        lower.from_account_id.clone(),
        AVT_TOKEN_CONTRACT.into(),
        lower.amount.into(),
        lower.t1_recipient
    )
    verify {
        assert_eq!(<T as pallet::Config>::Currency::free_balance(&lower.from_account_id), 1000u32.into());
    }

    execute_avt_lower {
        let lower: Lower<T> = Lower::new().setup();
    }: execute_lower(
        RawOrigin::<T::AccountId>::Root,
        lower.from_account_id.clone(),
        lower.lower_account_id.clone(),
        AVT_TOKEN_CONTRACT.into(),
        lower.amount.into(),
        lower.t1_recipient,
        lower.lower_id
    )
    verify {
        assert_eq!(<T as pallet::Config>::Currency::free_balance(&lower.from_account_id), 0u32.into());
        assert_last_nth_event::<T>(Event::<T>::AvtLowered {
            sender: lower.from_account_id,
            recipient: lower.lower_account_id,
            amount: lower.amount.into(),
            t1_recipient: lower.t1_recipient,
            lower_id: lower.lower_id
        }.into(), 2);
    }

    execute_non_avt_lower {
        let lower: Lower<T> = Lower::new().setup();
    }: execute_lower(
        RawOrigin::<T::AccountId>::Root,
        lower.from_account_id.clone(),
        lower.lower_account_id.clone(),
        lower.non_avt_token_id,
        lower.amount.into(),
        lower.t1_recipient,
        lower.lower_id
    )
    verify {
        assert_eq!(Balances::<T>::get((lower.non_avt_token_id, lower.from_account_id.clone())), 0u32.into());
        assert_last_nth_event::<T>(Event::<T>::TokenLowered {
            token_id: lower.non_avt_token_id,
            sender: lower.from_account_id,
            recipient: lower.lower_account_id,
            amount: lower.amount.into(),
            t1_recipient: lower.t1_recipient,
            lower_id: lower.lower_id
        }.into(), 2);
    }

    schedule_signed_lower {
        let signature = &hex!("32620d56eb6272109a32ddafe132e7d7932ac210a16de25f016aa15845cb43738d4fcdaaa23be0025a8eb164779e14c46ec8c3d37e093e6017c1b59f8c450c8d");
        let lower: Lower<T> = Lower::new().setup();
        let proof: Proof<T::Signature, T::AccountId> = lower.get_proof(&lower.from_account_id, signature);
    }: schedule_signed_lower(
        RawOrigin::<T::AccountId>::Signed(lower.from_account_id.clone()),
        proof,
        lower.from_account_id.clone(),
        AVT_TOKEN_CONTRACT.into(),
        lower.amount.into(),
        lower.t1_recipient
    )
    verify {
        assert_eq!(<T as pallet::Config>::Currency::free_balance(&lower.from_account_id), 1000u32.into());
        assert_last_event::<T>(
            Event::<T>::LowerRequested {
                token_id: AVT_TOKEN_CONTRACT.into(),
                from: lower.from_account_id.clone(),
                amount: lower.amount.into(),
                t1_recipient: lower.t1_recipient,
                sender_nonce: Some(0),
                schedule_name: ("Lower", &lower.lower_id).using_encoded(sp_io::hashing::blake2_256),
                lower_id: lower.lower_id,
            }.into()
        );
    }

    transfer_from_treasury {
        let treasury_account = Pallet::<T>::compute_treasury_account_id();
        let amount = 10u32;
        let recipient = account("recipient", 1, 1);

        <T as pallet::Config>::Currency::make_free_balance_be(&treasury_account, (amount * 2u32).into());
        assert_eq!(<T as pallet::Config>::Currency::free_balance(&recipient), 0u32.into());
    }: _(RawOrigin::Root, recipient.clone(), amount.into())
    verify {
        assert_eq!(<T as pallet::Config>::Currency::free_balance(&treasury_account), amount.into());
        assert_eq!(<T as pallet::Config>::Currency::free_balance(&recipient), amount.into());
    }

    regenerate_lower_proof {
        let lower: Lower<T> = Lower::new().setup();
        let token_id = H160(hex_literal::hex!("97d9b397189e8b771ffac3cb04cf26c780a93431"));
        let t2_sender: H256 = H256::from(T::AccountToBytesConvert::into_bytes(&lower.from_account_id));
        let t2_timestamp: u64 = T::TimeProvider::now().as_secs();
        let params = concat_lower_data(
            lower.lower_id,
            token_id.into(),
            &lower.amount.into(),
            &lower.t1_recipient,
            t2_sender,
            t2_timestamp,
        );

        let lower_data = vec![0u8; 32];

        let lower_proof_data = LowerProofData {
            params,
            encoded_lower_data: BoundedVec::<u8, LowerDataLimit>::try_from(lower_data).expect("test"),
        };

        <LowersReadyToClaim<T>>::insert(lower.lower_id, lower_proof_data);
    }: _(RawOrigin::<T::AccountId>::Signed(lower.from_account_id.clone()), lower.lower_id)
    verify {
        assert!(<LowersPendingProof<T>>::contains_key(lower.lower_id));
        assert_last_event::<T>(Event::<T>::RegeneratingLowerProof { lower_id: lower.lower_id, requester: lower.from_account_id }.into());
    }

    set_lower_schedule_period {
        let new_period: BlockNumberFor<T> = 100u32.into();
    }: _(RawOrigin::Root, new_period)
    verify {
        assert_eq!(LowerSchedulePeriod::<T>::get(), new_period);
    }

    toggle_lowering {
        let lower_enabled = false;
    }: _(RawOrigin::Root, lower_enabled)
    verify {
        assert_eq!(LowersDisabled::<T>::get(), true);
        assert_last_event::<T>(Event::<T>::LoweringDisabled.into());
    }

    set_native_token_eth_address {
        let new_address = H160(hex_literal::hex!("dadB0d80178819F2319190D340ce9A924f783711"));
    }: _(RawOrigin::Root, new_address)
    verify {
        assert_eq!(AVTTokenContract::<T>::get(), new_address);
    }
}

impl_benchmark_test_suite!(
    Pallet,
    crate::mock::ExtBuilder::build_default().as_externality(),
    crate::mock::TestRuntime,
);
