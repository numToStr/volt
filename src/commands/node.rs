/*
 *    Copyright 2021 Volt Contributors
 *
 *    Licensed under the Apache License, Version 2.0 (the "License");
 *    you may not use this file except in compliance with the License.
 *    You may obtain a copy of the License at
 *
 *        http://www.apache.org/licenses/LICENSE-2.0
 *
 *    Unless required by applicable law or agreed to in writing, software
 *    distributed under the License is distributed on an "AS IS" BASIS,
 *    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *    See the License for the specific language governing permissions and
 *    limitations under the License.
 */

//! Manage local node versions

use std::{
    env,
    fmt::Display,
    fs::File,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    str,
};

use base64::decode;
use clap::ArgMatches;
use futures::io;
use isahc::version;
use lzma_rs::xz_decompress;
use miette::Result;
use node_semver::{Range, Version};
use serde::{Deserialize, Deserializer};
use tempfile::tempdir;
use tokio::fs;

const PLATFORM: Os = if cfg!(target_os = "windows") {
    Os::Windows
} else if cfg!(target_os = "macos") {
    Os::Macos
} else if cfg!(target_os = "linux") {
    Os::Linux
} else {
    Os::Unknown
};

const ARCH: Arch = if cfg!(target_arch = "X86") {
    Arch::X86
} else if cfg!(target_arch = "x86_64") {
    Arch::X64
} else {
    Arch::Unknown
};

#[derive(Deserialize)]
#[serde(untagged)]
enum Lts {
    No(bool),
    Yes(String),
}

impl Into<Option<String>> for Lts {
    fn into(self) -> Option<String> {
        match self {
            Self::No(_) => None,
            Self::Yes(x) => Some(x),
        }
    }
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Lts::deserialize(deserializer)?.into())
}

#[derive(Deserialize, Clone, Debug)]
pub struct NodeVersion {
    pub version: Version,
    #[serde(deserialize_with = "deserialize")]
    pub lts: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum Os {
    Windows,
    Macos,
    Linux,
    Unknown,
}
impl Display for Os {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = match &self {
            Os::Windows => "win",
            Os::Macos => "darwin",
            Os::Linux => "linux",
            _ => unreachable!(),
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, PartialEq)]
enum Arch {
    X86,
    X64,
    Unknown,
}

impl Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = match *self {
            Arch::X86 => "x86",
            Arch::X64 => "x64",
            _ => unreachable!(),
        };
        write!(f, "{}", s)
    }
}

/// Struct implementation for the `Node` command.
#[derive(Clone)]
pub struct Node {}

impl Node {
    pub async fn download(args: &ArgMatches) -> Result<()> {
        match args.subcommand() {
            Some(("use", version)) => {
                let v = version.value_of("version").unwrap();
                use_node_version(v).await;
            }
            Some(("install", versions)) => {
                let v: Vec<&str> = versions.values_of("versions").unwrap().collect();
                download_node_version(v).await;
            }
            Some(("remove", versions)) => {
                let v: Vec<&str> = versions.values_of("versions").unwrap().collect();
                remove_node_version(v).await;
            }
            _ => {}
        }
        Ok(())
    }
}

fn get_node_path(version: &str) -> PathBuf {
    let version_dir = if cfg!(target_os = "windows") {
        version.to_string()
    } else {
        format!("node-v{}-{}-{}", version, PLATFORM, ARCH)
    };

    let node_path = dirs::data_dir()
        .unwrap()
        .join("volt")
        .join("node")
        .join(version_dir);

    node_path
}

// 32bit macos/linux systems cannot download a version of node >= 10.0.0
// They stopped making 32bit builds after that version
// https://nodejs.org/dist/
// TODO: Handle errors with file already existing and handle file creation/deletion errors
// TODO: Only make a tempdir if we have versions to download, i.e. verify all versions before
//       creating the directory
async fn download_node_version(versions: Vec<&str>) {
    tracing::debug!("On platform '{}' and arch '{}'", PLATFORM, ARCH);
    let dir: tempfile::TempDir = tempdir().unwrap();
    tracing::debug!("Temp dir is {:?}", dir);

    let mirror = "https://nodejs.org/dist";

    let node_versions: Vec<NodeVersion> = reqwest::get(format!("{}/index.json", mirror))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    for v in versions {
        let mut download_url = format!("{}/", mirror);

        let version: Option<Version> = if let Ok(ver) = v.parse() {
            if cfg!(all(unix, target_arch = "X86")) {
                if ver >= Version::parse("10.0.0").unwrap() {
                    println!("32 bit versions are not available for MacOS and Linux after version 10.0.0!");
                    continue;
                }
            }

            // TODO: Maybe suggest the closest available version if not found?

            let mut found = false;
            for n in &node_versions {
                if v == n.version.to_string() {
                    tracing::debug!("found version '{}' with URL '{}'", v, download_url);
                    found = true;
                    break;
                }
            }

            if found {
                Some(ver)
            } else {
                None
            }
        } else if let Ok(ver) = v.parse::<Range>() {
            let max_ver = node_versions
                .iter()
                .filter(|x| x.version.satisfies(&ver))
                .map(|v| v.version.clone())
                .max();

            if cfg!(all(unix, target_arch = "X86")) {
                if Range::parse(">=10").unwrap().allows_any(&ver) {
                    println!("32 bit versions are not available for macos and linux after version 10.0.0!");
                    continue;
                }
            }

            max_ver
        } else {
            println!("Unable to download {} -- not a valid version!", v);
            continue;
        };

        if version.is_none() {
            println!("Unable to find version {}!", v);
            continue;
        }

        let version = version.unwrap();

        download_url = format!("{}v{}/", download_url, version);

        download_url = if cfg!(target_os = "windows") {
            format!("{}/win-{}/node.exe", download_url, ARCH)
        } else {
            format!(
                "{}node-v{}-{}-{}.tar.xz",
                download_url, version, PLATFORM, ARCH
            )
        };

        println!("\n------------\n{}\n------------\n", download_url);

        println!("Got final URL '{}'", download_url);

        let node_path = {
            let datadir = dirs::data_dir().unwrap().join("volt").join("node");
            if !datadir.exists() {
                fs::create_dir_all(&datadir).await.unwrap();
            }
            datadir
        };

        // Get the name of the directory the tarball unpacks to
        let unpack_loc = if cfg!(target_os = "windows") {
            v // Windows locations are just saved in a folder named after the version number
              // e.g. C:\Users\Alice\AppData\Roaming\volt\node\12.2.0
        } else {
            // The unix folders are just created by the tarball,
            // which is the basename of the file
            download_url
                .split('/')
                .last()
                .unwrap()
                .strip_suffix(".tar.xz")
                .unwrap()
        };

        //let node_path = node_path.join(unpack_loc);
        if node_path.join(unpack_loc).exists() {
            println!("Node.js v{} is already installed, nothing to do!", version);
            continue;
        }

        tracing::debug!("Installing to: {:?}", node_path);

        // The name of the file we're downloading from the mirror
        let fname = download_url.split('/').last().unwrap().to_string();

        println!("Installing version {} from {} ", version, download_url);
        println!("file to download: '{}'", fname);

        let response = reqwest::get(&download_url).await.unwrap();

        let content = response.bytes().await.unwrap();

        if cfg!(target_os = "windows") {
            println!("Installing node.exe");
            fs::create_dir_all(&node_path).await.unwrap();
            let mut dest = File::create(node_path.join(&fname)).unwrap();
            dest.write_all(&content).unwrap();
        } else {
            // path to write the download to
            let xzip = dir.path().join(&fname);

            // Path to write the decompressed tarball to
            let tar = &dir.path().join(&fname.strip_suffix(".xz").unwrap());

            println!("Unzipping...");

            println!("Tar path: {:?}", tar);

            let mut tarball = File::create(tar).unwrap();
            tarball
                .write_all(&lzma::decompress(&content).unwrap())
                .unwrap();

            // Make sure the first file handle is closed
            drop(tarball);

            // Have to reopen it for reading, File::create() opens for write only
            let tarball = File::open(tar).unwrap();

            println!("Unpacking...");

            // Unpack the tarball
            tar::Archive::new(tarball).unpack(node_path).unwrap();

            // Using lzma-rs instead:
            /*
             * // Path to write the xz file to
             * let mut dest = File::create(xzip).unwrap();
             *
             *lzma_rs::xz_decompress(
             *    &mut BufReader::new(File::open(dir.path().join(&fname)).unwrap()),
             *    &mut tarball,
             *)
             *.unwrap();
             */
        }

        println!("Done!");
    }
}

async fn remove_node_version(versions: Vec<&str>) {
    for version in versions {
        let node_path = get_node_path(version);

        if node_path.exists() {
            fs::remove_dir_all(&node_path).await.unwrap();
            println!("Removed version {}", version);
        } else {
            println!(
                "Failed to remove NodeJS version {}.\nThat version was not installed.",
                version
            );
        }
    }
}

#[cfg(target_os = "windows")]
async fn use_windows(version: String) {
    let homedir = dirs::home_dir().unwrap();
    let node_path = format!(
        "{}\\AppData\\Local\\Volt\\Node\\{}\\node.exe",
        homedir.display(),
        version
    );

    let path = Path::new(&node_path);

    if path.exists() {
        println!("Using version {}", version);
        let link_dir = format!("{}\\AppData\\Local\\Volt\\bin", homedir.display());
        fs::create_dir_all(&link_dir).await.unwrap();
        let link_file = format!("{}\\AppData\\Local\\Volt\\bin\\node.exe", homedir.display());
        let link_file = Path::new(&link_file);
        if link_file.exists() {
            fs::remove_file(link_file).await.unwrap();
        }
        let link = format!("{}\\{}", link_dir, "node.exe");
        println!("{}\n{}", node_path, link);

        let symlink = std::os::windows::fs::symlink_file(node_path, link);

        match symlink {
            Ok(_) => {}
            Err(_) => {
                println!("Error: \"volt node use\" must be run as an administrator on Windows!")
            }
        }

        let path = env::var("PATH").unwrap();
        //println!("{}", path);
        if !path.contains(&link_dir) {
            //env_perm::append("PATH", &link_dir);
            let command = format!("[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + '{}', 'User')", &link_dir);
            let stdout = Command::new("Powershell")
                .args(&["-Command", &command])
                .output();
            println!("PATH environment variable updated.\nYou will need to restart your terminal for changes to apply.");
        }
    } else {
        println!("That version of node is not installed!\nTry \"volt node install {}\" to install that version.", version);
    }
}

async fn use_node_version(version: &str) {
    if PLATFORM == Os::Windows {
        #[cfg(target_os = "windows")]
        use_windows(version).await;
    } else if PLATFORM == Os::Linux {
        //
        // TODO: Need to make a link to the current "used" version of node so we can "undo" it
        //
        // TODO: Once set up a current link, check if current exists before overwriting/deleting existing entries in .local/bin
        //
        // TODO: On node remove, check if that version is being used and remove the symlinks
        //

        let node_path = get_node_path(version);

        if node_path.exists() {
            /*
             *            let cur = dirs::data_dir()
             *                .unwrap()
             *                .join("volt")
             *                .join("node")
             *                .join("current");
             *
             *            if cur.exists() {
             *                // unlink all the currently installed files
             *            }
             */

            let link_dir = dirs::home_dir().unwrap().join(".local").join("bin");
            let npm = node_path.join("bin").join("npm");
            let ld = link_dir.join("npm");

            let current = node_path.join("bin");

            for f in std::fs::read_dir(&current).unwrap() {
                let original = f.unwrap().path();
                let fname = original.file_name().unwrap();
                let link = link_dir.join(fname);

                println!("Linking to {:?} from {:?}", link, original);

                std::fs::remove_file(&link);

                let symlink = std::os::unix::fs::symlink(original, link).unwrap();
            }
        } else {
            println!("That version of node is not installed!\nTry \"volt node install {}\" to install that version.", version);
        }
    }
}
