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

use crate::{
    commands::init::Init,
    core::model::lock_file::{DependencyID, DependencyLock, LockFile},
    core::utils::package::PackageJson,
    core::utils::voltapi::{VoltPackage, VoltResponse},
    core::utils::{constants::PROGRESS_CHARS, install_extract_package, npm::parse_versions},
    core::{command::Command, VERSION},
    App,
};

use std::collections::HashMap;
use std::io::Write;
use std::process::exit;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use futures::{stream::FuturesUnordered, StreamExt, TryStreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use miette::Result;

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: Option<String>,
}

/// Struct implementation for the `Add` command.
#[derive(Clone)]
pub struct Add {}

#[async_trait]
impl Command for Add {
    /// Display a help menu for the `volt add` command.
    fn help() -> String {
        format!(
            r#"volt {}

            Add a package to your project's dependencies.
            Usage: {} {} {} {}
            Options:

            {} {} Output the version number.
            {} {} Output verbose messages on internal operations.
            {} {} Adds package as a dev dependency
            {} {} Disable progress bar."#,
            VERSION.bright_green().bold(),
            "volt".bright_green().bold(),
            "add".bright_purple(),
            "[packages]".white(),
            "[flags]".white(),
            "--version".blue(),
            "(-ver)".yellow(),
            "--verbose".blue(),
            "(-v)".yellow(),
            "--dev".blue(),
            "(-D)".yellow(),
            "--no-progress".blue(),
            "(-np)".yellow()
        )
    }

    /// Execute the `volt add` command
    ///
    /// Adds a package to dependencies for your project.
    /// ## Arguments
    /// * `app` - Instance of the command (`Arc<App>`)
    /// * `packages` - List of packages to add (`Vec<String>`)
    /// * `flags` - List of flags passed in through the CLI (`Vec<String>`)
    /// ## Examples
    /// ```rust
    /// // Add react to your dependencies with logging level verbose
    /// // .exec() is an async call so you need to await it
    /// Add.exec(app, vec!["react"], vec!["--verbose"]).await;
    /// ```
    /// ## Returns
    /// * `Result<()>`
    async fn exec(app: Arc<App>) -> Result<()> {
        let mut packages = app
            .args
            .values_of("package-names")
            .unwrap()
            .map(|v| v.to_string())
            .collect::<Vec<String>>();

        packages.dedup();

        let versions = parse_versions(&packages).await?;

        println!("{:#?}", versions);
        std::process::exit(0);

        // Check if package.json exists, otherwise, let the user know.
        if !app.current_dir.join("package.json").exists() {
            crate::error!("no package.json found.");
            print!("Do you want to initialize package.json (Y/N): ");

            std::io::stdout().flush().expect("Could not flush stdout");

            let mut input: String = String::new();

            std::io::stdin().read_line(&mut input).unwrap();

            if input.trim().to_lowercase() != "y" || input.trim().to_lowercase() != "yes" {
                exit(0);
            } else {
                Init::exec(app.clone()).await.unwrap();
            }
        }

        // Load the existing package.json file
        let mut package_file = PackageJson::from("package.json");

        let start = Instant::now();

        // Get the integrity hash and version of the requested package.

        let lockfile_path = &app.lock_file_path;

        let global_lockfile = &app.home_dir.join(".global.lock");

        // Load global and local lock files.

        let mut lock_file =
            LockFile::load(lockfile_path).unwrap_or_else(|_| LockFile::new(lockfile_path));

        let mut global_lock_file =
            LockFile::load(global_lockfile).unwrap_or_else(|_| LockFile::new(global_lockfile));

        // Create progress bar for resolving dependencies.

        let progress_bar = ProgressBar::new(packages.len() as u64);

        progress_bar.set_style(
            ProgressStyle::default_bar()
                .progress_chars(PROGRESS_CHARS)
                .template(&format!(
                    "{} [{{bar:40.magenta/blue}}] {{msg:.blue}}",
                    "Resolving Dependencies".bright_blue()
                )),
        );

        let start = Instant::now();
        let responses: Result<Vec<VoltResponse>> = if packages.len() > 1 {
            crate::core::utils::get_volt_response_multi(packages.clone(), &progress_bar)
                .await
                .into_iter()
                .collect()
        } else {
            Ok(vec![
                crate::core::utils::get_volt_response(packages[0].clone()).await?,
            ])
        };
        println!("{}", start.elapsed().as_secs_f32());
        let mut dependencies: HashMap<String, VoltPackage> = HashMap::new();

        let responses = responses?;

        for res in responses.iter() {
            let current_version = res.versions.get(&res.version).unwrap();
            dependencies.extend(current_version.clone());
        }

        let end = Instant::now();

        progress_bar.finish_with_message("[OK]".bright_green().to_string());

        let length = dependencies.len();

        if length == 1 {
            let duration = (end - start).as_secs_f32();

            if duration < 0.001 {
                println!(
                    "{}: resolved 1 dependency in {:.5}s.",
                    "success".bright_green(),
                    duration
                );
            } else {
                println!(
                    "{}: resolved 1 dependency in {:.2}s.",
                    "success".bright_green(),
                    duration
                );
            }
        } else {
            let duration = (end - start).as_secs_f32();

            if duration < 0.001 {
                println!(
                    "{}: resolved {} dependencies in {:.4}s.",
                    "success".bright_green(),
                    length,
                    duration
                );
            } else {
                println!(
                    "{}: resolved {} dependencies in {:.2}s.",
                    "success".bright_green(),
                    length,
                    duration
                );
            }
        }

        let mut dependencies: Vec<_> = dependencies
            .iter()
            .map(|(_name, object)| {
                let mut lock_dependencies: Vec<String> = vec![];

                if let Some(peer_deps) = &object.peer_dependencies {
                    for dep in peer_deps {
                        if !crate::core::utils::check_peer_dependency(&dep) {
                            progress_bar.println(format!(
                                "{}{} {} has unmet peer dependency {}",
                                " warn ".black().bright_yellow(),
                                ":",
                                object.name.bright_cyan(),
                                &dep.bright_yellow()
                            ));
                        }
                    }
                }

                if let Some(dependencies) = &object.dependencies {
                    for dep in dependencies {
                        lock_dependencies.push(dep.to_string());
                    }
                }

                let object_instance = object.clone();

                lock_file.dependencies.insert(
                    DependencyID(object_instance.name, object_instance.version),
                    DependencyLock {
                        name: object.name.clone(),
                        version: object.version.clone(),
                        tarball: object.tarball.clone(),
                        integrity: object.integrity.clone(),
                        dependencies: lock_dependencies.clone(),
                    },
                );

                let second_instance = object.clone();

                global_lock_file.dependencies.insert(
                    DependencyID(second_instance.name, second_instance.version.to_owned()),
                    DependencyLock {
                        name: object.name.clone(),
                        version: object.version.clone(),
                        tarball: object.tarball.clone(),
                        integrity: object.integrity.clone(),
                        dependencies: lock_dependencies,
                    },
                );

                object
            })
            .collect();

        let progress_bar = ProgressBar::new(dependencies.len() as u64);

        progress_bar.set_style(
            ProgressStyle::default_bar()
                .progress_chars(PROGRESS_CHARS)
                .template(&format!(
                    "{} [{{bar:40.magenta/blue}}] {{msg:.blue}}",
                    "Installing Packages".bright_blue()
                )),
        );

        dependencies.dedup();

        dependencies
            .into_iter()
            .map(|v| install_extract_package(&app, &v))
            .collect::<FuturesUnordered<_>>()
            .inspect(|_| progress_bar.inc(1))
            .try_collect::<()>()
            .await
            .unwrap();

        progress_bar.finish();

        // for (index, _) in packages.iter().enumerate() {
        //     let data = &versions[index];

        //     package_file.add_dependency(data.0.clone(), data.1.clone());
        // }

        Ok(())
    }
}
