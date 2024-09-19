use crate::{
    signer::{
        self,
        types::{sign_txn_env, Transaction},
    },
    xdr::TransactionEnvelope,
};
use clap::arg;
use soroban_env_host::xdr::WriteXdr;
use soroban_sdk::xdr::Limits;
use stellar_strkey::ed25519::PublicKey;
use url::Url;

use super::{
    locator,
    network::{self, Network},
    secret::{self, Secret},
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Network(#[from] network::Error),
    #[error(transparent)]
    Signer(#[from] signer::types::Error),
    #[error(transparent)]
    Secret(#[from] secret::Error),
    #[error(transparent)]
    Locator(#[from] locator::Error),
    #[error(transparent)]
    Rpc(#[from] soroban_rpc::Error),
    #[error("No sign with key provided")]
    NoSignWithKey,
    #[error(transparent)]
    StrKey(#[from] stellar_strkey::DecodeError),
    #[error(transparent)]
    Xdr(#[from] soroban_env_host::xdr::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Open(#[from] std::io::Error),
}

#[derive(Debug, clap::Args, Clone, Default)]
#[group(skip)]
pub struct Args {
    /// Sign with a local key. Can be an identity (--sign-with-key alice), a secret key (--sign-with-key SC36…), or a seed phrase (--sign-with-key "kite urban…"). If using seed phrase, `--hd-path` defaults to the `0` path.
    #[arg(long, conflicts_with = "sign_with_lab", env = "STELLAR_SIGN_WITH_KEY")]
    pub sign_with_key: Option<String>,
    /// Sign with laboratory
    #[arg(long, conflicts_with = "sign_with_key", env = "STELLAR_SIGN_WITH_LAB")]
    pub sign_with_lab: bool,
    /// Lab URL for `sign_with_lab`
    #[arg(
        long,
        conflicts_with = "sign_with_key",
        env = "STELLAR_SIGN_WITH_LAB_URL",
        default_value = "https://lab.stellar.org/transaction/cli-sign"
    )]
    pub lab_url: String,

    #[arg(long, conflicts_with = "sign_with_lab")]
    /// If using a seed phrase to sign, sets which hierarchical deterministic path to use, e.g. `m/44'/148'/{hd_path}`. Example: `--hd-path 1`. Default: `0`
    pub hd_path: Option<usize>,
}

impl Args {
    pub fn secret(&self, locator: &locator::Args) -> Result<Secret, Error> {
        let account = self.sign_with_key.as_deref().ok_or(Error::NoSignWithKey)?;
        Ok(locator.account(account)?)
    }

    pub async fn sign_txn_env(
        &self,
        tx: TransactionEnvelope,
        locator: &locator::Args,
        network: &Network,
        quiet: bool,
    ) -> Result<TransactionEnvelope, Error> {
        let secret = self.secret(locator)?;
        let signer = secret.signer(self.hd_path, false, quiet)?;
        self.sign_tx_env_with_signer(&signer, tx, network).await
    }

    pub async fn sign_tx_env_with_signer(
        &self,
        signer: &(impl Transaction + std::marker::Sync),
        tx_env: TransactionEnvelope,
        network: &Network,
    ) -> Result<TransactionEnvelope, Error> {
        Ok(sign_txn_env(signer, tx_env, network).await?)
    }

    pub fn sign_tx_env_with_lab(
        &self,
        network: &Network,
        tx_env: &TransactionEnvelope,
    ) -> Result<(), Error> {
        let passphrase = network.network_passphrase.clone();
        let xdr_buffer = tx_env.to_xdr_base64(Limits::none())?;

        let mut url = Url::parse(&self.lab_url)?;
        url.query_pairs_mut()
            .append_pair("networkPassphrase", &passphrase)
            .append_pair("xdr", &xdr_buffer);

        let txn_sign_url = url.to_string();

        println!("Opening lab to sign transaction: {}", &txn_sign_url);
        open::that(txn_sign_url)?;

        Ok(())
    }
}
