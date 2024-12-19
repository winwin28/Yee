use clap::arg;

use crate::commands::config::{key, locator};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] locator::Error),

    #[error(transparent)]
    Key(#[from] key::Error),

    #[error(transparent)]
    StrKey(#[from] stellar_strkey::DecodeError),
}

#[derive(Debug, clap::Parser, Clone)]
#[group(skip)]
pub struct Cmd {
    /// Name of identity to lookup, default test identity used if not provided
    pub name: String,

    /// If identity is a seed phrase use this hd path, default is 0
    #[arg(long)]
    pub hd_path: Option<usize>,

    #[command(flatten)]
    pub locator: locator::Args,
}

impl Cmd {
    pub fn run(&self) -> Result<(), Error> {
        println!("{}", self.public_key()?);
        Ok(())
    }

    pub fn private_key(&self) -> Result<ed25519_dalek::SigningKey, Error> {
        Ok(ed25519_dalek::SigningKey::from_bytes(
            &self
                .locator
                .read_identity(&self.name)?
                .private_key(self.hd_path)?
                .0,
        ))
    }

    pub fn public_key(&self) -> Result<stellar_strkey::ed25519::PublicKey, Error> {
        if let Ok(key) = stellar_strkey::ed25519::PublicKey::from_string(&self.name) {
            Ok(key)
        } else {
            Ok(stellar_strkey::ed25519::PublicKey::from_payload(
                self.private_key()?.verifying_key().as_bytes(),
            )?)
        }
    }
}
