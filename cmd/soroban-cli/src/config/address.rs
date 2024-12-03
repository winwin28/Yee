use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use crate::xdr;

use super::{locator, secret};

/// Address can be either a public key or eventually an alias of a address.
#[derive(Clone, Debug)]
pub enum Address {
    MuxedAccount(xdr::MuxedAccount),
    AliasOrSecret(String),
}

impl Default for Address {
    fn default() -> Self {
        Address::AliasOrSecret(String::default())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Locator(#[from] locator::Error),
    #[error(transparent)]
    Secret(#[from] secret::Error),
    #[error("Address cannot be used to sign {0}")]
    CannotSign(xdr::MuxedAccount),
    #[error("Invalid key name: {0}\n only alphanumeric characters, `_`and `-` are allowed")]
    InvalidKeyName(String),
}

impl FromStr for Address {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(xdr::MuxedAccount::from_str(value).map_or_else(
            |_| Address::AliasOrSecret(value.to_string()),
            Address::MuxedAccount,
        ))
    }
}

impl Address {
    pub fn resolve_muxed_account(
        &self,
        locator: &locator::Args,
        hd_path: Option<usize>,
    ) -> Result<xdr::MuxedAccount, Error> {
        match self {
            Address::MuxedAccount(muxed_account) => Ok(muxed_account.clone()),
            Address::AliasOrSecret(alias) => alias.parse().or_else(|_| {
                Ok(xdr::MuxedAccount::Ed25519(
                    locator.read_identity(alias)?.public_key(hd_path)?.0.into(),
                ))
            }),
        }
    }

    pub fn resolve_secret(&self, locator: &locator::Args) -> Result<secret::Secret, Error> {
        match &self {
            Address::MuxedAccount(muxed_account) => Err(Error::CannotSign(muxed_account.clone())),
            Address::AliasOrSecret(alias) => Ok(locator.read_identity(alias)?),
        }
    }
}

#[derive(Clone, Debug)]
pub struct KeyName(pub String);

impl std::ops::Deref for KeyName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::str::FromStr for KeyName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.chars().all(allowed_char) {
            return Err(Error::InvalidKeyName(s.to_string()));
        }
        if s == "ledger" {
            return Err(Error::InvalidKeyName(s.to_string()));
        }
        Ok(KeyName(s.to_string()))
    }
}

impl Display for KeyName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn allowed_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}
