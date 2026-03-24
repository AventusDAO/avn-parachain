use crate::{configs::AssetRegistryStringLimit, Balance, CurrencyId};
use codec::{Decode, Encode, MaxEncodedLen};
use orml_traits::asset_registry::{
    AssetMetadata, AssetProcessor, AvnAssetLocation, AvnAssetMetadata,
};
use polkadot_sdk::sp_runtime::DispatchError;
use scale_info::TypeInfo;

#[derive(
    Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen,
)]
/// Implements orml_traits::asset_registry::AssetProcessor. We use an Asset enum for our AssetId.
pub struct AvnAssetProcessor;

impl
    AssetProcessor<
        CurrencyId,
        AssetMetadata<Balance, AvnAssetMetadata, AvnAssetLocation, AssetRegistryStringLimit>,
    > for AvnAssetProcessor
{
    fn pre_register(
        id: Option<CurrencyId>,
        metadata: AssetMetadata<
            Balance,
            AvnAssetMetadata,
            AvnAssetLocation,
            AssetRegistryStringLimit,
        >,
    ) -> Result<
        (
            CurrencyId,
            AssetMetadata<Balance, AvnAssetMetadata, AvnAssetLocation, AssetRegistryStringLimit>,
        ),
        DispatchError,
    > {
        match id {
            Some(id) => Ok((id, metadata)),
            None => Err(DispatchError::Other("asset-registry: AssetId is required")),
        }
    }

    fn post_register(
        _id: CurrencyId,
        _asset_metadata: AssetMetadata<
            Balance,
            AvnAssetMetadata,
            AvnAssetLocation,
            AssetRegistryStringLimit,
        >,
    ) -> Result<(), DispatchError> {
        Ok(())
    }
}
