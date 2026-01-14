// Copyright 2025 Aventus Network Services (UK) Ltd.

pub mod ethereum_converters {
    use sp_std::vec::Vec;
    pub fn into_32_be_bytes(bytes: &[u8]) -> Vec<u8> {
        let mut vec = Vec::new();
        vec.extend(bytes.iter().copied());
        vec.resize(32, 0);
        vec.reverse();
        return vec
    }

    #[cfg(test)]
    pub fn get_topic_32_bytes(n: u8) -> Vec<u8> {
        return vec![n; 32]
    }
}

pub mod utilities {
    use codec::Decode;
    use sp_core::{sr25519, Pair};
    use sp_runtime::traits::Verify;
    pub type AccountIdTest = u128;
    pub type SignatureTest = sr25519::Signature;
    pub type TestAccountIdPK = <SignatureTest as Verify>::Signer;

    pub struct TestAccount {
        pub seed: [u8; 32],
    }

    impl TestAccount {
        pub fn new(seed: [u8; 32]) -> Self {
            TestAccount { seed }
        }

        pub fn account_id(&self) -> TestAccountIdPK {
            return TestAccountIdPK::decode(&mut self.key_pair().public().to_vec().as_slice())
                .unwrap();
        }

        pub fn key_pair(&self) -> sr25519::Pair {
            return sr25519::Pair::from_seed(&self.seed);
        }

        pub fn public_key(&self) -> sr25519::Public {
            return self.key_pair().public()
        }
    }

    pub fn get_account_from_seed(seed: [u8; 32]) -> TestAccountIdPK {
        TestAccount::new(seed).account_id()
    }

    pub fn get_account_from_mnemonic(mnemonic: &str) -> TestAccountIdPK {
        let seed = sr25519::Pair::from_phrase(mnemonic, None).unwrap().1;
        return TestAccount::new(seed).account_id()
    }

    pub fn get_test_account_from_mnemonic(mnemonic: &str) -> TestAccount {
        let seed = sr25519::Pair::from_phrase(mnemonic, None).unwrap().1;
        return TestAccount::new(seed)
    }

    pub fn get_account(index: u8) -> TestAccountIdPK {
        TestAccount::new([index; 32]).account_id()
    }

    // copied from substrate-test-utils to avoid errors in dependencies.
    #[macro_export]
    macro_rules! assert_eq_uvec {
        ($x:expr, $y:expr $(,)?) => {{
            ($x).iter().for_each(|e| {
                if !($y).contains(e) {
                    panic!(
                        "assert_eq_uvec! failed: left has an element not in right.\nleft:  {:?}\nright: {:?}\nmissing: {:?}",
                        $x, $y, e
                    );
                }
            });

            ($y).iter().for_each(|e| {
                if !($x).contains(e) {
                    panic!(
                        "assert_eq_uvec! failed: right has an element not in left.\nleft:  {:?}\nright: {:?}\nmissing: {:?}",
                        $x, $y, e
                    );
                }
            });
        }};
    }
}
