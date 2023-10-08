use std::{
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use config::Config;
use env_logger::Env;
use error::Bg3ModError;
use log::{debug, error};
use mod_meta::{read_mod_info, read_mod_settings, write_mod_settings, ModInfo};
use pak_reader::{read_file, read_file_list, read_header};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;

mod error;

#[derive(Debug, Deserialize)]
struct Configuration {
    mods_path: PathBuf,
    profile_path: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Available,
    Enabled,
    Order {
        #[arg(short, long)]
        pattern: String,
        #[arg(short, long)]
        order: u32,
    },
    Enable {
        #[arg(short, long)]
        pattern: String,
    },
    Disable {
        #[arg(short, long)]
        pattern: String,
    },
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config_path: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

fn read_config(args: &Args) -> Result<Configuration, config::ConfigError> {
    Config::builder()
        .add_source(config::File::from(args.config_path.as_path()))
        .build()?
        .try_deserialize()
}

fn read_available_mods(config: &Configuration) -> Result<Vec<ModInfo>, Box<dyn std::error::Error>> {
    if !config.mods_path.is_dir() {
        Err(Bg3ModError::PathNotDirectory)?;
    }

    let mut mod_infos = Vec::new();

    let paths = fs::read_dir(&config.mods_path)?;
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

fn compile_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    let pattern = regex::escape(pattern).replace("\\*", ".+");
    RegexBuilder::new(&pattern).case_insensitive(true).build()
}

fn execute_command(
    modsettings_path: &Path,
    cmd: Commands,
    available: &[ModInfo],
    enabled: &[ModInfo],
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::Available => {
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "mods: {}", available.len())?;
            for (i, m) in available.iter().enumerate() {
                writeln!(stdout, "{} - {}", i, m.name)?;
            }
            Ok(())
        }
        Commands::Enabled => {
            let mut stdout = std::io::stdout().lock();
            for (i, m) in enabled.iter().enumerate() {
                writeln!(stdout, "{} - {}", i, m.name)?;
            }
            Ok(())
        }
        Commands::Enable { pattern } => {
            let pattern = compile_pattern(&pattern)?;
            let to_be_enabled = available
                .iter()
                .filter(|m| pattern.is_match(&m.name))
                .filter(|m| !enabled.iter().any(|e| e.uuid == m.uuid))
                .collect::<Vec<_>>();
            let mut stdout = std::io::stdout().lock();
            if !to_be_enabled.is_empty() {
                for m in to_be_enabled.clone() {
                    writeln!(stdout, "enable {}", m.name)?;
                }
                let enabled = enabled.iter().chain(to_be_enabled).collect::<Vec<_>>();
                writeln!(stdout)?;
                for (i, m) in enabled.iter().enumerate() {
                    writeln!(stdout, "{} - {}", i, m.name)?;
                }
                write_mod_settings(fs::File::create(modsettings_path)?, &enabled)?;
            } else {
                writeln!(stdout, "no matches for pattern")?;
            }
            Ok(())
        }
        Commands::Disable { pattern } => {
            let pattern = compile_pattern(&pattern)?;
            let to_be_disabled = enabled
                .iter()
                .filter(|m| {
                    m.name != "Gustav" && m.name != "GustavDev" && pattern.is_match(&m.name)
                })
                .collect::<Vec<_>>();
            let mut stdout = std::io::stdout().lock();
            if !to_be_disabled.is_empty() {
                for m in to_be_disabled.as_slice() {
                    writeln!(stdout, "disable {}", m.name)?;
                }
                let enabled = enabled
                    .iter()
                    .filter(|m| {
                        (m.name == "Gustav" || m.name == "GustavDev") || !pattern.is_match(&m.name)
                    })
                    .collect::<Vec<_>>();
                writeln!(stdout)?;
                for (i, m) in enabled.iter().enumerate() {
                    writeln!(stdout, "{} - {}", i, m.name)?;
                }
                write_mod_settings(fs::File::create(modsettings_path)?, &enabled)?;
            } else {
                writeln!(stdout, "no matches for pattern")?;
            }
            Ok(())
        }
        Commands::Order { pattern, order } => {
            let pattern = compile_pattern(&pattern)?;
            let to_be_ordered = enabled
                .iter()
                .filter(|m| {
                    m.name != "Gustav" && m.name != "GustavDev" && pattern.is_match(&m.name)
                })
                .collect::<Vec<_>>();
            let mut stdout = std::io::stdout().lock();
            if !to_be_ordered.is_empty() {
                let mut enabled = enabled
                    .iter()
                    .filter(|m| {
                        m.name == "Gustav" || m.name == "GustavDev" || !pattern.is_match(&m.name)
                    })
                    .collect::<Vec<_>>();
                for m in to_be_ordered.as_slice() {
                    writeln!(stdout, "order {}", m.name)?;
                }
                let order = order.min(1) as usize;
                for m in to_be_ordered.iter().rev() {
                    enabled.insert(order, m);
                }
                writeln!(stdout)?;
                for (i, m) in enabled.iter().enumerate() {
                    writeln!(stdout, "{} - {}", i, m.name)?;
                }
                write_mod_settings(fs::File::create(modsettings_path)?, &enabled)?;
            } else {
                writeln!(stdout, "no matches for pattern")?;
            }
            Ok(())
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let args = Args::parse();
    let conf = read_config(&args)?;

    let available = read_available_mods(&conf)?;
    let modsettings_path = [&conf.profile_path, Path::new("modsettings.lsx")]
        .iter()
        .collect::<PathBuf>();
    let enabled = read_mod_settings(fs::File::open(&modsettings_path)?)?;

    if let Err(e) = execute_command(&modsettings_path, args.command, &available, &enabled) {
        error!("error: {}", e);
        Err(e)
    } else {
        Ok(())
    }
}
