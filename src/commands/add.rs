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

//! Add a package to the dependencies for your project.

use std::{collections::HashMap, time::Instant};

use crate::{
    cli::{VoltCommand, VoltConfig},
    core::model::lock_file::{DependencyID, DependencyLock, LockFile},
    core::utils::fetch_dep_tree,
    core::utils::voltapi::VoltPackage,
    core::utils::{install_package, State},
    core::{command::Command, VERSION},
};

use async_trait::async_trait;
use clap::Parser;
use colored::Colorize;
use futures::{stream::FuturesUnordered, StreamExt, TryStreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use miette::{IntoDiagnostic, Result};
use package_spec::PackageSpec;
use reqwest::Client;

/// Add a package to your project's dependencies
#[derive(Debug, Parser)]
pub struct Add {
    /// Packages to add to the dependencies for your project.
    packages: Vec<PackageSpec>,
}

#[async_trait]
impl VoltCommand for Add {
    async fn exec(self, config: VoltConfig) -> miette::Result<()> {
        let idk = Instant::now();

        // Load the existing package.json file
        // let (mut package_file, _package_file_path) = PackageJson::open("package.json")?;

        // Construct a path to the local and global lockfile.
        // let lockfile_path = &app.lock_file_path;

        // let global_lockfile = &app.home_dir.join(".global.lock");

        // // Load local and global lockfiles.
        // let mut lock_file =
        //     LockFile::load(lockfile_path).unwrap_or_else(|_| LockFile::new(lockfile_path));

        // let mut global_lock_file =
        //     LockFile::load(global_lockfile).unwrap_or_else(|_| LockFile::new(global_lockfile));

        // let resolve_start = Instant::now();

        let bar = ProgressBar::new_spinner()
            .with_style(ProgressStyle::default_spinner().template("{spinner:.cyan} {msg}"));

        bar.enable_steady_tick(10);

        let resolve_start = Instant::now();

        // Fetch pre-flattened dependency trees from the registry
        let responses = fetch_dep_tree(&self.packages, &bar).await?;

        let mut tree: HashMap<String, VoltPackage> = HashMap::new();

        for response in responses {
            tree.extend(response.tree);
        }

        let total = tree.len();

        bar.finish_and_clear();

        println!(
            "{} Resolved {} dependencies",
            format!("[{:.2}{}]", resolve_start.elapsed().as_secs_f32(), "s")
                .truecolor(156, 156, 156)
                .bold(),
            total.to_string().truecolor(196, 206, 255).bold()
        );

        let install_start = Instant::now();

        let bar = ProgressBar::new(total as u64);

        bar.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40.cyan/blue}] {pos:>7}/{len:7} {msg}")
                .progress_chars("=>-"),
        );

        if !config.node_modules_dir.exists() {
            std::fs::create_dir_all(&config.node_modules_dir.join(".volt/")).unwrap();
        }

        let client = Client::builder().use_rustls_tls().build().unwrap();

        let start = Instant::now();

        let node_modules_directory = config.node_modules_dir.join(".volt/");

        // pnpm linking algorithm
        for value in tree.values() {
            // None means it's not platform-specific
            // We get a list of platforms, and if our current OS isn't on this list - it means that we can skip this package
            // this is only if the package is optional

            if value.optional {
                // TODO: check if engines.nodeis compatible
                // TODO: do a CPU arch check
                if let Some(os) = &value.os {
                    if !os.contains(&config.os) && !os.contains(&format!("!{}", config.os)) {
                        continue;
                    }
                }
            }

            let mut name = value.name.clone();

            if value.name.starts_with('@') {
                // replace @ with +
                name = name.replace("/", "+");
            }

            std::fs::create_dir(node_modules_directory.join(format!("{}@{}", name, value.version)))
                .into_diagnostic()?;

            std::fs::create_dir(
                node_modules_directory
                    .join(format!("{}@{}", name, value.version))
                    .join("node_modules/"),
            )
            .into_diagnostic()?;

            std::fs::create_dir(
                node_modules_directory
                    .join(format!("{}@{}", name, value.version))
                    .join("node_modules/")
                    .join(&name),
            )
            .into_diagnostic()?;
        }

        println!("{}", start.elapsed().as_secs_f32());

        tree.values()
            .map(|data| {
                install_package(
                    &config,
                    data,
                    State {
                        http_client: client.clone(),
                    },
                )
            })
            .collect::<FuturesUnordered<_>>()
            .inspect(|_| bar.inc(1))
            .try_collect::<Vec<_>>()
            .await
            .unwrap();

        bar.finish_and_clear();

        println!(
            "{} Installed {} dependencies",
            format!("[{:.2}{}]", install_start.elapsed().as_secs_f32(), "s")
                .truecolor(156, 156, 156)
                .bold(),
            total.to_string().truecolor(196, 206, 255).bold()
        );

        // TODO: add this to the global lockfiles

        // for package in packages {
        //     package_file.add_dependency(package.to_owned());
        // }

        // Save package.json
        // package_file.save()?;

        // Save lockfiles
        // global_lock_file.save()?;
        // lock_file.save()?;
        println!("{}", idk.elapsed().as_secs_f32());

        Ok(())
    }
}
