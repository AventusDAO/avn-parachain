#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::string::String;

use alloy_primitives::{Address, Bytes, FixedBytes, B256 as AlloyB256, U256 as AlloyU256};
use alloy_sol_types::{eip712_domain, sol, Eip712Domain, SolStruct};
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::str::{self, FromStr};
use sp_core::{ConstU32, H160, H256};
use sp_io::hashing::blake2_256;
use sp_runtime::{scale_info::TypeInfo, BoundedVec, Deserialize, Serialize};
use sp_std::vec::Vec;
pub type EthereumId = u32;

pub const PACKED_LOWER_V1_PARAMS_SIZE: usize = 76;
pub const PACKED_LOWER_V2_PARAMS_SIZE: usize = 116;
pub type LowerParams = [u8; PACKED_LOWER_V2_PARAMS_SIZE];

const TOKEN_SPAN: core::ops::Range<usize> = 0..20;
const AMOUNT_PADDING_SPAN: core::ops::Range<usize> = 20..36;
const AMOUNT_SPAN: core::ops::Range<usize> = 36..52;
const RECIPIENT_SPAN: core::ops::Range<usize> = 52..72;
const LOWER_ID_SPAN: core::ops::Range<usize> = 72..PACKED_LOWER_V1_PARAMS_SIZE;
const T2_SENDER_SPAN: core::ops::Range<usize> = PACKED_LOWER_V1_PARAMS_SIZE..108;
const T2_TIMESTAMP_SPAN: core::ops::Range<usize> = 108..PACKED_LOWER_V2_PARAMS_SIZE;

#[derive(
    Encode,
    Decode,
    Default,
    Clone,
    Debug,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EthereumNetwork {
    #[default]
    Ethereum,
    EWC,
    Sepolia,
    Holesky,
    Volta,
    Custom(u64),
}

impl EthereumNetwork {
    pub fn chain_id(&self) -> u64 {
        match self {
            EthereumNetwork::Ethereum => 1_u64,
            EthereumNetwork::EWC => 246_u64,
            EthereumNetwork::Sepolia => 11155111_u64,
            EthereumNetwork::Holesky => 17000_u64,
            EthereumNetwork::Volta => 73799_u64,
            EthereumNetwork::Custom(value) => *value,
        }
    }
}

impl From<u64> for EthereumNetwork {
    fn from(value: u64) -> Self {
        match value {
            1 => EthereumNetwork::Ethereum,
            246 => EthereumNetwork::EWC,
            11155111 => EthereumNetwork::Sepolia,
            17000 => EthereumNetwork::Holesky,
            73799 => EthereumNetwork::Volta,
            _ => EthereumNetwork::Custom(value),
        }
    }
}

#[derive(
    Encode,
    Decode,
    Default,
    Clone,
    Debug,
    PartialEq,
    Eq,
    TypeInfo,
    MaxEncodedLen,
    DecodeWithMemTracking,
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EthBridgeInstance {
    pub network: EthereumNetwork,
    pub bridge_contract: H160,
    pub name: BoundedVec<u8, ConstU32<256>>,
    pub version: BoundedVec<u8, ConstU32<256>>,
    pub salt: Option<[u8; 32]>,
}

impl EthBridgeInstance {
    pub fn hash(&self) -> [u8; 32] {
        let encoded = self.encode();
        blake2_256(&encoded)
    }

    pub fn is_valid(&self) -> bool {
        !self.bridge_contract.is_zero() && !self.name.is_empty() && !self.version.is_empty()
    }
}

impl Into<Eip712Domain> for EthBridgeInstance {
    fn into(self) -> Eip712Domain {
        let name_vec: Vec<u8> = self.name.into_inner();
        let version_vec: Vec<u8> = self.version.into_inner();

        // Ignore salt for now
        eip712_domain!(
            name: String::from_utf8_lossy(&name_vec).into_owned(),
            version: String::from_utf8_lossy(&version_vec).into_owned(),
            chain_id: self.network.chain_id(),
            verifying_contract: Address::from_slice(&self.bridge_contract.as_bytes()),
        )
    }
}

#[derive(Debug)]
pub enum ECDSAVerificationError {
    InvalidSignature,
    InvalidValueForV,
    InvalidValueForRS,
    InvalidMessageFormat,
    BadSignature,
    FailedToHashStringData,
    FailedToHash32BytesHexData,
}

pub enum BridgeContractMethod {
    ReferenceRateUpdatedAt,
    CheckReferenceRate,
    UpdateReferenceRate,
    PublishRoot,
    TriggerGrowth,
    AddAuthor,
    RemoveAuthor,
}

impl BridgeContractMethod {
    pub fn name_as_bytes(&self) -> &[u8] {
        match self {
            BridgeContractMethod::ReferenceRateUpdatedAt => b"referenceRateUpdatedAt",
            BridgeContractMethod::CheckReferenceRate => b"checkReferenceRate",
            BridgeContractMethod::UpdateReferenceRate => b"updateReferenceRate",
            BridgeContractMethod::PublishRoot => b"publishRoot",
            BridgeContractMethod::TriggerGrowth => b"triggerGrowth",
            BridgeContractMethod::AddAuthor => b"addAuthor",
            BridgeContractMethod::RemoveAuthor => b"removeAuthor",
        }
    }
}

impl TryFrom<&[u8]> for BridgeContractMethod {
    type Error = ();

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        match value {
            b"referenceRateUpdatedAt" => Ok(BridgeContractMethod::ReferenceRateUpdatedAt),
            b"checkReferenceRate" => Ok(BridgeContractMethod::CheckReferenceRate),
            b"updateReferenceRate" => Ok(BridgeContractMethod::UpdateReferenceRate),
            b"publishRoot" => Ok(BridgeContractMethod::PublishRoot),
            b"triggerGrowth" => Ok(BridgeContractMethod::TriggerGrowth),
            b"addAuthor" => Ok(BridgeContractMethod::AddAuthor),
            b"removeAuthor" => Ok(BridgeContractMethod::RemoveAuthor),
            _ => Err(()),
        }
    }
}

sol! {
    struct PublishRoot {
        bytes32 rootHash;
        uint256 expiry;
        uint32 t2TxId;
    }
}

sol! {
    struct TriggerGrowth {
        uint256 rewards;
        uint256 avgStaked;
        uint32 period;
        uint256 expiry;
        uint32 t2TxId;
    }
}

sol! {
    struct AddAuthor {
        bytes t1PubKey;
        bytes32 t2PubKey;
        uint256 expiry;
        uint32 t2TxId;
    }
}

sol! {
    struct RemoveAuthor {
        bytes32 t2PubKey;
        bytes t1PubKey;
        uint256 expiry;
        uint32 t2TxId;
    }
}

sol! {
    struct LowerData {
        address token;
        uint256 amount;
        address recipient;
        uint32 lowerId;
        bytes32 t2Sender;
        uint64 t2Timestamp;
    }
}

impl TryFrom<LowerParams> for LowerData {
    type Error = ();

    fn try_from(lower_params: LowerParams) -> Result<Self, Self::Error> {
        if lower_params.len() != PACKED_LOWER_V2_PARAMS_SIZE {
            return Err(())
        }

        let token = Address::from_slice(&lower_params[TOKEN_SPAN]);
        let amount = AlloyU256::try_from_be_slice(&lower_params[AMOUNT_SPAN]).ok_or(())?;
        let recipient = Address::from_slice(&lower_params[RECIPIENT_SPAN]);
        let lower_id = u32::from_be_bytes(lower_params[LOWER_ID_SPAN].try_into().map_err(|_| ())?);
        let t2_sender = AlloyB256::from_slice(&lower_params[T2_SENDER_SPAN]);
        let t2_timestamp =
            u64::from_be_bytes(lower_params[T2_TIMESTAMP_SPAN].try_into().map_err(|_| ())?);

        Ok(LowerData {
            token,
            amount,
            recipient,
            lowerId: lower_id,
            t2Sender: t2_sender,
            t2Timestamp: t2_timestamp,
        })
    }
}

pub fn create_lower_proof_hash(
    lower_params: LowerParams,
    domain: Eip712Domain,
) -> Result<H256, ()> {
    let claim_lower = LowerData::try_from(lower_params)?;
    let hash = eip712_hash(&claim_lower, &domain);

    Ok(hash)
}

pub fn eip712_hash<T: SolStruct>(data: &T, domain: &Eip712Domain) -> H256 {
    let hash: AlloyB256 = data.eip712_signing_hash(domain);
    H256::from_slice(&hash.0)
}

fn parse_from_utf8<T: FromStr>(bytes: &[u8]) -> Result<T, ()> {
    str::from_utf8(bytes).map_err(|_| ())?.parse::<T>().map_err(|_| ())
}

pub fn create_function_confirmation_hash(
    function_name: Vec<u8>,
    params: Vec<(Vec<u8>, Vec<u8>)>,
    domain: Eip712Domain,
) -> Result<H256, ()> {
    let extract_tx_id_and_expiry =
        |params: &Vec<(Vec<u8>, Vec<u8>)>| -> Result<(u32, AlloyU256), ()> {
            if params.len() < 2 {
                return Err(()) // Ensure there are at least 2 elements in params
            }
            let last_two_elements = &params[params.len() - 2..];
            let expiry = AlloyU256::from(parse_from_utf8::<u64>(&last_two_elements[0].1)?);
            let tx_id = parse_from_utf8::<u32>(&last_two_elements[1].1)?;

            Ok((tx_id, expiry))
        };

    match BridgeContractMethod::try_from(&function_name[..])? {
        BridgeContractMethod::PublishRoot => {
            if params.len() != 3 {
                return Err(()) // Ensure there are at least 3 elements in params
            }
            let root_data = H256::from_slice(&params[0].1);
            let (tx_id, expiry) = extract_tx_id_and_expiry(&params)?;
            let data = PublishRoot {
                rootHash: FixedBytes::from_slice(root_data.as_fixed_bytes()),
                expiry,
                t2TxId: tx_id,
            };
            return Ok(eip712_hash(&data, &domain))
        },
        BridgeContractMethod::TriggerGrowth => {
            if params.len() != 5 {
                return Err(()) // Ensure there are at least 5 elements in params
            }

            let rewards = AlloyU256::from(parse_from_utf8::<u128>(&params[0].1)?);
            let avg_staked = AlloyU256::from(parse_from_utf8::<u128>(&params[1].1)?);
            let period = parse_from_utf8::<u32>(&params[2].1)?;

            let (tx_id, expiry) = extract_tx_id_and_expiry(&params)?;
            let data =
                TriggerGrowth { rewards, avgStaked: avg_staked, period, expiry, t2TxId: tx_id };
            return Ok(eip712_hash(&data, &domain))
        },

        BridgeContractMethod::AddAuthor => {
            if params.len() != 4 {
                return Err(())
            }

            let t1_pub_key: Bytes = Bytes::from(params[0].1.clone());
            let t2_pub_key: AlloyB256 = AlloyB256::from_slice(&params[1].1);

            let (tx_id, expiry) = extract_tx_id_and_expiry(&params)?;
            let data =
                AddAuthor { t1PubKey: t1_pub_key, t2PubKey: t2_pub_key, expiry, t2TxId: tx_id };
            return Ok(eip712_hash(&data, &domain))
        },

        BridgeContractMethod::RemoveAuthor => {
            if params.len() != 4 {
                return Err(())
            }

            let t2_pub_key: AlloyB256 = AlloyB256::from_slice(&params[0].1);
            let t1_pub_key: Bytes = Bytes::from(params[1].1.clone());

            let (tx_id, expiry) = extract_tx_id_and_expiry(&params)?;
            let data =
                RemoveAuthor { t2PubKey: t2_pub_key, t1PubKey: t1_pub_key, expiry, t2TxId: tx_id };
            return Ok(eip712_hash(&data, &domain))
        },

        _ => return Err(()),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn test_conversion_endianess() {
        let value_in_u32: u32 = 123456;
        let value_in_u64: u64 = 123456;
        let value_in_u128: u128 = 123456;
        let params = vec![
            (b"uint32".to_vec(), format!("{}", value_in_u32).as_bytes().to_vec()),
            (b"uint64".to_vec(), format!("{}", value_in_u64).as_bytes().to_vec()),
            (b"uint128".to_vec(), format!("{}", value_in_u128).as_bytes().to_vec()),
        ];

        let restored_u32 = parse_from_utf8::<u32>(&params[0].1).expect("msg should be valid");
        let restored_u64 = parse_from_utf8::<u64>(&params[1].1).expect("msg should be valid");
        let restored_u128 = parse_from_utf8::<u128>(&params[2].1).expect("msg should be valid");

        assert!(restored_u32 == value_in_u32, "Restored value does not match original");
        assert!(restored_u64 == value_in_u64, "Restored value does not match original");
        assert!(restored_u128 == value_in_u128, "Restored value does not match original");
    }

    fn domain() -> Eip712Domain {
        EthBridgeInstance {
            network: EthereumNetwork::Sepolia,
            bridge_contract: H160::from_slice(&hex!("0101010101010101010101010101010101010101")),
            name: BoundedVec::try_from(b"TestBridge".to_vec()).unwrap(),
            version: BoundedVec::try_from(b"1".to_vec()).unwrap(),
            salt: None,
        }
        .into()
    }

    #[test]
    fn publish_root_eip_712_hash() {
        let root_hash: H256 =
            H256(hex!("df21ce83ba19b6350f8a4dca44c50ab953563d53706bfe91a341401275de70b3"));
        let root = PublishRoot {
            rootHash: FixedBytes::from_slice(root_hash.as_fixed_bytes()),
            expiry: AlloyU256::from(1695811529),
            t2TxId: 1,
        };

        let eip712_domain: Eip712Domain = domain();
        let hash = eip712_hash(&root, &eip712_domain);

        assert_eq!(
            hash,
            H256(hex!("9796a122383f313ec3e416ea8fc4649a1e35ec321a15cbf72250bd0a7e5465b7")) /* Generated via the EnergyBridge contract */
        );
    }

    #[test]
    fn remove_author_eip_712_hash() {
        use hex_literal::hex;

        let t2_pub_key =
            H256(hex!("14aeac90dbd3573458f9e029eb2de122ee94f2f0bc5ee4b6c6c5839894f1a547"));
        let t1_pub_key: Bytes =
        Bytes::from(hex!("23d79f6492dddecb436333a5e7a4cfcc969f568e01283fa2964aae15327fb8a3b685a4d0f3ef9b3c2adb20f681dbc74b7f82c1cf8438d37f2c10e9c79591e9ea"));

        let remove_author = RemoveAuthor {
            t2PubKey: FixedBytes::from_slice(t2_pub_key.as_fixed_bytes()),
            t1PubKey: t1_pub_key,
            expiry: AlloyU256::from(1695811529),
            t2TxId: 1,
        };

        let eip712_domain: Eip712Domain = domain();
        let hash = eip712_hash(&remove_author, &eip712_domain);

        assert_eq!(
            hash,
            H256(hex!("f032c115ddf96bea32c9b445f53836fcce9b0e6243824063ea76da8b8049ea64")) /* Generated via the EnergyBridge contract */
        );
    }

    #[test]
    fn add_author_eip_712_hash() {
        use hex_literal::hex;

        let t2_pub_key =
            H256(hex!("14aeac90dbd3573458f9e029eb2de122ee94f2f0bc5ee4b6c6c5839894f1a547"));
        let t1_pub_key: Bytes =
        Bytes::from(hex!("23d79f6492dddecb436333a5e7a4cfcc969f568e01283fa2964aae15327fb8a3b685a4d0f3ef9b3c2adb20f681dbc74b7f82c1cf8438d37f2c10e9c79591e9ea"));

        let add_author = AddAuthor {
            t1PubKey: t1_pub_key,
            t2PubKey: FixedBytes::from_slice(t2_pub_key.as_fixed_bytes()),
            expiry: AlloyU256::from(1695811529),
            t2TxId: 1,
        };

        let eip712_domain: Eip712Domain = domain();
        let hash = eip712_hash(&add_author, &eip712_domain);

        assert_eq!(
            hash,
            H256(hex!("2cdf5c4ea05f21718a8028baeb214a34e7830b42fbb064054f07271e2c8743df")) /* Generated via the EnergyBridge contract */
        );
    }

    #[test]
    fn trigger_growth_eip_712_hash() {
        use hex_literal::hex;

        let growth = TriggerGrowth {
            rewards: AlloyU256::from(500_000_000_000_000_000_000u128),
            avgStaked: AlloyU256::from(1_000_000_000_000_000_000_000u128),
            period: 30,
            expiry: AlloyU256::from(1695811529),
            t2TxId: 1,
        };

        let eip712_domain: Eip712Domain = domain();
        let hash = eip712_hash(&growth, &eip712_domain);

        assert_eq!(
            hash,
            H256(hex!("560ddcd37021adc249014f89c426ee711d1928db1d6e3e145101ddc128aae27c")) /* Generated via the EnergyBridge contract */
        );
    }

    #[test]
    fn lower_data_eip_712_hash() {
        use hex_literal::hex;

        let lower_data = LowerData {
            token: Address::from_slice(&H160::from([3u8; 20]).as_bytes()),
            amount: AlloyU256::from(100_000_000_000_000_000_000u128),
            recipient: Address::from_slice(&H160::from([2u8; 20]).as_bytes()),
            lowerId: 10,
            t2Sender: FixedBytes::from_slice(H256::from([5u8; 32]).as_fixed_bytes()),
            t2Timestamp: 1_000_000_000u64,
        };

        let eip712_domain: Eip712Domain = domain();
        let hash = eip712_hash(&lower_data, &eip712_domain);

        assert_eq!(
            hash,
            H256(hex!("e5bf20ae6173912260d45213e1fc29b9d68f7ddc72f2922779a4f040f373f50e"))
        );
    }
}

pub fn concat_lower_data(
    lower_id: u32,
    token_id: H160,
    amount: &u128,
    t1_recipient: &H160,
    t2_sender: H256,
    t2_timestamp: u64,
) -> LowerParams {
    let mut lower_params: [u8; PACKED_LOWER_V2_PARAMS_SIZE] = [0u8; PACKED_LOWER_V2_PARAMS_SIZE];

    lower_params[TOKEN_SPAN].copy_from_slice(token_id.as_fixed_bytes());
    lower_params[AMOUNT_PADDING_SPAN].fill(0);
    lower_params[AMOUNT_SPAN].copy_from_slice(&amount.to_be_bytes());
    lower_params[RECIPIENT_SPAN].copy_from_slice(t1_recipient.as_fixed_bytes());
    lower_params[LOWER_ID_SPAN].copy_from_slice(&lower_id.to_be_bytes());
    lower_params[T2_SENDER_SPAN].copy_from_slice(t2_sender.as_fixed_bytes());
    lower_params[T2_TIMESTAMP_SPAN].copy_from_slice(&t2_timestamp.to_be_bytes());

    lower_params
}
