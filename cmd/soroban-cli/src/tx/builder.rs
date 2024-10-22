pub mod transaction;

pub use transaction::TxExt;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Transaction contains too many operations")]
    TooManyOperations,
}
