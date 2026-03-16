use anyhow::{Context, Result};
use clap::Subcommand;

use crate::output;

/// Keyring service identifier for all stored credentials.
const SERVICE_NAME: &str = "okxweb3";

#[derive(Subcommand)]
pub enum TestCommand {
    /// Store a value in the system keyring
    Set {
        /// Key name
        key: String,
        /// Value to store
        value: String,
    },
    /// Retrieve a value from the system keyring
    Getdata {
        /// Key name
        key: String,
    },
}

pub async fn execute(cmd: TestCommand) -> Result<()> {
    match cmd {
        TestCommand::Set { key, value } => {
            let entry = keyring::Entry::new(SERVICE_NAME, &key)
                .context("failed to create keyring entry")?;
            entry
                .set_password(&value)
                .context("failed to store value in keyring")?;
            output::success(format!("stored key '{}' in keyring (service={})", key, SERVICE_NAME));
        }
        TestCommand::Getdata { key } => {
            let entry = keyring::Entry::new(SERVICE_NAME, &key)
                .context("failed to create keyring entry")?;
            match entry.get_password() {
                Ok(val) => output::success(val),
                Err(keyring::Error::NoEntry) => {
                    output::error(&format!("no entry found for key '{}'", key));
                }
                Err(e) => {
                    return Err(e).context("failed to retrieve value from keyring");
                }
            }
        }
    }
    Ok(())
}
