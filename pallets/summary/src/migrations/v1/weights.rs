use frame_support::weights::Weight;

pub trait WeightInfo {
    fn step() -> Weight;
}

pub struct SubstrateWeight<T>(core::marker::PhantomData<T>);

impl<T> WeightInfo for SubstrateWeight<T>
where
    T: frame_system::Config,
    crate::default_weights::SubstrateWeight<T>: crate::default_weights::WeightInfo,
{
    fn step() -> Weight {
        <crate::default_weights::SubstrateWeight<T> as crate::default_weights::WeightInfo>::mbm_migration_step()
    }
}
