#[cfg(test)]
mod tests {
    use core::fmt::Debug;

    use codec::{Decode, Encode, MaxEncodedLen};
    use frame_support::traits::Get;
    use scale_info::TypeInfo;
    use sp_runtime::BoundedVec;

    use crate::set::BoundedOrderedSet;

    #[derive(Clone, Encode, Decode, Debug, TypeInfo, MaxEncodedLen)]
    pub struct MaxBound;

    impl Get<u32> for MaxBound {
        fn get() -> u32 {
            const MAX_NOMINATIONS: u32 = 100;
            MAX_NOMINATIONS
        }
    }

    fn vec_to_ordered_set<T: Ord + Debug + Clone, S: Get<u32>>(
        values: Vec<T>,
    ) -> BoundedOrderedSet<T, S> {
        let bounded_vec: BoundedVec<T, S> = BoundedVec::truncate_from(values);

        let mut new_bounded_vec = BoundedVec::default(); // Create a new BoundedVec
        for value in bounded_vec.iter() {
            new_bounded_vec
                .try_push(value.clone())
                .expect("Failed to push value into new BoundedVec");
        }

        BoundedOrderedSet::from(new_bounded_vec)
    }

    #[test]
    fn test_new() {
        let set: BoundedOrderedSet<i32, MaxBound> = BoundedOrderedSet::new();
        assert!(set.0.is_empty());
    }

    #[test]
    fn test_from() {
        let vec = vec![3, 1, 4, 1, 5, 9];
        let set: BoundedOrderedSet<i32, MaxBound> = vec_to_ordered_set(vec);

        assert_eq!(set.0.len(), 5);
        assert_eq!(set.0[0], 1);
        assert_eq!(set.0[1], 3);
        assert_eq!(set.0[2], 4);
        assert_eq!(set.0[3], 5);
    }

    #[test]
    fn test_try_insert() {
        let mut set: BoundedOrderedSet<u32, MaxBound> = BoundedOrderedSet::new();

        assert_eq!(set.try_insert(3), Ok(true));
        assert_eq!(set.try_insert(1), Ok(true));
        assert_eq!(set.try_insert(4), Ok(true));
        assert_eq!(set.try_insert(1), Ok(false));
        assert_eq!(set.0.len(), 3);
        assert_eq!(set.0[0], 1);
        assert_eq!(set.0[1], 3);
        assert_eq!(set.0[2], 4);
    }

    #[test]
    fn test_remove() {
        let mut set = vec_to_ordered_set::<i32, MaxBound>(vec![3, 1, 4]);

        assert_eq!(set.remove(&3), true);
        assert_eq!(set.remove(&2), false);

        assert_eq!(set.0.len(), 2);
        assert_eq!(set.0[0], 1);
        assert_eq!(set.0[1], 4);
    }

    #[test]
    fn test_contains() {
        let set = vec_to_ordered_set::<i32, MaxBound>(vec![3, 1, 4]);

        assert_eq!(set.contains(&3), true);
        assert_eq!(set.contains(&2), false);
    }
}
