use std::{fs::File, path::PathBuf};

use clap::Parser;
use env_logger::Env;
use log::info;
use pak_reader::{read_file, read_file_list, read_header};
use quick_xml::events::Event;
use quick_xml::reader::Reader;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    path: PathBuf,
}

fn read_meta(content: &[u8]) {
    let mut buf = Vec::new();
    let mut reader = Reader::from_reader(content);
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"attribute" => info!("attr: {:?}", e),
                _ => {}
            },
            Ok(Event::Start(_e)) => {}
            Ok(_) => {}
            Err(e) => panic!("error: {}", e),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));

    let args = Args::parse();

    let mut file = File::open(args.path)?;

    let header = read_header(&mut file)?;
    info!("header hash: {:?}", header.hash);

    let file_list = read_file_list(&mut file, &header)?;

    let mut count = 0;
    for entry in file_list.iter() {
        if let Ok(entry) = entry {
            count += 1;
            let name = std::str::from_utf8(entry.name)?;
            if name.contains("/meta.lsx") {
                info!("f: {} {:?}", name, entry.flags);
                let content = read_file(&mut file, &entry)?;
                read_meta(&content.data);
            }
        }
    }

    info!("{}", count);

    Ok(())
}
