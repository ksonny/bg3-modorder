use std::{ffi::OsStr, fs, path::PathBuf};

use clap::{Parser, Subcommand};
use env_logger::Env;
use error::Bg3ModError;
use log::{error, info};
use mod_meta::{read_mod_info, write_mod_settings, ModInfo};
use pak_reader::{read_file, read_file_list, read_header};
use serde::Deserialize;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    path: PathBuf,
    #[arg(short, long)]
    dest: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Settings {
    mod_path: PathBuf,
    profile_path: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    ListMods,
    ListEnabled,
    EnableMod { pattern: String },
    DisableMod { pattern: String },
}

mod error {
    #[derive(Debug)]
    pub enum Bg3ModError {
        PathNotDirectory,
    }

    impl std::fmt::Display for Bg3ModError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Bg3ModError::PathNotDirectory => write!(f, "Provided path is not a directory"),
            }
        }
    }

    impl std::error::Error for Bg3ModError {}
}

fn execute_args(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if !args.path.is_dir() {
        Err(Bg3ModError::PathNotDirectory)?;
    }

    let mut mod_infos = Vec::new();
    mod_infos.push(ModInfo {
        folder: Some("GustavDev".to_string()),
        md5: None,
        name: Some("GustavDev".to_string()),
        uuid: "28ac9ce2-2aba-8cda-b3b5-6e922f71b6b8".to_string(),
        version: Some("36028797018963968".to_string()),
    });

    let paths = fs::read_dir(args.path)?;

    for path in paths.flatten() {
        match path.path().extension().and_then(OsStr::to_str) {
            Some("pak") => {}
            _ => continue,
        }
        if !path.path().try_exists()? {
            info!("File doesn't exist: {}", path.path().display());
            continue;
        }
        if path.path().file_name() == Some(OsStr::new("ModFixer.pak")) {
            continue;
        }

        info!("Open file {}", path.path().display());
        let mut file = fs::File::open(path.path())?;
        let header = read_header(&mut file)?;
        info!("Read file list");
        let file_list = read_file_list(&mut file, &header)?;

        for entry in file_list.iter().flatten() {
            if entry.name.ends_with(b"/meta.lsx") {
                info!(
                    "Read meta from: {}",
                    std::str::from_utf8(entry.name).unwrap()
                );
                let content = read_file(&mut file, &entry)?;
                if let Some(mod_info) = read_mod_info(&content.data)? {
                    mod_infos.push(mod_info);
                }
            }
        }
        info!("Close {}", path.path().display());
    }

    let output = fs::File::create(args.dest)?;
    write_mod_settings(output, &mod_infos)?;

    Ok(())
}

fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let args = Args::parse();

    if let Err(e) = execute_args(args) {
        error!("error: {}", e);
    }
}
