use std::{
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use env_logger::Env;
use error::Bg3ModError;
use globset::Glob;
use log::{debug, error, info};
use mod_meta::{read_mod_info, read_mod_settings, write_mod_settings, ModInfo};
use pak_reader::{read_file, read_file_list, read_header};
use serde_json::json;
use steamlocate::SteamDir;
use lazy_static::lazy_static;

mod error;

#[derive(Debug)]
struct Configuration {
    mods_path: PathBuf,
    modsettings_path: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    InfoJson {
        path: PathBuf,
    },
    Available,
    Enabled,
    Enable {
        #[arg(short, long)]
        pattern: String,
    },
    Disable {
        #[arg(short, long)]
        pattern: String,
    },
    Clean,
    Order {
        #[arg(short, long)]
        pattern: String,
        #[arg(short, long)]
        order: u32,
    },
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    bg3_path: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

lazy_static! {
    static ref COMPATDATA_APPDATA_PATH: PathBuf = PathBuf::from("compatdata/1086940/pfx/drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3/");
    static ref MODS_PATH: PathBuf = PathBuf::from("Mods");
    static ref MODSETTINGS_PATH: PathBuf = PathBuf::from("PlayerProfiles/Public/modsettings.lsx");
}

fn create_config(args: &Args) -> Result<Configuration, Bg3ModError> {
    if let Some(bg3_path) = &args.bg3_path {
        let mods_path = [bg3_path, &MODS_PATH].iter().collect::<PathBuf>();
        let modsettings_path = [bg3_path, &MODSETTINGS_PATH]
            .iter()
            .collect::<PathBuf>();
        Ok(Configuration {
            mods_path,
            modsettings_path,
        })
    } else if cfg!(unix) {
        let mut steamdir = SteamDir::locate().unwrap();
        let bg3_path = steamdir.libraryfolders().paths.iter().find_map(|path| {
            let bg3_path = [path, &COMPATDATA_APPDATA_PATH].iter().collect::<PathBuf>();
            if bg3_path.is_dir() {
                Some(bg3_path)
            } else {
                None
            }
        });
        if let Some(bg3_path) = bg3_path.as_deref() {
            let mods_path = [bg3_path, &MODS_PATH].iter().collect::<PathBuf>();
            let modsettings_path = [bg3_path, &MODSETTINGS_PATH]
                .iter()
                .collect::<PathBuf>();
            Ok(Configuration {
                mods_path,
                modsettings_path,
            })
        } else {
            Err(Bg3ModError::AppDataNotFound)
        }
    } else {
        Err(Bg3ModError::AppDataDetectionNotSupported)
    }
}

fn read_available_mods(mods_path: &Path) -> Result<Vec<ModInfo>, Box<dyn std::error::Error>> {
    if !mods_path.is_dir() {
        Err(Bg3ModError::PathNotDirectory)?;
    }

    let mut mod_infos = Vec::new();

    let paths = fs::read_dir(mods_path)?;
    for path in paths.flatten() {
        match path.path().extension().and_then(OsStr::to_str) {
            Some("pak") => {}
            _ => continue,
        }
        if !path.path().try_exists()? {
            error!("File doesn't exist: {}", path.path().display());
            continue;
        }
        if path.path().file_name() == Some(OsStr::new("ModFixer.pak")) {
            continue;
        }

        debug!(
            "Open {}",
            path.path().file_name().unwrap().to_str().unwrap()
        );
        let mut file = fs::File::open(path.path())?;
        let header = read_header(&mut file)?;
        debug!("Read file list");
        let file_list = read_file_list(&mut file, &header)?;

        for entry in file_list.iter().flatten() {
            if entry.name.ends_with(b"/meta.lsx") {
                debug!(
                    "Read meta from: {}",
                    std::str::from_utf8(entry.name).unwrap_or("non-utf8")
                );
                let content = read_file(&mut file, &entry)?;
                if let Some(mod_info) = read_mod_info(&content.data)? {
                    mod_infos.push(mod_info);
                }
            }
        }
        debug!("Close");
    }

    Ok(mod_infos)
}

fn execute_command(
    conf: &Configuration,
    cmd: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::InfoJson { path } => {
            let mut file = fs::File::open(path)?;
            let header = read_header(&mut file)?;
            let file_list = read_file_list(&mut file, &header)?;
            let entry = file_list
                .iter()
                .flatten()
                .find(|e| e.name.ends_with(b"/meta.lsx"));
            if let Some(entry) = entry {
                let content = read_file(&mut file, &entry)?;
                info!("{}", std::str::from_utf8(&content.data).unwrap());
                if let Some(mod_info) = read_mod_info(&content.data)? {
                    let json = json!({ "mods": [serde_json::to_value(mod_info)?] });
                    writeln!(
                        std::io::stdout(),
                        "{}",
                        serde_json::to_string_pretty(&json)?
                    )?;
                }
            } else {
                error!("Failed to read mod meta");
            }
            Ok(())
        }
        Commands::Available => {
            let available = read_available_mods(&conf.mods_path)?;
            info!(
                "mods:\n{}",
                available
                    .iter()
                    .map(|m| format!(
                        "'{}' by {}\n",
                        m.name,
                        m.author.as_deref().unwrap_or("unknown")
                    ))
                    .collect::<String>()
            );
            Ok(())
        }
        Commands::Enabled => {
            let enabled = read_mod_settings(fs::File::open(&conf.modsettings_path)?)?;
            info!(
                "mods:\n{}",
                enabled
                    .iter()
                    .enumerate()
                    .map(|(i, m)| format!("{}: '{}'\n", i, m.name))
                    .collect::<String>()
            );
            Ok(())
        }
        Commands::Enable { pattern } => {
            let available = read_available_mods(&conf.mods_path)?;
            let enabled = read_mod_settings(fs::File::open(&conf.modsettings_path)?)?;
            let pattern = Glob::new(&pattern)?.compile_matcher();
            let to_be_enabled = available
                .iter()
                .filter(|m| pattern.is_match(&m.name))
                .filter(|m| !enabled.iter().any(|e| e.uuid == m.uuid))
                .collect::<Vec<_>>();
            if !to_be_enabled.is_empty() {
                for m in to_be_enabled.clone() {
                    info!("enable {}", m.name);
                }
                let enabled = enabled.iter().chain(to_be_enabled).collect::<Vec<_>>();
                info!(
                    "mods:\n{}",
                    enabled
                        .iter()
                        .enumerate()
                        .map(|(i, m)| format!("{}: '{}'\n", i, m.name))
                        .collect::<String>()
                );
                write_mod_settings(fs::File::create(&conf.modsettings_path)?, &enabled)?;
            } else {
                error!("no matches for pattern or all enabled");
            }
            Ok(())
        }
        Commands::Disable { pattern } => {
            let enabled = read_mod_settings(fs::File::open(&conf.modsettings_path)?)?;
            let pattern = Glob::new(&pattern)?.compile_matcher();
            let to_be_disabled = enabled
                .iter()
                .filter(|m| !m.is_internal() && pattern.is_match(&m.name))
                .collect::<Vec<_>>();
            if !to_be_disabled.is_empty() {
                for m in to_be_disabled.as_slice() {
                    info!("disable {}", m.name);
                }
                let enabled = enabled
                    .iter()
                    .filter(|m| m.is_internal() || !pattern.is_match(&m.name))
                    .collect::<Vec<_>>();
                info!(
                    "mods:\n{}",
                    enabled
                        .iter()
                        .enumerate()
                        .map(|(i, m)| format!("{}: '{}'\n", i, m.name))
                        .collect::<String>()
                );
                write_mod_settings(fs::File::create(&conf.modsettings_path)?, &enabled)?;
            } else {
                error!("no matches for pattern in enabled");
            }
            Ok(())
        }
        Commands::Clean => {
            let available = read_available_mods(&conf.mods_path)?;
            let enabled = read_mod_settings(fs::File::open(&conf.modsettings_path)?)?;
            let to_be_removed = enabled
                .iter()
                .filter(|m| !m.is_internal() && !available.iter().any(|e| e.uuid == m.uuid))
                .collect::<Vec<_>>();
            if !to_be_removed.is_empty() {
                for m in to_be_removed.as_slice() {
                    info!("clean {}", m.name);
                }
                let enabled = enabled
                    .iter()
                    .filter(|m| m.is_internal() || available.iter().any(|e| e.uuid == m.uuid))
                    .collect::<Vec<_>>();
                info!(
                    "mods:\n{}",
                    enabled
                        .iter()
                        .enumerate()
                        .map(|(i, m)| format!("{}: '{}'\n", i, m.name))
                        .collect::<String>()
                );
                write_mod_settings(fs::File::create(&conf.modsettings_path)?, &enabled)?;
            } else {
                error!("nothing to clean");
            }
            Ok(())
        }
        Commands::Order { pattern, order } => {
            let enabled = read_mod_settings(fs::File::open(&conf.modsettings_path)?)?;
            let pattern = Glob::new(&pattern)?.compile_matcher();
            let to_be_ordered = enabled
                .iter()
                .filter(|m| !m.is_internal() && pattern.is_match(&m.name))
                .collect::<Vec<_>>();
            if !to_be_ordered.is_empty() {
                let mut enabled = enabled
                    .iter()
                    .filter(|m| m.is_internal() || !pattern.is_match(&m.name))
                    .collect::<Vec<_>>();
                for m in to_be_ordered.as_slice() {
                    info!("order {}", m.name);
                }
                let order = (order as usize).max(1usize).min(enabled.len());
                for m in to_be_ordered.iter().rev() {
                    enabled.insert(order, m);
                }
                info!(
                    "mods:\n{}",
                    enabled
                        .iter()
                        .enumerate()
                        .map(|(i, m)| format!("{}: '{}'\n", i, m.name))
                        .collect::<String>()
                );
                write_mod_settings(fs::File::create(&conf.modsettings_path)?, &enabled)?;
            } else {
                error!("no matches for pattern in enabled");
            }
            Ok(())
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let args = Args::parse();
    let conf = create_config(&args)?;

    if let Err(e) = execute_command(&conf, args.command) {
        error!("error: {}", e);
        Err(e)
    } else {
        Ok(())
    }
}
