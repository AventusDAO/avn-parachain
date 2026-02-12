// Copyright 2026 Aventus DAO Ltd

use crate::*;

pub const VALIDATOR_ID_CAUSES_CONVERSION_ERROR: u8 = 99;

pub type AccountId = u64;
pub struct U64To32BytesConverter {}

impl AccountToBytesConverter<AccountId> for U64To32BytesConverter {
    fn into_bytes(account: &AccountId) -> [u8; 32] {
        let mut bytes = account.encode();
        // In tests AccountIds are u64 therefore 8 bytes. We need convert to 32 bytes for having
        // consistent information to the storage
        let mut bytes32: Vec<u8> = vec![0; 32 - bytes.len()];
        bytes32.append(&mut bytes);
        let mut data: [u8; 32] = Default::default();
        data.copy_from_slice(&bytes32[0..32]);
        data
    }

    fn try_from(account_bytes: &[u8; 32]) -> Result<AccountId, DispatchError> {
        let mut data: [u8; 8] = Default::default();
        // In tests AccountIds are u64 therefore 8 bytes. The first 24 bytes are just added 0
        data.copy_from_slice(&account_bytes[24..32]);
        let account_result = AccountId::decode(&mut &data[..]);
        if account_result.is_err() {
            return Err(DispatchError::Other("Error converting AccountId"));
        }
        Ok(account_result.expect("Already checked"))
    }

    fn try_from_any(bytes: Vec<u8>) -> Result<AccountId, DispatchError> {
        if bytes[0] == VALIDATOR_ID_CAUSES_CONVERSION_ERROR {
            return Err(DispatchError::Other("Error converting to AccountId"));
        }

        let mut account_bytes: [u8; 8] = Default::default();
        account_bytes.copy_from_slice(&bytes[0..8]);

        return AccountId::decode(&mut &account_bytes[..])
            .map_err(|_| DispatchError::Other("Error converting to AccountId"));
    }
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
impl<AuthorityId: Member, AccountId: Member> NewSessionHandler<AuthorityId, AccountId> for Tuple {
    fn on_genesis_session(validators: &Vec<Validator<AuthorityId, AccountId>>) {
        for_tuples!( #( Tuple::on_genesis_session(validators); )* );
    }

    fn on_new_session(
        changed: bool,
        active_validators: &Vec<Validator<AuthorityId, AccountId>>,
        disabled_validators: &Vec<AccountId>,
    ) {
        for_tuples!( #( Tuple::on_new_session(changed, active_validators, disabled_validators); )* );
    }
}
