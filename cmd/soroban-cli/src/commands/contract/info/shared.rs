use std::path::PathBuf;

use clap::arg;
use soroban_env_host::xdr;
use soroban_rpc::Client;

use crate::commands::contract::info::shared::Error::InvalidWasmHash;
use crate::commands::contract::InfoOutput;
use crate::config::{locator, network};
use crate::utils::rpc::get_remote_wasm_from_hash;
use crate::wasm;
use crate::wasm::Error::ContractIsStellarAsset;

#[derive(Debug, clap::Args, Clone, Default)]
#[command(group(
    clap::ArgGroup::new("src")
    .required(true)
    .args(& ["wasm", "wasm_hash", "contract_id"]),
))]
#[group(skip)]
pub struct Args {
    /// Wasm file to extract the data from
    #[arg(long, group = "src")]
    pub wasm: Option<PathBuf>,
    /// Wasm hash to get the data for
    #[arg(long = "wasm-hash", group = "src")]
    pub wasm_hash: Option<String>,
    /// Contract id to get the data for
    #[arg(long = "id", env = "STELLAR_CONTRACT_ID", group = "src")]
    pub contract_id: Option<String>,
    /// Format of the output
    #[arg(long, default_value = "pretty")]
    pub output: InfoOutput,
    #[command(flatten)]
    pub network: network::Args,
    #[command(flatten)]
    pub locator: locator::Args,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Network(#[from] network::Error),
    #[error(transparent)]
    Wasm(#[from] wasm::Error),
    #[error("provided wasm hash is invalid {0:?}")]
    InvalidWasmHash(String),
    #[error(transparent)]
    Rpc(#[from] soroban_rpc::Error),
}

pub async fn fetch_wasm(args: &Args) -> Result<Option<Vec<u8>>, Error> {
    let network = &args.network.get(&args.locator)?;

    let wasm = if let Some(path) = &args.wasm {
        wasm::Args { wasm: path.clone() }.read()?
    } else if let Some(wasm_hash) = &args.wasm_hash {
        let hash = hex::decode(wasm_hash)
            .map_err(|_| InvalidWasmHash(wasm_hash.clone()))?
            .try_into()
            .map_err(|_| InvalidWasmHash(wasm_hash.clone()))?;

        let hash = xdr::Hash(hash);

        let client = Client::new(&network.rpc_url)?;
        client
            .verify_network_passphrase(Some(&network.network_passphrase))
            .await?;

        get_remote_wasm_from_hash(&client, &hash).await?
    } else if let Some(contract_id) = &args.contract_id {
        let res = wasm::fetch_from_contract(contract_id, network, &args.locator).await;
        if let Some(ContractIsStellarAsset()) = res.as_ref().err() {
            return Ok(None);
        }
        res?
    } else {
        unreachable!("One of contract location arguments must be passed");
    };

    Ok(Some(wasm))
}
