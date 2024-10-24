use clap::{arg, command, Parser};

use crate::{config, xdr};

#[derive(Parser, Debug, Clone)]
#[group(skip)]
pub struct Cmd {
    /// ID of the Stellar classic asset to wrap, e.g. "USDC:G...5"
    #[arg(long)]
    pub asset: xdr::Asset,

    #[command(flatten)]
    pub config: config::ArgsLocatorAndNetwork,
}
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ConfigError(#[from] config::Error),
    #[error(transparent)]
    Xdr(#[from] xdr::Error),
}
impl Cmd {
    pub fn run(&self) -> Result<(), Error> {
        println!("{}", self.contract_address()?);
        Ok(())
    }

    pub fn contract_address(&self) -> Result<stellar_strkey::Contract, Error> {
        let network = self.config.get_network()?;
        self.try_into()
    }
}

impl TryFrom<&Cmd> for stellar_strkey::Contract {
    type Error = xdr::Error;

    fn try_from(Cmd { asset, config }: &Cmd) -> Result<Self, Self::Error> {
        let network = config.get_network()?;
        let asset: Asset = asset.into()?;
        Ok(asset.into_contract_id(&network.network_passphrase)?)
    }
}
