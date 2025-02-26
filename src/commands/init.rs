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

use crate::{
    cli::{VoltCommand, VoltConfig},
    core::{
        classes::init_data::{InitData, License},
        prompt::prompts::{Confirm, Input, Select},
        utils, VERSION,
    },
    App, Command,
};

use async_trait::async_trait;
use clap::Parser;
use colored::Colorize;
use miette::Result;
use regex::Regex;
use std::{fs::File, io::Write, sync::Arc, time::Instant};
use tracing::error;

/// Interactively create or update a package.json file for a project
#[derive(Debug, Parser)]
pub struct Init {
    /// Use default options
    #[clap(short, long)]
    yes: bool,
}

#[async_trait]
impl VoltCommand for Init {
    /// Execute the `volt init` command
    ///
    /// Interactively create or update a package.json file for a project.
    /// ## Arguments
    /// * `app` - Instance of the command (`Arc<App>`)
    /// * `packages` - List of packages to add (`Vec<String>`)
    /// * `flags` - List of flags passed in through the CLI (`Vec<String>`)
    /// ## Examples
    /// ```
    /// // Initialize a new package.json file without any prompts
    /// // .exec() is an async call so you need to await it
    /// Init.exec(app, vec![], vec!["--yes"]).await;
    /// ```
    /// ## Returns
    /// * `Result<()>`
    async fn exec(self, config: VoltConfig) -> miette::Result<()> {
        let start = Instant::now();
        // get cwd
        let cwd = config
            .current_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let data = if self.yes {
            // Set name to current directory name
            let name = cwd;
            let version = "0.1.0".to_string();

            let description = None;

            let main = "index.js".to_string();

            let author = {
                let git_user_name =
                    utils::get_git_config(&config, "user.name")?.unwrap_or_else(String::new);

                let git_email =
                    utils::get_git_config(&config, "user.email")?.unwrap_or_else(String::new);

                if git_user_name.is_empty() && git_email.is_empty() {
                    None
                } else {
                    Some([git_user_name, format!("<{}>", git_email)].join(" "))
                }
            };

            let repository = utils::get_git_config(&config, "remote.origin.url")?;

            let license = License::default();

            InitData {
                name,
                version,
                description,
                main,
                repository,
                author,
                license,
                private: None,
            }
        } else {
            // Get "name"
            let input = Input {
                message: String::from("name"),
                default: Some(cwd),
                allow_empty: false,
            };

            let mut name;
            name = input.run().unwrap_or_else(|err| {
                eprintln!("{}", err);
                std::process::exit(1);
            });

            let re_name =
                Regex::new("^(?:@[a-z0-9-*~][a-z0-9-*._~]*/)?[a-z0-9-~][a-z0-9-._~]*$").unwrap();

            if re_name.is_match(&name) { // returns bool
                 // Do nothing
                 // It passes and everyone is happy
                 // it continues with the other code
            } else {
                println!("{}", "Name cannot contain special characters".red());
                loop {
                    let input: Input = Input {
                        message: String::from("name"),
                        default: Some(
                            config
                                .current_dir
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_string(),
                        ),
                        allow_empty: false,
                    };

                    name = input.run().unwrap_or_else(|err| {
                        eprintln!("{}", err);
                        std::process::exit(1);
                    });

                    if re_name.is_match(&name) {
                        break;
                    } else {
                        println!("{}", "Name cannot contain special characters".red());
                    }
                }
            }

            // Get "version"
            let input: Input = Input {
                message: String::from("version"),
                default: Some(String::from("1.0.0")),
                allow_empty: false,
            };

            let version = input.run().unwrap_or_else(|err| {
                error!("{}", err.to_string());
                std::process::exit(1);
            });

            // Get "description"
            let input: Input = Input {
                message: String::from("description"),
                default: None,
                allow_empty: true,
            };

            let description = input.run().unwrap_or_else(|err| {
                error!("{}", err.to_string());
                std::process::exit(1);
            });

            // Get "main"
            let input: Input = Input {
                message: String::from("main"),
                default: Some(String::from("index.js")),
                allow_empty: false,
            };

            let main = input.run().unwrap_or_else(|err| {
                error!("{}", err.to_string());
                std::process::exit(1);
            });

            // Get "author"
            let git_user_name =
                utils::get_git_config(&config, "user.name")?.unwrap_or_else(String::new);

            let git_email = format!(
                "<{}>",
                utils::get_git_config(&config, "user.email")?.unwrap_or_else(String::new)
            );

            let author;

            if git_user_name != String::new() && git_email != String::new() {
                let input: Input = Input {
                    message: String::from("author"),
                    default: Some(format!("{} {}", git_user_name, git_email)),
                    allow_empty: true,
                };

                author = input.run().unwrap_or_else(|err| {
                    error!("{}", err.to_string());
                    std::process::exit(1);
                });
            } else {
                let input: Input = Input {
                    message: String::from("author"),
                    default: None,
                    allow_empty: true,
                };
                author = input.run().unwrap_or_else(|err| {
                    error!("{}", err.to_string());
                    std::process::exit(1);
                });
            }

            // Get "repository"
            let input: Input = Input {
                message: String::from("repository"),
                default: None,
                allow_empty: true,
            };

            let repository = input.run().unwrap_or_else(|err| {
                error!("{}", err.to_string());
                std::process::exit(1);
            });

            let licenses: Vec<String> = License::options();

            let select = Select {
                message: String::from("License"),
                paged: true,
                selected: Some(1),
                items: licenses,
            };

            select.run().unwrap_or_else(|err| {
                error!("{}", err.to_string());
                std::process::exit(1);
            });

            let license = License::from_index(select.selected.unwrap()).unwrap();

            let input = Confirm {
                message: String::from("private"),
                default: false,
            };

            let private = input.run().unwrap_or_else(|err| {
                error!("{}", err.to_string());
                std::process::exit(1);
            });

            InitData {
                name,
                version,
                description: Some(description),
                main,
                repository: Some(repository),
                author: Some(author),
                license,
                private: Some(private),
            }
        };

        let mut file = File::create(r"package.json").unwrap();
        if let Err(error) = file.write(data.dump().as_bytes()) {
            error!(
                "{} {}",
                "Failed to create package.json -",
                error.to_string().bright_yellow().bold()
            );
            std::process::exit(1);
        }
        println!("{}", start.elapsed().as_secs_f32());
        println!("{}", "Successfully Initialized package.json".bright_green());

        Ok(())
    }
}
