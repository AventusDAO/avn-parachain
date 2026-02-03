use std::{error, fmt};

/// Error types for merkle tree and extrinsic utils.
#[derive(Debug, Clone)]
pub enum TreeError {
    DecodeError = 1,
    ResponseError = 2,
    InvalidExtrinsicInLocalDB = 3,
    ErrorGettingBlockData = 4,
    BlockDataNotFound = 5,
    BlockNotFinalised = 6,
    ErrorGeneratingRoot = 7,
    LeafDataEmpty = 8,
    EmptyLeaves = 9,
}

impl From<TreeError> for i32 {
    fn from(e: TreeError) -> i32 {
        match e {
            TreeError::DecodeError => 1_i32,
            TreeError::ResponseError => 2_i32,
            TreeError::InvalidExtrinsicInLocalDB => 3_i32,
            TreeError::ErrorGettingBlockData => 4_i32,
            TreeError::BlockDataNotFound => 5_i32,
            TreeError::BlockNotFinalised => 6_i32,
            TreeError::ErrorGeneratingRoot => 7_i32,
            TreeError::LeafDataEmpty => 8_i32,
            TreeError::EmptyLeaves => 9_i32,
        }
    }
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TreeError: {:?}", self)
    }
}

impl error::Error for TreeError {}
