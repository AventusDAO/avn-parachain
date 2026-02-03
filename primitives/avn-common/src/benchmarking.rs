use codec::{Decode, Encode};
use core::any::TypeId;
use sp_core::sr25519;
use sp_runtime::MultiSignature;

pub fn convert_sr25519_signature<Signature>(signature: sr25519::Signature) -> Signature
where
    Signature: Decode + Encode + 'static,
{
    if TypeId::of::<Signature>() == TypeId::of::<MultiSignature>() {
        let multi_sig = MultiSignature::from(signature);
        Signature::decode(&mut &multi_sig.encode()[..]).expect("MultiSignature decodes")
    } else if TypeId::of::<Signature>() == TypeId::of::<sr25519::Signature>() {
        Signature::decode(&mut &signature.encode()[..]).expect("sr25519 signature decodes")
    } else {
        Signature::decode(&mut &signature.encode()[..]).expect("signature bytes decode")
    }
}
