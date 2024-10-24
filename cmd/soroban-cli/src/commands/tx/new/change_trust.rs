use clap::{command, Parser};

use crate::{commands::tx, xdr};

#[derive(Parser, Debug, Clone)]
#[group(skip)]
pub struct Cmd {
    #[command(flatten)]
    pub tx: tx::Args,
    #[arg(long)]
    pub line: xdr::Asset,
    /// Limit for the trust line, 0 to remove the trust line
    #[arg(long, default_value = u64::MAX.to_string())]
    pub limit: i64,
}

impl From<&Cmd> for xdr::OperationBody {
    fn from(Cmd { line, limit, .. }: &Cmd) -> Self {
        xdr::OperationBody::ChangeTrust(xdr::ChangeTrustOp {
            line: line.into(),
            limit: *limit,
        })
    }
}
