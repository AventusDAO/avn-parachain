// Copyright 2026 Aventus DAO Ltd

pub mod ethereum_converters {
    use sp_std::vec::Vec;
    pub fn into_32_be_bytes(bytes: &[u8]) -> Vec<u8> {
        let mut vec = Vec::new();
        vec.extend(bytes.iter().copied());
        vec.resize(32, 0);
        vec.reverse();
        return vec;
    }

    #[cfg(test)]
    pub fn get_topic_32_bytes(n: u8) -> Vec<u8> {
        return vec![n; 32];
    }
}

pub mod utilities {
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
