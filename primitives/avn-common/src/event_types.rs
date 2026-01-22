use crate::bounds::NftExternalRefBound;
use codec::{Decode, Encode, MaxEncodedLen};
use hex_literal::hex;
use sp_core::{bounded::BoundedVec, H160, H256, H512, U256};
use sp_runtime::{scale_info::TypeInfo, traits::Member, DispatchError, DispatchResult};
use sp_std::{convert::TryInto, vec::Vec};
use strum::{EnumIter, IntoEnumIterator};

// ================================= Events Types ====================================

const WORD_LENGTH: usize = 32; // basic word type for Ethereum is 32 bytes
const HALF_WORD_LENGTH: usize = 16; // needed for creating a u128
const TWENTY_FOUR_BYTES: usize = 24; // needed for creating a u64
const TWENTY_EIGHT_BYTES: usize = 28; // needed for creating a u32
const DISCARDED_ZERO_BYTES: usize = 12; // Used to ignore the first 12 bytes of a 32-byte value when creating an Ethereum address.
const BYTE_LENGTH: usize = 1; // the length of 1 byte
pub const LEGACY_LIFT_SIGNATURE: [H256; 2] = [
    H256(hex!("8964776336bc2fa8ecaaf70b6f8e8450807efb1ff78f8b87980707aa821f0ec0")),
    H256(hex!("53dbd0621188344e69521ce5392debdff038d57d0ebd39536df06b20d9142bc0")),
];

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    AddedValidatorEventMissingData,
    AddedValidatorEventBadDataLength,
    AddedValidatorEventWrongTopicCount,
    AddedValidatorEventBadTopicLength,

    LiftedEventMissingData,
    LiftedEventDataOverflow,
    LiftedEventBadDataLength,
    LiftedEventWrongTopicCount,
    LiftedEventBadTopicLength,

    NftMintedEventMissingData,
    NftMintedEventWrongTopicCount,
    NftMintedEventBadTopicLength,
    NftMintedEventSaleIndexConversion,
    NftMintedEventBadDataLength,
    NftMintedEventBadRefLength,

    NftTransferToEventShouldOnlyContainTopics,
    NftTransferToEventWrongTopicCount,
    NftTransferToEventBadTopicLength,
    NftTransferToEventTranferNonceConversion,

    NftCancelListingEventShouldOnlyContainTopics,
    NftCancelListingEventWrongTopicCount,
    NftCancelListingEventBadTopicLength,
    NftCancelListingEventDataOverflow,
    NftCancelListingEventTranferNonceConversion,

    NftEndBatchListingEventShouldOnlyContainTopics,
    NftEndBatchListingEventWrongTopicCount,
    NftEndBatchListingEventBadTopicLength,

    AvtGrowthLiftedEventShouldOnlyContainTopics,
    AvtGrowthLiftedEventWrongTopicCount,
    AvtGrowthLiftedEventBadTopicLength,
    AvtGrowthLiftedEventDataOverflow,
    AvtGrowthLiftedEventPeriodConversion,

    AvtLowerClaimedEventMissingData,
    AvtLowerClaimedEventWrongTopicCount,
    AvtLowerClaimedEventBadTopicLength,
    AvtLowerClaimedEventIdConversion,

    LiftedToPredictionMarketEventMissingData,
    LiftedToPredictionMarketEventDataOverflow,
    LiftedToPredictionMarketEventBadDataLength,
    LiftedToPredictionMarketEventWrongTopicCount,
    LiftedToPredictionMarketEventBadTopicLength,

    TotalSupplyUpdatedEventShouldOnlyContainTopics,
    TotalSupplyUpdatedEventWrongTopicCount,
    TotalSupplyUpdatedEventBadTopicLength,
    TotalSupplyUpdatedEventOldSupplyOverflow,
    TotalSupplyUpdatedEventNewSupplyOverflow,
    TotalSupplyUpdatedEventT2TxIdConversion,
}

#[derive(
    Encode, Decode, Clone, PartialOrd, Ord, Debug, PartialEq, Eq, TypeInfo, MaxEncodedLen, EnumIter,
)]

/// Represents the set of valid events supported by the AvN.
///
/// # Overview
/// This enumeration defines the types of events that the system recognizes and processes.
/// The majority of these events are emitted by the bridge contract and are classified as **primary
/// events**. Events that are associated with the contract but emitted by other contracts are
/// classified as **secondary events**.
///
/// Primary events take precedence and override secondary events if both occur in the same
/// transaction.
pub enum ValidEvents {
    /// A validator was added.
    AddedValidator,
    /// A lift operation was executed.
    Lifted,
    /// An NFT was minted.
    NftMint,
    /// An NFT was transferred.
    NftTransferTo,
    /// An NFT listing was canceled.
    NftCancelListing,
    /// End of a batch NFT listing.
    NftEndBatchListing,
    /// AVT growth was lifted.
    AvtGrowthLifted,
    /// A claim for lower AVT was executed.
    AvtLowerClaimed,
    /// A lift operation to the prediction market.
    LiftedToPredictionMarket,
    /// Secondary event emitted by the ERC-20 token contract.
    Erc20DirectTransfer,
    // AVT Supply Updated event
    TotalSupplyUpdated,
}

impl ValidEvents {
    pub fn signature(&self) -> H256 {
        match *self {
            // PLEASE: keep these comments in here.
            // Since hashes are one-way, they are essentially meaningless
            // and we can't check if they are up to date or we need to change them as we update
            // event signatures

            // hex string of Keccak-256 for LogValidatorRegistered(bytes32,bytes32,bytes32,uint256)
            ValidEvents::AddedValidator =>
                H256(hex!("ff083a6e395a67771f3c9108922bc274c27b38b48c210b0f6a8c5f4710c0494b")),

            // hex string of Keccak-256 for LogLifted(address,bytes32,uint256)
            ValidEvents::Lifted =>
                H256(hex!("418da8f85cfa851601f87634c6950491b6b8785a6445c8584f5658048d512cae")),

            // hex string of Keccak-256 for LogLiftedToPredictionMarket(address,bytes32,uint256)
            ValidEvents::LiftedToPredictionMarket =>
                H256(hex!("2bf8107bf8c15cdcd8d6360f4a02ee97d7098a46b18fccd32df8796775552fc0")),

            // hex string of Keccak-256 for AvnMintTo(uint256,uint64,bytes32,string)
            ValidEvents::NftMint =>
                H256(hex!("242e8a2c5335295f6294a23543699a458e6d5ed7a5839f93cc420116e0a31f99")),

            // hex string of Keccak-256 for AvnTransferTo(uint256,bytes32,uint64)
            ValidEvents::NftTransferTo =>
                H256(hex!("fff226ba128aca9718a568817388f3711cfeedd8c81cec4d02dcefc50f3c67bb")),

            // hex string of Keccak-256 for AvnCancelNftListing(uint256,uint64)
            ValidEvents::NftCancelListing =>
                H256(hex!("eb0a71ca01b1505be834cafcd54b651d77eafd1ca915d21c0898575bcab53358")),

            // hex string of Keccak-256 for AvnEndBatchListing(uint256)
            ValidEvents::NftEndBatchListing =>
                H256(hex!("20c46236a16e176bc83a795b3a64ad94e5db8bc92afc8cc6d3fd4a3864211f8f")),

            // hex string of Keccak-256 for LogGrowth(uint256,uint32)
            ValidEvents::AvtGrowthLifted =>
                H256(hex!("3ad58a8dc1110baa37ad88a68db14181b4ef0c69192dfa7699a9588960eca7fd")),

            // hex string of Keccak-256 for LogLowerClaimed(uint32)
            ValidEvents::AvtLowerClaimed =>
                H256(hex!("9853e4c075911a10a89a0f7a46bac6f8a246c4e9152480d16d86aa6a2391a4f1")),

            // hex string of Keccak-256 for Transfer(address,address,uint256)
            ValidEvents::Erc20DirectTransfer =>
                H256(hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")),

            // hex string of keccak256 for LogAvtSupplyUpdated(uint256,uint256,uint32)
            ValidEvents::TotalSupplyUpdated =>
                H256(hex!("4f329d0d0d89ca94365b34a2342dc677a3891e1cfe20e5fe9ed28e438649a540")),
        }
    }

    pub fn is_nft_event(&self) -> bool {
        match *self {
            ValidEvents::NftMint |
            ValidEvents::NftTransferTo |
            ValidEvents::NftCancelListing |
            ValidEvents::NftEndBatchListing => true,
            _ => false,
        }
    }

    pub fn values() -> Vec<ValidEvents> {
        ValidEvents::iter().collect()
    }

    pub fn is_primary(&self) -> bool {
        match *self {
            ValidEvents::Erc20DirectTransfer => false,
            _ => true,
        }
    }
}

impl TryFrom<&H256> for ValidEvents {
    type Error = ();

    fn try_from(value: &H256) -> Result<Self, Self::Error> {
        if let Some(e) = ValidEvents::iter().find(|event| event.signature() == *value) {
            Ok(e)
        } else {
            Err(())
        }
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct AddedValidatorData {
    pub eth_public_key: H512,
    pub t2_address: H256,
    pub validator_account_id: U256,
}

impl AddedValidatorData {
    const TOPIC_INDEX_T1_PUBLIC_KEY_LHS: usize = 1;
    const TOPIC_INDEX_T1_PUBLIC_KEY_RHS: usize = 2;
    const TOPIC_INDEX_T2_ADDRESS: usize = 3;

    pub fn is_valid(&self) -> bool {
        return !self.eth_public_key.is_zero() &&
            !self.t2_address.is_zero() &&
            !self.validator_account_id.is_zero()
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> deposit (32 bytes)
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> t1 public key part 1 (32 bytes = first half of the 64 bytes public key)
        // topics[2] --> t1 public key part 2 (32 bytes = second half of the 64 bytes public key)
        // topics[3] --> t2 address

        if data.is_none() {
            return Err(Error::AddedValidatorEventMissingData)
        }
        let data = data.expect("Already checked for errors");

        if data.len() != WORD_LENGTH {
            return Err(Error::AddedValidatorEventBadDataLength)
        }

        if topics.len() != 4 {
            return Err(Error::AddedValidatorEventWrongTopicCount)
        }

        if topics[Self::TOPIC_INDEX_T1_PUBLIC_KEY_LHS].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_T1_PUBLIC_KEY_RHS].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_T2_ADDRESS].len() != WORD_LENGTH
        {
            return Err(Error::AddedValidatorEventBadTopicLength)
        }

        // The full public key is split into 2 32 byte words
        let mut eth_public_key_full = topics[Self::TOPIC_INDEX_T1_PUBLIC_KEY_LHS].to_vec();
        eth_public_key_full.append(&mut topics[Self::TOPIC_INDEX_T1_PUBLIC_KEY_RHS].to_vec());

        let eth_public_key = H512::from_slice(eth_public_key_full.as_slice());

        let t2_address = H256::from_slice(&topics[Self::TOPIC_INDEX_T2_ADDRESS]);
        let validator_id = <U256 as From<&[u8]>>::from(&data);
        return Ok(AddedValidatorData {
            eth_public_key,
            t2_address,
            validator_account_id: validator_id,
        })
    }
}

// T1 Event definition:
// event LogLifted(address indexed tokenContract, bytes32 indexed liftee,
// uint256 amount);
#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct LiftedData {
    pub token_contract: H160,
    pub sender_address: H160,
    pub receiver_address: H256,
    pub amount: u128,
    pub nonce: U256,
}

impl LiftedData {
    const TOPIC_CURRENCY_CONTRACT: usize = 1;
    const TOPIC_INDEX_T2_ADDRESS: usize = 2;

    pub fn is_valid(&self) -> bool {
        return !self.token_contract.is_zero() && !self.receiver_address.is_zero()
    }

    pub fn new(token_contract: H160, receiver_address: H256, amount: u128) -> Self {
        LiftedData {
            token_contract,
            receiver_address,
            amount,
            sender_address: H160::zero(),
            nonce: U256::zero(),
        }
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> amount (32 bytes) (big endian)
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> currency contract address (first 12 bytes are 0 and should be ignored)
        // topics[2] --> receiver t2 public key (32 bytes)

        if data.is_none() {
            return Err(Error::LiftedEventMissingData)
        }
        let data = data.expect("Already checked for errors");

        if data.len() != WORD_LENGTH {
            return Err(Error::LiftedEventBadDataLength)
        }

        if topics.len() != 3 {
            return Err(Error::LiftedEventWrongTopicCount)
        }

        if topics[Self::TOPIC_CURRENCY_CONTRACT].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_T2_ADDRESS].len() != WORD_LENGTH
        {
            return Err(Error::LiftedEventBadTopicLength)
        }

        let token_contract = H160::from_slice(
            &topics[Self::TOPIC_CURRENCY_CONTRACT][DISCARDED_ZERO_BYTES..WORD_LENGTH],
        );

        let receiver_address = H256::from_slice(&topics[Self::TOPIC_INDEX_T2_ADDRESS]);

        if data[0..HALF_WORD_LENGTH].iter().any(|byte| byte > &0) {
            return Err(Error::LiftedEventDataOverflow)
        }

        let amount = u128::from_be_bytes(
            data[HALF_WORD_LENGTH..WORD_LENGTH]
                .try_into()
                .expect("Slice is the correct size"),
        );
        return Ok(LiftedData {
            token_contract,
            // SYS-1905 Keeping for backwards compatibility with the dapps (block explorer)
            sender_address: H160::zero(),
            receiver_address,
            amount,
            // SYS-1905 Keeping for backwards compatibility with the dapps (block explorer)
            nonce: U256::zero(),
        })
    }
}

impl LiftedData {
    const TOPIC_T1_FROM_ADDRESS: usize = 1;
    const TOPIC_BRIDGE_CONTRACT: usize = 2;

    pub fn from_erc_20_contract_transfer_bytes(
        data: Option<Vec<u8>>,
        topics: Vec<Vec<u8>>,
    ) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> amount (32 bytes) (big endian)
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> ethereum sender address
        // topics[2] --> ethereum receiver address (bridge contract)

        if data.is_none() {
            return Err(Error::LiftedEventMissingData)
        }
        let data = data.expect("Already checked for errors");

        if data.len() != WORD_LENGTH {
            return Err(Error::LiftedEventBadDataLength)
        }

        if topics.len() != 3 {
            return Err(Error::LiftedEventWrongTopicCount)
        }

        if topics[Self::TOPIC_BRIDGE_CONTRACT].len() != WORD_LENGTH ||
            topics[Self::TOPIC_T1_FROM_ADDRESS].len() != WORD_LENGTH
        {
            return Err(Error::LiftedEventBadTopicLength)
        }

        let bridge_contract = H160::from_slice(
            &topics[Self::TOPIC_BRIDGE_CONTRACT][DISCARDED_ZERO_BYTES..WORD_LENGTH],
        );

        let sender_address = H160::from_slice(
            &topics[Self::TOPIC_T1_FROM_ADDRESS][DISCARDED_ZERO_BYTES..WORD_LENGTH],
        );

        if data[0..HALF_WORD_LENGTH].iter().any(|byte| byte > &0) {
            return Err(Error::LiftedEventDataOverflow)
        }

        let amount = u128::from_be_bytes(
            data[HALF_WORD_LENGTH..WORD_LENGTH]
                .try_into()
                .expect("Slice is the correct size"),
        );
        return Ok(LiftedData {
            token_contract: H160::zero(),
            sender_address,
            // This must be overwritten after the lift reach consensus with the account configured
            // on the pallet
            receiver_address: bridge_contract.into(),
            amount,
            nonce: U256::zero(),
        })
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct NftMintData {
    pub batch_id: U256,
    pub t2_owner_public_key: H256,
    #[deprecated(note = "must only be used for backwards compatibility reasons")]
    pub op_id: u64,
    #[deprecated(note = "must only be used for backwards compatibility reasons")]
    pub t1_contract_issuer: H160,
    pub sale_index: u64,
    pub unique_external_ref: BoundedVec<u8, NftExternalRefBound>,
}

impl NftMintData {
    const TOPIC_INDEX_BATCH_ID: usize = 1;
    const TOPIC_INDEX_SALE_INDEX: usize = 2;
    const TOPIC_INDEX_T2_OWNER_PUBLIC_KEY: usize = 3;

    pub fn is_valid(&self) -> bool {
        return !self.batch_id.is_zero() &&
            !self.t2_owner_public_key.is_zero() &&
            !self.unique_external_ref.is_empty()
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> unique_external_ref (string)
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> the nft batch_id (32 bytes)
        // topics[2] --> sale index (first 24 bytes are 0 and should be ignored)
        // topics[3] --> AvN onwer public key (32 bytes)

        if data.is_none() {
            return Err(Error::NftMintedEventMissingData)
        }
        let data = data.expect("Already checked for errors");

        if data.len() != 4 * WORD_LENGTH {
            return Err(Error::NftMintedEventBadDataLength)
        }

        if topics.len() != 4 {
            return Err(Error::NftMintedEventWrongTopicCount)
        }

        if topics[Self::TOPIC_INDEX_BATCH_ID].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_T2_OWNER_PUBLIC_KEY].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_SALE_INDEX].len() != WORD_LENGTH
        {
            return Err(Error::NftMintedEventBadTopicLength)
        }

        let batch_id = <U256 as From<&[u8]>>::from(&topics[Self::TOPIC_INDEX_BATCH_ID]);
        let sale_index = u64::from_be_bytes(
            topics[Self::TOPIC_INDEX_SALE_INDEX][TWENTY_FOUR_BYTES..WORD_LENGTH]
                .try_into()
                .map_err(|_| Error::NftMintedEventSaleIndexConversion)?,
        );
        let t2_owner_public_key = H256::from_slice(&topics[Self::TOPIC_INDEX_T2_OWNER_PUBLIC_KEY]);

        // This is a string field but its value should always be 4 WORD_LENGTH.
        // The first 2 WORD_LENGTH are encoding data.
        // The actual unique ref is expected to be a UUID which is made up of 32bytes (WORD_LENGTH)
        // + 4 bytes for the dashes Example: b1dc0452-8b2f-78ec-7e80-167002d11678
        let ref_size = WORD_LENGTH + 4 * BYTE_LENGTH;
        let unique_external_ref = BoundedVec::<u8, NftExternalRefBound>::try_from(
            data[2 * WORD_LENGTH..2 * WORD_LENGTH + ref_size].to_vec(),
        )
        .map_err(|_| Error::NftMintedEventBadRefLength)?;

        #[allow(deprecated)]
        return Ok(NftMintData {
            batch_id,
            t2_owner_public_key,
            op_id: 0u64,
            t1_contract_issuer: H160::zero(),
            sale_index,
            unique_external_ref,
        })
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct NftTransferToData {
    pub nft_id: U256,
    pub t2_transfer_to_public_key: H256,
    pub op_id: u64,
}

impl NftTransferToData {
    const TOPIC_INDEX_NFT_ID: usize = 1;
    const TOPIC_INDEX_T2_TRANSFER_TO_PUBLIC_KEY: usize = 2;
    const TOPIC_INDEX_OP_ID: usize = 3;

    pub fn is_valid(&self) -> bool {
        return !self.t2_transfer_to_public_key.is_zero()
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> empty
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> the nft nft_id (32 bytes)
        // topics[2] --> AvN public key to transfer the token to(32 bytes)
        // topics[3] --> transfer nonce (first 24 bytes are 0 and should be ignored)

        if data.is_some() {
            return Err(Error::NftTransferToEventShouldOnlyContainTopics)
        }

        if topics.len() != 4 {
            return Err(Error::NftTransferToEventWrongTopicCount)
        }

        if topics[Self::TOPIC_INDEX_NFT_ID].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_T2_TRANSFER_TO_PUBLIC_KEY].len() != WORD_LENGTH ||
            topics[Self::TOPIC_INDEX_OP_ID].len() != WORD_LENGTH
        {
            return Err(Error::NftTransferToEventBadTopicLength)
        }

        let nft_id = <U256 as From<&[u8]>>::from(&topics[Self::TOPIC_INDEX_NFT_ID]);
        let t2_transfer_to_public_key =
            H256::from_slice(&topics[Self::TOPIC_INDEX_T2_TRANSFER_TO_PUBLIC_KEY]);
        let op_id = u64::from_be_bytes(
            topics[Self::TOPIC_INDEX_OP_ID][TWENTY_FOUR_BYTES..WORD_LENGTH]
                .try_into()
                .map_err(|_| Error::NftTransferToEventTranferNonceConversion)?,
        );

        return Ok(NftTransferToData { nft_id, t2_transfer_to_public_key, op_id })
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct NftCancelListingData {
    pub nft_id: U256,
    pub op_id: u64,
}

impl NftCancelListingData {
    const TOPIC_INDEX_NFT_ID: usize = 1;
    const TOPIC_INDEX_OP_ID: usize = 2;

    pub fn is_valid(&self) -> bool {
        return true
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> empty
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> the nft nft_id (32 bytes)
        // topics[2] --> op_id (first 24 bytes are 0 and should be ignored)

        if data.is_some() {
            return Err(Error::NftCancelListingEventShouldOnlyContainTopics)
        }

        if topics.len() != 3 {
            return Err(Error::NftCancelListingEventWrongTopicCount)
        }

        if topics[Self::TOPIC_INDEX_NFT_ID].len() != WORD_LENGTH {
            return Err(Error::NftCancelListingEventBadTopicLength)
        }

        if topics[Self::TOPIC_INDEX_OP_ID][0..TWENTY_FOUR_BYTES]
            .iter()
            .any(|byte| byte > &0)
        {
            return Err(Error::NftCancelListingEventDataOverflow)
        }

        let nft_id = <U256 as From<&[u8]>>::from(&topics[Self::TOPIC_INDEX_NFT_ID]);
        let op_id = u64::from_be_bytes(
            topics[Self::TOPIC_INDEX_OP_ID][TWENTY_FOUR_BYTES..WORD_LENGTH]
                .try_into()
                .map_err(|_| Error::NftCancelListingEventTranferNonceConversion)?,
        );

        return Ok(NftCancelListingData { nft_id, op_id })
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct NftEndBatchListingData {
    pub batch_id: U256,
}

impl NftEndBatchListingData {
    const TOPIC_INDEX_BATCH_ID: usize = 1;

    pub fn is_valid(&self) -> bool {
        return true
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data --> empty
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> the nft nft_id (32 bytes)

        if data.is_some() {
            return Err(Error::NftEndBatchListingEventShouldOnlyContainTopics)
        }

        if topics.len() != 2 {
            return Err(Error::NftEndBatchListingEventWrongTopicCount)
        }

        if topics[Self::TOPIC_INDEX_BATCH_ID].len() != WORD_LENGTH {
            return Err(Error::NftEndBatchListingEventBadTopicLength)
        }

        let batch_id = <U256 as From<&[u8]>>::from(&topics[Self::TOPIC_INDEX_BATCH_ID]);

        return Ok(NftEndBatchListingData { batch_id })
    }
}

// T1 Event definition:
// event LogGrowth(uint256 amount, uint32 period);
#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct AvtGrowthLiftedData {
    pub amount: u128,
    pub period: u32,
}

impl AvtGrowthLiftedData {
    const TOPIC_AMOUNT: usize = 1;
    const TOPIC_PERIOD: usize = 2;

    pub fn is_valid(&self) -> bool {
        return self.amount > 0u128
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure of input bytes:
        // data -> empty
        // all topics are 32 bytes long
        // topics[0] --> event signature (can be ignored)
        // topics[1] --> amount (32 bytes)
        // topics[2] --> period (first 28 bytes are 0 and should be ignored)

        if data.is_some() {
            return Err(Error::AvtGrowthLiftedEventShouldOnlyContainTopics)
        }

        if topics.len() != 3 {
            return Err(Error::AvtGrowthLiftedEventWrongTopicCount)
        }

        if topics[Self::TOPIC_AMOUNT].len() != WORD_LENGTH ||
            topics[Self::TOPIC_PERIOD].len() != WORD_LENGTH
        {
            return Err(Error::AvtGrowthLiftedEventBadTopicLength)
        }

        if topics[Self::TOPIC_AMOUNT][0..HALF_WORD_LENGTH].iter().any(|byte| byte > &0) {
            return Err(Error::AvtGrowthLiftedEventDataOverflow)
        }

        let amount = u128::from_be_bytes(
            topics[Self::TOPIC_AMOUNT][HALF_WORD_LENGTH..WORD_LENGTH]
                .try_into()
                .expect("Slice is the correct size"),
        );

        let period = u32::from_be_bytes(
            topics[Self::TOPIC_PERIOD][TWENTY_EIGHT_BYTES..WORD_LENGTH]
                .try_into()
                .map_err(|_| Error::AvtGrowthLiftedEventPeriodConversion)?,
        );

        return Ok(AvtGrowthLiftedData { amount, period })
    }
}

// T1 Event definition:
// event LogLowerClaimed(uint32 lowerId);
#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct AvtLowerClaimedData {
    pub lower_id: u32,
}

impl AvtLowerClaimedData {
    const TOPIC_LOWER_ID: usize = 1;

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        if data.is_some() {
            return Err(Error::AvtLowerClaimedEventMissingData)
        }

        if topics.len() != 2 {
            return Err(Error::AvtLowerClaimedEventWrongTopicCount)
        }

        if topics[Self::TOPIC_LOWER_ID].len() != WORD_LENGTH {
            return Err(Error::AvtLowerClaimedEventBadTopicLength)
        }

        let lower_id: u32 = u32::from_be_bytes(
            topics[Self::TOPIC_LOWER_ID][TWENTY_EIGHT_BYTES..WORD_LENGTH]
                .try_into()
                .map_err(|_| Error::AvtLowerClaimedEventIdConversion)?,
        );

        return Ok(AvtLowerClaimedData { lower_id })
    }
}

// T1 Event definition:
// event LogT1TotalSupplyUpdated(uint256 indexed oldSupply, uint256 indexed newSupply, uint32
// indexed t2TxId);
#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct TotalSupplyUpdatedData {
    pub old_supply: u128,
    pub new_supply: u128,
    pub t2_tx_id: u32,
}

impl TotalSupplyUpdatedData {
    const TOPIC_OLD_SUPPLY: usize = 1;
    const TOPIC_NEW_SUPPLY: usize = 2;
    const TOPIC_T2_TX_ID: usize = 3;

    pub fn is_valid(&self) -> bool {
        self.new_supply > 0u128 && self.t2_tx_id > 0u32
    }

    pub fn parse_bytes(data: Option<Vec<u8>>, topics: Vec<Vec<u8>>) -> Result<Self, Error> {
        // Structure:
        // data -> empty
        // topics[0] -> event signature
        // topics[1] -> oldSupply (uint256)
        // topics[2] -> newSupply (uint256)
        // topics[3] -> t2TxId (uint32 encoded in uint256 slot)

        if data.is_some() {
            return Err(Error::TotalSupplyUpdatedEventShouldOnlyContainTopics)
        }

        if topics.len() != 4 {
            return Err(Error::TotalSupplyUpdatedEventWrongTopicCount)
        }

        if topics[Self::TOPIC_OLD_SUPPLY].len() != WORD_LENGTH ||
            topics[Self::TOPIC_NEW_SUPPLY].len() != WORD_LENGTH ||
            topics[Self::TOPIC_T2_TX_ID].len() != WORD_LENGTH
        {
            return Err(Error::TotalSupplyUpdatedEventBadTopicLength)
        }

        // oldSupply and newSupply are uint256 → we only support up to u128 (like LiftedData)
        if topics[Self::TOPIC_OLD_SUPPLY][0..HALF_WORD_LENGTH].iter().any(|byte| byte > &0) {
            return Err(Error::TotalSupplyUpdatedEventOldSupplyOverflow)
        }
        if topics[Self::TOPIC_NEW_SUPPLY][0..HALF_WORD_LENGTH].iter().any(|byte| byte > &0) {
            return Err(Error::TotalSupplyUpdatedEventNewSupplyOverflow)
        }

        let old_supply = u128::from_be_bytes(
            topics[Self::TOPIC_OLD_SUPPLY][HALF_WORD_LENGTH..WORD_LENGTH]
                .try_into()
                .expect("Slice is the correct size"),
        );

        let new_supply = u128::from_be_bytes(
            topics[Self::TOPIC_NEW_SUPPLY][HALF_WORD_LENGTH..WORD_LENGTH]
                .try_into()
                .expect("Slice is the correct size"),
        );

        let t2_tx_id = u32::from_be_bytes(
            topics[Self::TOPIC_T2_TX_ID][TWENTY_EIGHT_BYTES..WORD_LENGTH]
                .try_into()
                .map_err(|_| Error::TotalSupplyUpdatedEventT2TxIdConversion)?,
        );

        Ok(TotalSupplyUpdatedData { old_supply, new_supply, t2_tx_id })
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub enum EventData {
    LogAddedValidator(AddedValidatorData),
    LogLifted(LiftedData),
    EmptyEvent,
    LogNftMinted(NftMintData),
    LogNftTransferTo(NftTransferToData),
    LogNftCancelListing(NftCancelListingData),
    LogNftEndBatchListing(NftEndBatchListingData),
    LogAvtGrowthLifted(AvtGrowthLiftedData),
    LogLowerClaimed(AvtLowerClaimedData),
    LogLiftedToPredictionMarket(LiftedData),
    LogErc20Transfer(LiftedData),
    LogT1TotalSupplyUpdated(TotalSupplyUpdatedData),
}

impl EventData {
    #[allow(unreachable_patterns)]
    pub fn is_valid(&self) -> bool {
        return match self {
            // LogLowerClaimed missing. TODO add and remove unreachable patterns.
            EventData::LogAddedValidator(d) => d.is_valid(),
            EventData::LogLifted(d) => d.is_valid(),
            EventData::LogNftMinted(d) => d.is_valid(),
            EventData::LogNftTransferTo(d) => d.is_valid(),
            EventData::LogNftCancelListing(d) => d.is_valid(),
            EventData::LogNftEndBatchListing(d) => d.is_valid(),
            EventData::LogAvtGrowthLifted(d) => d.is_valid(),
            EventData::LogLiftedToPredictionMarket(d) => d.is_valid(),
            EventData::LogErc20Transfer(d) => d.is_valid(),
            EventData::LogT1TotalSupplyUpdated(d) => d.is_valid(),
            EventData::EmptyEvent => true,
            _ => false,
        }
    }
}

impl Default for EventData {
    fn default() -> Self {
        EventData::EmptyEvent
    }
}

// ================================= Checking and Validating Events
// ====================================
#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub enum CheckResult {
    /// Event exists on tier 1
    Ok,
    /// Event is not valid.
    /// This could be due to several reason such as the event missing or not having a correctly
    /// formatted event data
    Invalid,
    /// Http error
    HttpErrorCheckingEvent,
    /// Event is too young, not enough confirmations
    InsufficientConfirmations,
    /// Default value
    Unknown,
}

impl Default for CheckResult {
    fn default() -> Self {
        CheckResult::Unknown
    }
}

// Data type for storing an Ethereum transaction ID.
pub type EthTransactionId = H256;

#[derive(Encode, Decode, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
pub struct EthProcessedEvent {
    pub id: ValidEvents,
    pub accepted: bool,
}
#[derive(Encode, Decode, Default, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
// Note: strictly speaking, different contracts can have events with the same signature, which would
// suggest that the contract should be part of the EventId.
// But the expected communication framework is that all these events are generated by contracts we
// write and deploy ourselves. When we check the validity of an event with this identifier, we can
// explicitly check it against our contracts and avoid conflicts with events generated maliciously
// by some attacker contract
pub struct EthEventId {
    pub signature: H256, /* this is the Event Signature, as in ethereum's Topic0. It is not a
                          * cryptographic signature */
    pub transaction_hash: EthTransactionId,
}

impl EthEventId {
    pub fn hashed<R, F: FnOnce(&[u8]) -> R>(&self, hasher: F) -> R {
        return (self.signature, self.transaction_hash).using_encoded(hasher)
    }
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, Eq, TypeInfo, MaxEncodedLen)]
pub struct EthEvent {
    pub event_id: EthEventId,
    pub event_data: EventData,
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, TypeInfo, MaxEncodedLen)]
pub struct EthEventCheckResult<BlockNumber: Member, AccountId: Member> {
    pub event: EthEvent,
    pub result: CheckResult,
    pub checked_by: AccountId,
    pub checked_at_block: BlockNumber,
    pub ready_for_processing_after_block: BlockNumber,
    // Minimum number of votes to successfully challenge this result
    pub min_challenge_votes: u32,
}

impl<BlockNumber: Member, AccountId: Member> EthEventCheckResult<BlockNumber, AccountId> {
    pub fn new(
        ready_after_block: BlockNumber,
        result: CheckResult,
        event_id: &EthEventId,
        event_data: &EventData,
        checked_by: AccountId,
        checked_at_block: BlockNumber,
        min_challenge_votes: u32,
    ) -> Self {
        return EthEventCheckResult::<BlockNumber, AccountId> {
            event: EthEvent { event_id: event_id.clone(), event_data: event_data.clone() },
            result,
            checked_by,
            ready_for_processing_after_block: ready_after_block,
            checked_at_block,
            min_challenge_votes,
        }
    }
}

// ================================= Challenges
// =======================================================
#[derive(Encode, Decode, Clone, PartialEq, Debug, Eq, TypeInfo)]
pub enum ChallengeReason {
    /// The result of the check is not correct
    IncorrectResult,
    /// The event data is not correct
    IncorrectEventData,
    /// Default value
    Unknown,
}

#[derive(Encode, Decode, Default, Clone, PartialEq, Debug, TypeInfo)]
pub struct Challenge<AccountId: Member> {
    pub event_id: EthEventId,
    pub challenge_reason: ChallengeReason,
    pub challenged_by: AccountId,
}

impl<AccountId: Member> Challenge<AccountId> {
    pub fn new(
        event_id: EthEventId,
        challenge_reason: ChallengeReason,
        challenged_by: AccountId,
    ) -> Self {
        return Challenge::<AccountId> { event_id, challenge_reason, challenged_by }
    }
}

impl Default for ChallengeReason {
    fn default() -> Self {
        ChallengeReason::Unknown
    }
}

// ================================= Authorities and Validators
// =======================================

#[derive(Encode, Decode, Default, Clone, Debug, PartialEq, TypeInfo, MaxEncodedLen)]
pub struct Validator<AuthorityId: Member, AccountId: Member> {
    pub account_id: AccountId,
    pub key: AuthorityId,
}

impl<AuthorityId: Member, AccountId: Member> Validator<AuthorityId, AccountId> {
    pub fn new(account_id: AccountId, key: AuthorityId) -> Self {
        return Validator::<AuthorityId, AccountId> { account_id, key }
    }
}

// ======================================== Processed events
// ==========================================

pub trait ProcessedEventHandler {
    fn on_event_processed(event: &EthEvent) -> DispatchResult;
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl ProcessedEventHandler for Tuple {
    fn on_event_processed(_event: &EthEvent) -> DispatchResult {
        for_tuples!( #( Tuple::on_event_processed(_event)?; )* );
        Ok(())
    }
}

/// Trait to expose lift and lower functionality to external pallets
pub trait TokenInterface<TokenId, AccountId> {
    fn process_lift(event: &EthEvent) -> DispatchResult;

    fn deposit_tokens(
        token_id: TokenId,
        recipient_account_id: AccountId,
        raw_amount: u128,
    ) -> DispatchResult;
}

impl<TokenId, AccountId> TokenInterface<TokenId, AccountId> for () {
    fn process_lift(_event: &EthEvent) -> DispatchResult {
        return Err(DispatchError::Other("Not implemented"))
    }

    fn deposit_tokens(
        _token_id: TokenId,
        _recipient_account_id: AccountId,
        _raw_amount: u128,
    ) -> DispatchResult {
        return Err(DispatchError::Other("Not implemented"))
    }
}

// ======================================== Tests
// =====================================================

#[cfg(test)]
#[path = "tests/test_event_types.rs"]
mod test_event_types;

#[cfg(test)]
#[path = "tests/nft_event_tests.rs"]
mod nft_event_tests;

#[cfg(test)]
#[path = "tests/test_avt_growth_event_parsing.rs"]
mod test_avt_growth_event_parsing;

#[cfg(test)]
#[path = "tests/test_lower_claim.rs"]
mod test_lower_claim;
