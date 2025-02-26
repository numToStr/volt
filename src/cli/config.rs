/*
    Copyright 2021 Volt Contributors

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

use crate::core::utils::{enable_ansi_support, errors::VoltError};

use clap::ArgMatches;
use clap::Parser;
use dirs::home_dir;
use miette::Result;
use package_spec::{parse_package_spec, PackageSpec};
use sha1::Digest;
use sha2::Sha512;
use ssri::{Algorithm, Integrity};
use std::{env, path::PathBuf};

#[derive(Debug, Clone, Parser)]
pub struct VoltConfig {
    #[clap(short, long)]
    pub current_dir: PathBuf,

    #[clap(short, long)]
    pub home_dir: PathBuf,

    #[clap(short, long)]
    pub node_modules_dir: PathBuf,

    #[clap(short, long)]
    pub volt_dir: PathBuf,

    #[clap(short, long)]
    pub lock_file_path: PathBuf,

    #[clap(short, long)]
    pub os: String,
}

impl VoltConfig {
    pub fn initialize(args: &ArgMatches) -> Result<VoltConfig> {
        enable_ansi_support().unwrap();

        // Current Directory
        let current_directory = env::current_dir().map_err(|e| VoltError::EnvironmentError {
            env: "CURRENT_DIRECTORY".to_string(),
            source: e,
        })?;

        // Home Directory: /username or C:\Users\username
        let home_directory = home_dir().ok_or(VoltError::GetHomeDirError)?;

        // node_modules/
        let node_modules_directory = current_directory.join("node_modules");

        // Volt Global Directory: /username/.volt or C:\Users\username\.volt
        let volt_dir = home_directory.join(".volt");

        // Create volt directory if it doesn't exist
        std::fs::create_dir_all(&volt_dir).map_err(VoltError::CreateDirError)?;

        // ./volt.lock
        let lock_file_path = current_directory.join("volt.lock");

        Ok(VoltConfig {
            current_dir: current_directory,
            home_dir: home_directory,
            node_modules_dir: node_modules_directory,
            volt_dir,
            lock_file_path,
            os: std::env::consts::OS.to_string(),
        })
    }

    /// Calculate the hash of a tarball
    ///
    /// ## Examples
    /// ```rs
    /// calc_hash(bytes::Bytes::new(), ssri::Algorithm::Sha1)?;
    /// ```
    /// ## Returns
    /// * Result<String>
    pub fn calc_hash(data: &bytes::Bytes, algorithm: Algorithm) -> Result<String> {
        let integrity;

        if algorithm == Algorithm::Sha1 {
            let hash = ssri::IntegrityOpts::new()
                .algorithm(Algorithm::Sha1)
                .chain(&data)
                .result();

            integrity = format!("sha1-{}", hash.to_hex().1);
        } else {
            integrity = ssri::IntegrityOpts::new()
                .algorithm(Algorithm::Sha512)
                .chain(&data)
                .result()
                .to_string();
        }

        Ok(integrity)
    }
}
