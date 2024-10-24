use clap::{command, Parser};

use crate::{commands::tx, xdr};

#[derive(Parser, Debug, Clone)]
#[group(skip)]
pub struct Cmd {
    #[command(flatten)]
    pub tx: tx::Args,
    /// Account to send to, e.g. `GBX...`
    #[arg(long)]
    pub destination: xdr::MuxedAccount,
    /// Asset to send, default native, e.i. XLM
    #[arg(long, default_value = "native")]
    pub asset: xdr::Asset,
    /// Amount of the aforementioned asset to send.
    #[arg(long)]
    pub amount: i64,
}

impl From<&Cmd> for xdr::OperationBody {
    fn from(
        Cmd {
            destination,
            asset,
            amount,
            ..
        }: &Cmd,
    ) -> Self {
        xdr::OperationBody::Payment(xdr::PaymentOp {
            destination: destination.into(),
            asset: asset.into(),
            amount: *amount,
        })
    }
}
