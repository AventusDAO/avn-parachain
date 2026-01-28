#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
pub mod default_weights;
pub use default_weights::WeightInfo;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    #[cfg(not(feature = "std"))]
    extern crate alloc;

    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    use frame_support::{pallet_prelude::*, weights::WeightMeter};
    use frame_system::{
        offchain::{SendTransactionTypes, SubmitTransaction},
        pallet_prelude::*,
    };
    use log;
    use pallet_avn::{self as avn, Error as avn_error};
    use pallet_timestamp as timestamp;
    use scale_info::prelude::{format, string::String, vec, vec::Vec};
    use serde_json::Value;
    use sp_avn_common::event_types::Validator;
    use sp_runtime::{traits::Saturating, DispatchError, RuntimeAppPublic};

    const PALLET_NAME: &'static [u8] = b"AvnOracle";
    pub const PRICE_SUBMISSION_CONTEXT: &'static [u8] = b"update_price_signing_context";
    pub const CLEAR_CONSENSUS_SUBMISSION_CONTEXT: &'static [u8] =
        b"clear_consensus_signing_context";
    const BATCH_PER_STORAGE: usize = 6;
    pub const MAX_DELETE_ATTEMPTS: u32 = 5;
    pub const MAX_CURRENCY_LENGTH: u32 = 4;
    pub const MAX_RATES: u32 = 10;

    pub type AVN<T> = avn::Pallet<T>;
    pub type Currency = BoundedVec<u8, ConstU32<{ MAX_CURRENCY_LENGTH }>>;
    pub type Rates = BoundedVec<(Currency, u128), ConstU32<{ MAX_RATES }>>;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    #[pallet::getter(fn voting_round_id)]
    pub type VotingRoundId<T> = StorageValue<_, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn price_submission_timestamps)]
    pub type PriceSubmissionTimestamps<T: Config> =
        StorageMap<_, Blake2_128Concat, u32, (u64, u64), OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn price_reporters)]
    pub type PriceReporters<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, u32, Blake2_128Concat, T::AccountId, (), ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn last_price_submission)]
    pub type LastPriceSubmission<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn last_cleared_voting_round_ids)]
    pub type LastClearedVotingRoundIds<T: Config> = StorageValue<_, (u32, u32), OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn processed_voting_round_ids)]
    pub type ProcessedVotingRoundIds<T: Config> = StorageValue<_, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn reported_rates)]
    pub type ReportedRates<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, u32, Blake2_128Concat, Rates, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn native_token_rate_by_currency)]
    pub type NativeTokenRateByCurrency<T: Config> =
        StorageMap<_, Blake2_128Concat, Currency, u128, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn currency_symbols)]
    pub type Currencies<T: Config> = StorageMap<_, Blake2_128Concat, Currency, (), OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn rates_refresh_range)]
    pub type RatesRefreshRangeBlocks<T> =
        StorageValue<_, u32, ValueQuery, DefaultRatesRefreshRange<T>>;

    #[pallet::type_value]
    pub fn DefaultRatesRefreshRange<T: Config>() -> u32 {
        T::MinRatesRefreshRange::get()
    }

    #[pallet::config]
    pub trait Config:
        SendTransactionTypes<Call<Self>>
        + frame_system::Config
        + pallet_avn::Config
        + timestamp::Config
    {
        /// The overarching runtime event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// A type representing the weights required by the dispatchables of this pallet.
        type WeightInfo: WeightInfo;

        /// Grace period for consensus
        #[pallet::constant]
        type ConsensusGracePeriod: Get<u32>;

        /// Maximum number of currencies
        #[pallet::constant]
        type MaxCurrencies: Get<u32>;

        /// Minimum Rates Refresh range
        #[pallet::constant]
        type MinRatesRefreshRange: Get<u32>;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        RatesUpdated { rates: Rates, round_id: u32 },
        ConsensusCleared { period: u32 },
        CurrencyRegistered { currency: Vec<u8> },
        CurrencyRemoved { currency: Vec<u8> },
        RatesRefreshRangeUpdated { old: u32, new: u32 },
    }

    #[pallet::error]
    pub enum Error<T> {
        SubmitterNotAValidator,
        ErrorSigning,
        ErrorSubmittingTransaction,
        ErrorFetchingPrice,
        ValidatorAlreadySubmitted,
        PriceMustBeGreaterThanZero,
        InvalidRateFormat,
        MissingPriceTimestamps,
        InvalidCurrency,
        TooManyCurrencies,
        GracePeriodNotPassed,
        CurrencyNotFound,
        TooManyRates,
        UnregisteredCurrency,
        RateRangeTooLow,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::submit_price())]
        pub fn submit_price(
            origin: OriginFor<T>,
            rates: Rates,
            submitter: Validator<T::AuthorityId, T::AccountId>,
            _signature: <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;
            ensure!(
                AVN::<T>::is_validator(&submitter.account_id),
                Error::<T>::SubmitterNotAValidator
            );

            for (currency, _) in rates.iter() {
                ensure!(Currencies::<T>::contains_key(currency), Error::<T>::UnregisteredCurrency);
            }

            let round_id = VotingRoundId::<T>::get();
            ensure!(
                !PriceReporters::<T>::contains_key(round_id, &submitter.account_id),
                Error::<T>::ValidatorAlreadySubmitted
            );
            PriceReporters::<T>::insert(round_id, &submitter.account_id, ());

            let count = ReportedRates::<T>::mutate(round_id, &rates, |count| {
                *count = count.saturating_add(1);
                *count
            });

            if count > AVN::<T>::quorum() {
                log::info!("🎁 Quorum reached: {}, proceeding to publish rates", count);
                Self::deposit_event(Event::<T>::RatesUpdated { rates: rates.clone(), round_id });

                for (currency, value) in rates.iter() {
                    NativeTokenRateByCurrency::<T>::insert(currency, value.clone());
                }

                ProcessedVotingRoundIds::<T>::put(round_id);
                LastPriceSubmission::<T>::put(<frame_system::Pallet<T>>::block_number());
                VotingRoundId::<T>::mutate(|value| *value += 1);
            }
            Ok(().into())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::clear_consensus())]
        pub fn clear_consensus(
            origin: OriginFor<T>,
            submitter: Validator<T::AuthorityId, T::AccountId>,
            _signature: <<T as avn::Config>::AuthorityId as RuntimeAppPublic>::Signature,
        ) -> DispatchResultWithPostInfo {
            ensure_none(origin)?;

            ensure!(
                AVN::<T>::is_validator(&submitter.account_id),
                Error::<T>::SubmitterNotAValidator
            );

            let current_block = <frame_system::Pallet<T>>::block_number();
            let last_submission_block = LastPriceSubmission::<T>::get();

            let required_block = last_submission_block
                .saturating_add(BlockNumberFor::<T>::from(RatesRefreshRangeBlocks::<T>::get()))
                .saturating_add(BlockNumberFor::<T>::from(T::ConsensusGracePeriod::get()));

            ensure!(current_block >= required_block, Error::<T>::GracePeriodNotPassed);

            let new_last_submission_block = current_block
                .saturating_sub(BlockNumberFor::<T>::from(RatesRefreshRangeBlocks::<T>::get()));
            LastPriceSubmission::<T>::put(new_last_submission_block);

            let cleared_period = VotingRoundId::<T>::get();
            VotingRoundId::<T>::mutate(|value| *value += 1);

            Self::deposit_event(Event::<T>::ConsensusCleared { period: cleared_period });
            Ok(().into())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::register_currency(T::MaxCurrencies::get()))]
        pub fn register_currency(
            origin: OriginFor<T>,
            currency_symbol: Vec<u8>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            let current_count = Currencies::<T>::iter().count() as u32;
            ensure!(current_count < T::MaxCurrencies::get(), Error::<T>::TooManyCurrencies);

            let currency = Self::to_currency(currency_symbol.clone())?;
            Currencies::<T>::insert(currency.clone(), ());

            Self::deposit_event(Event::<T>::CurrencyRegistered { currency: currency_symbol });

            let final_weight = <T as pallet::Config>::WeightInfo::register_currency(current_count);
            Ok(Some(final_weight).into())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::remove_currency())]
        pub fn remove_currency(
            origin: OriginFor<T>,
            currency_symbol: Vec<u8>,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;

            let currency = Self::to_currency(currency_symbol.clone())?;
            ensure!(Currencies::<T>::contains_key(&currency), Error::<T>::CurrencyNotFound);
            Currencies::<T>::remove(&currency);

            Self::deposit_event(Event::<T>::CurrencyRemoved { currency: currency_symbol });
            Ok(().into())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_rates_refresh_range())]
        pub fn set_rates_refresh_range(
            origin: OriginFor<T>,
            new_value: u32,
        ) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;
            ensure!(new_value >= T::MinRatesRefreshRange::get(), Error::<T>::RateRangeTooLow);

            let old = RatesRefreshRangeBlocks::<T>::get();
            RatesRefreshRangeBlocks::<T>::put(new_value);
            Self::deposit_event(Event::<T>::RatesRefreshRangeUpdated { old, new: new_value });
            Ok(().into())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            let mut total_weight = Weight::zero();

            let last_submission_block = LastPriceSubmission::<T>::get();
            let round_id = VotingRoundId::<T>::get();
            if Self::is_refresh_due(n, last_submission_block) &&
                !PriceSubmissionTimestamps::<T>::contains_key(round_id)
            {
                let now = pallet_timestamp::Pallet::<T>::now();
                let now_u64: u64 = now.try_into().unwrap_or_default();
                let now_secs = now_u64 / 1000;
                let two_minutes_secs = 120;

                // we do this to ensure all data for the given period is available and the data is
                // consistent
                let to = now_secs.saturating_sub(two_minutes_secs);

                // 10 minutes
                let ten_minutes_secs = 600;
                let from = to.saturating_sub(ten_minutes_secs);
                PriceSubmissionTimestamps::<T>::insert(round_id, (from, to));

                total_weight = total_weight.saturating_add(
                    <T as pallet::Config>::WeightInfo::on_initialize_updates_rates_query_timestamps(
                    ),
                );
            }

            total_weight.saturating_add(<T as pallet::Config>::WeightInfo::on_initialize_without_updating_rates_query_timestamps());
            total_weight
        }

        fn offchain_worker(block_number: BlockNumberFor<T>) {
            log::info!(
                "Vow prices manager OCW -> 🚧 🚧 Running offchain worker for block: {:?}",
                block_number
            );

            let setup_result = AVN::<T>::pre_run_setup(block_number, PALLET_NAME.to_vec());
            if let Err(e) = setup_result {
                match e {
                    _ if e == DispatchError::from(avn_error::<T>::OffchainWorkerAlreadyRun) => {
                        ();
                    },
                    _ => {
                        log::error!("💔 Unable to run offchain worker: {:?}", e);
                    },
                };
                return
            }
            let (this_validator, _) = setup_result.expect("We have a validator");

            let _ = Self::submit_price_if_required(&this_validator);
            let _ = Self::clear_consensus_if_required(&this_validator, block_number);
        }

        fn on_idle(_now: BlockNumberFor<T>, limit: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(limit / 2);
            let min_on_idle_weight =
                <T as pallet::Config>::WeightInfo::on_idle_one_full_iteration();

            if !meter.can_consume(min_on_idle_weight) {
                log::debug!("⚠️ Not enough weight to proceed with cleanup.");
                return meter.consumed();
            }

            let (mut price_reporters_round_id, mut prices_round_id) =
                LastClearedVotingRoundIds::<T>::get().unwrap_or((0, 0));

            let max_vow_price_round_id_to_delete = ProcessedVotingRoundIds::<T>::get();

            // Exit early if we've already caught up
            if price_reporters_round_id >= max_vow_price_round_id_to_delete &&
                prices_round_id >= max_vow_price_round_id_to_delete
            {
                return meter.consumed();
            }

            for _n in 0..MAX_DELETE_ATTEMPTS {
                if !meter.can_consume(min_on_idle_weight) {
                    break;
                }

                if price_reporters_round_id < max_vow_price_round_id_to_delete {
                    let cleared = PriceReporters::<T>::drain_prefix(price_reporters_round_id)
                        .take(BATCH_PER_STORAGE)
                        .count();
                    if cleared < BATCH_PER_STORAGE {
                        price_reporters_round_id += 1;
                    }
                }

                if prices_round_id < max_vow_price_round_id_to_delete {
                    let cleared = ReportedRates::<T>::drain_prefix(prices_round_id)
                        .take(BATCH_PER_STORAGE)
                        .count();
                    if cleared < BATCH_PER_STORAGE {
                        prices_round_id += 1;
                    }
                }

                meter.consume(min_on_idle_weight);
            }

            LastClearedVotingRoundIds::<T>::put((price_reporters_round_id, prices_round_id));
            meter.consumed()
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;
        fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            match source {
                TransactionSource::Local | TransactionSource::InBlock => { /* allowed */ },
                _ => return InvalidTransaction::Call.into(),
            }

            match call {
                Call::submit_price { rates, submitter, signature } =>
                    if AVN::<T>::signature_is_valid(
                        &(PRICE_SUBMISSION_CONTEXT, rates, VotingRoundId::<T>::get()),
                        &submitter,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("SubmitAvtPrice")
                            .and_provides(vec![(
                                PRICE_SUBMISSION_CONTEXT,
                                rates,
                                VotingRoundId::<T>::get(),
                                submitter.account_id.clone(),
                            )
                                .encode()])
                            .priority(TransactionPriority::max_value())
                            .longevity(64_u64)
                            .propagate(false)
                            .build()
                    } else {
                        InvalidTransaction::Custom(1u8).into()
                    },
                Call::clear_consensus { submitter, signature } =>
                    if AVN::<T>::signature_is_valid(
                        &(CLEAR_CONSENSUS_SUBMISSION_CONTEXT, VotingRoundId::<T>::get()),
                        &submitter,
                        signature,
                    ) {
                        ValidTransaction::with_tag_prefix("ClearConsensus")
                            .and_provides(vec![(
                                CLEAR_CONSENSUS_SUBMISSION_CONTEXT,
                                VotingRoundId::<T>::get(),
                                submitter.account_id.clone(),
                            )
                                .encode()])
                            .priority(TransactionPriority::max_value())
                            .longevity(64_u64)
                            .propagate(false)
                            .build()
                    } else {
                        InvalidTransaction::Custom(1u8).into()
                    },
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    impl<T: Config> Pallet<T> {
        fn submit_price_if_required(
            submitter: &Validator<T::AuthorityId, T::AccountId>,
        ) -> Result<(), DispatchError> {
            if !Self::should_query_rates() {
                return Ok(());
            }

            let current_block = <frame_system::Pallet<T>>::block_number();
            let last_submission_block = LastPriceSubmission::<T>::get();
            let round_id = VotingRoundId::<T>::get();

            let guard_lock_name =
                Self::create_guard_lock(b"submit_price::", round_id, &submitter.account_id);

            if Self::is_refresh_due(current_block, last_submission_block) {
                let mut lock = AVN::<T>::get_ocw_locker(&guard_lock_name);
                if let Ok(guard) = lock.try_lock() {
                    let rates = Self::fetch_and_decode_rates()?;
                    let signature = submitter
                        .key
                        .sign(&(PRICE_SUBMISSION_CONTEXT, rates.clone(), round_id).encode())
                        .ok_or(Error::<T>::ErrorSigning)?;

                    let _ = SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(
                        Call::submit_price { rates, submitter: submitter.clone(), signature }
                            .into(),
                    )
                    .map_err(|_| Error::<T>::ErrorSubmittingTransaction);

                    guard.forget();
                };
            }

            Ok(())
        }

        fn clear_consensus_if_required(
            submitter: &Validator<T::AuthorityId, T::AccountId>,
            current_block: BlockNumberFor<T>,
        ) -> Result<(), DispatchError> {
            if !Self::should_query_rates() {
                return Ok(());
            }

            let last_submission_block = LastPriceSubmission::<T>::get();

            if Self::can_clear(current_block, last_submission_block) {
                let signature = submitter
                    .key
                    .sign(&(CLEAR_CONSENSUS_SUBMISSION_CONTEXT, VotingRoundId::<T>::get()).encode())
                    .ok_or(Error::<T>::ErrorSigning)?;

                let _ = SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(
                    Call::clear_consensus { submitter: submitter.clone(), signature }.into(),
                )
                .map_err(|_| Error::<T>::ErrorSubmittingTransaction);
            }
            Ok(())
        }

        pub fn create_guard_lock<BlockNumber: Encode>(
            prefix: &'static [u8],
            block_number: BlockNumber,
            authority: &T::AccountId,
        ) -> Vec<u8> {
            let mut name = prefix.to_vec();
            name.extend_from_slice(&block_number.encode());
            name.extend_from_slice(&authority.encode());
            name
        }

        fn fetch_and_decode_rates() -> Result<Rates, DispatchError> {
            let stored_currencies: Vec<String> = Currencies::<T>::iter_keys()
                .map(|s| {
                    core::str::from_utf8(&s.as_slice())
                        .map(|v| v.to_string())
                        .map_err(|_| Error::<T>::InvalidCurrency.into())
                })
                .collect::<Result<Vec<_>, DispatchError>>()?;

            let round_id = VotingRoundId::<T>::get();
            let (from, to) = PriceSubmissionTimestamps::<T>::get(round_id)
                .ok_or(Error::<T>::MissingPriceTimestamps)?;

            let endpoint = format!(
                "/get_token_rates/aventus/{}/{}/{}",
                stored_currencies.join(","),
                from,
                to,
            );
            let response = AVN::<T>::get_data_from_service(endpoint)
                .map_err(|_| Error::<T>::ErrorFetchingPrice)?;

            let formatted = Self::format_rates(response);
            log::info!("✅ Formatted Rates: {:?}", formatted);

            formatted
        }

        pub fn format_rates(prices_json: Vec<u8>) -> Result<Rates, DispatchError> {
            let prices: Value = serde_json::from_slice(&prices_json)
                .map_err(|_| DispatchError::Other("JSON Parsing Error"))?;

            let mut formatted_rates: Vec<(Currency, u128)> = Vec::new();

            if let Some(rates) = prices.as_object() {
                for (currency_symbol, rate_value) in rates {
                    let rate = rate_value.as_f64().ok_or(Error::<T>::InvalidRateFormat)?;
                    ensure!(rate > 0.0, Error::<T>::PriceMustBeGreaterThanZero);

                    let scaled_rate = (rate * 1e8) as u128;
                    let currency = Self::to_currency(currency_symbol.as_bytes().to_vec())?;
                    formatted_rates.push((currency, scaled_rate));
                }
            }
            Self::to_rates(formatted_rates)
        }

        pub fn should_query_rates() -> bool {
            Currencies::<T>::iter_keys().next().is_some()
        }

        fn is_refresh_due(current: BlockNumberFor<T>, last: BlockNumberFor<T>) -> bool {
            current >=
                last.saturating_add(BlockNumberFor::<T>::from(
                    RatesRefreshRangeBlocks::<T>::get(),
                ))
        }

        fn can_clear(current: BlockNumberFor<T>, last: BlockNumberFor<T>) -> bool {
            current >=
                last.saturating_add(BlockNumberFor::<T>::from(
                    RatesRefreshRangeBlocks::<T>::get(),
                ))
                .saturating_add(BlockNumberFor::<T>::from(T::ConsensusGracePeriod::get()))
        }

        pub fn to_currency(currency_symbol: Vec<u8>) -> Result<Currency, DispatchError> {
            let currency =
                BoundedVec::<u8, ConstU32<{ MAX_CURRENCY_LENGTH }>>::try_from(currency_symbol)
                    .map_err(|_| DispatchError::from(Error::<T>::InvalidCurrency))?;
            Ok(currency)
        }

        fn to_rates(rates: Vec<(Currency, u128)>) -> Result<Rates, DispatchError> {
            let bounded: Rates =
                rates.try_into().map_err(|_| DispatchError::from(Error::<T>::TooManyRates))?;
            Ok(bounded)
        }
    }
}

#[cfg(any(test, feature = "runtime-benchmarks"))]
mod mock;
#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
