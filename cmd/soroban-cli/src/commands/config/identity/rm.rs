use super::super::locator;
use clap::command;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Locator(#[from] locator::Error),
}

#[derive(Debug, clap::Parser)]
#[group(skip)]
pub struct Cmd {
    /// Identity to remove
    pub name: String,

    #[command(flatten)]
    pub config: locator::Args,
}

impl Cmd {
    pub fn run(&self) -> Result<(), Error> {
        Ok(self.config.remove_identity(&self.name)?)
    }
}
