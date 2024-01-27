use std::{
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf}, collections::BTreeMap,
};

use clap::{Parser, Subcommand};
use env_logger::Env;
use error::Bg3ModError;
use globset::Glob;
use lazy_static::lazy_static;
use log::{debug, error, info};
use mod_meta::{read_mod_info, read_mod_settings, write_mod_settings, ModInfo};
use pak_reader::Package;
use serde_json::json;
use steamlocate::SteamDir;

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
    static ref COMPATDATA_APPDATA_PATH: PathBuf =
        PathBuf::from("compatdata/1086940/pfx/drive_c/users/steamuser/AppData");
    static ref BG3_DATA_PATH: PathBuf = PathBuf::from("Local/Larian Studios/Baldur's Gate 3");
    static ref MODS_PATH: PathBuf = PathBuf::from("Mods");
    static ref MODSETTINGS_PATH: PathBuf = PathBuf::from("PlayerProfiles/Public/modsettings.lsx");
}

fn create_config(args: &Args) -> Result<Configuration, Bg3ModError> {
    let bg3_path = if let Some(bg3_path) = &args.bg3_path {
        Ok(bg3_path.to_owned())
    } else if cfg!(unix) {
        let mut steamdir = SteamDir::locate().unwrap();
        steamdir
            .libraryfolders()
            .paths
            .iter()
            .find_map(|path| {
                let bg3_path = [path, &COMPATDATA_APPDATA_PATH, &BG3_DATA_PATH]
                    .iter()
                    .collect::<PathBuf>();
                if bg3_path.is_dir() {
                    Some(bg3_path)
                } else {
                    None
                }
            })
            .ok_or(Bg3ModError::AppDataNotFound)
    } else if cfg!(windows) {
        std::env::var("APP_DATA")
            .map(|path| {
                [Path::new(&path), &BG3_DATA_PATH]
                    .iter()
                    .collect::<PathBuf>()
            })
            .map_err(|_| Bg3ModError::AppDataNotFound)
    } else {
        Err(Bg3ModError::AppDataDetectionNotSupported)
    }?;

    let mods_path = [&bg3_path, &MODS_PATH].iter().collect::<PathBuf>();
    let modsettings_path = [&bg3_path, &MODSETTINGS_PATH].iter().collect::<PathBuf>();
    Ok(Configuration {
        mods_path,
        modsettings_path,
    })
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
        let mut package = Package::new(fs::File::open(path.path())?);

        for entry in package.files()?.iter().flatten() {
            if entry.name.ends_with(b"/meta.lsx") {
                debug!(
                    "Read meta from: {}",
                    std::str::from_utf8(entry.name).unwrap_or("non-utf8")
                );
                let data = package.content(&entry)?;
                if let Some(mod_info) = read_mod_info(&data)? {
                    mod_infos.push(mod_info);
                }
            }
        }
        debug!("Close");
    }

    Ok(mod_infos)
}

fn execute_command(conf: &Configuration, cmd: Commands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::InfoJson { path } => {
            let mut package = Package::new(fs::File::open(path)?);
            let file_list = package.files()?;
            let entry = file_list
                .iter()
                .flatten()
                .find(|e| e.name.ends_with(b"/meta.lsx"));
            if let Some(entry) = entry {
                let data = package.content(&entry)?;
                debug!("{}", std::str::from_utf8(&data).unwrap());
                if let Some(mod_info) = read_mod_info(&data)? {
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
            let enabled = read_mod_settings(fs::File::open(&conf.modsettings_path)?)?;
            let index_map = enabled
                .iter()
                .enumerate()
                .map(|(index, m)| (&m.uuid, index))
                .collect::<BTreeMap<_, _>>();

            info!(
                "mods:\n{}",
                available
                    .iter()
                    .map(move |m| format!(
                        "{:>3} '{}' by {}\n",
                        index_map.get(&m.uuid).map_or("-".to_string(), |index| format!("{}", index)),
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
                    .map(|(i, m)| format!("{:>3}: '{}'\n", i, m.name))
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
