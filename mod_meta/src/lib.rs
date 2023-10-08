use quick_xml::{
    events::{BytesDecl, BytesEnd, BytesStart, Event},
    Reader, Writer,
};
use serde::Serialize;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::BTreeMap,
    fmt::Display,
    ops::{Deref, DerefMut},
};

struct StackPath(Vec<Vec<u8>>);

impl Deref for StackPath {
    type Target = Vec<Vec<u8>>;

    fn deref(&self) -> &Self::Target {
        let StackPath(inner) = self;
        inner
    }
}

impl DerefMut for StackPath {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let StackPath(inner) = self;
        inner
    }
}

impl Display for StackPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, part) in self.iter().enumerate() {
            if let Ok(part) = std::str::from_utf8(part) {
                write!(f, "{}", part)?;
            }
            if i < self.len() - 1 {
                write!(f, "/")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct ModInfo {
    pub uuid: String,
    pub name: String,
    pub folder: Option<String>,
    pub md5: Option<String>,
    pub version: Option<String>,
}

impl ModInfo {
    pub fn is_internal(&self) -> bool {
        self.name == "Gustav" || self.name == "GustavDev"
    }
}

pub fn read_mod_attribute(
    map: &mut BTreeMap<String, String>,
    e: &BytesStart,
) -> Result<(), quick_xml::Error> {
    let id = e.try_get_attribute(b"id")?;
    let value = e.try_get_attribute(b"value")?;
    if let (Some(id), Some(value)) = (id, value) {
        let id = id.unescape_value()?;
        let value = value.unescape_value()?;
        map.insert(id.to_string(), value.to_string());
    }
    Ok(())
}

fn read_mod_attr_value<'a>(
    e: &'a BytesStart<'a>,
    name: &[u8],
) -> Result<Option<Cow<'a, str>>, quick_xml::Error> {
    Ok(if let Some(value) = e.try_get_attribute(name)? {
        Some(value.unescape_value()?)
    } else {
        None
    })
}

pub fn write_mod_settings(
    writer: impl std::io::Write,
    mod_infos: &[&ModInfo],
) -> Result<(), quick_xml::Error> {
    let mut writer = Writer::new_with_indent(writer, b' ', 4);

    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    writer.write_event(Event::Start(BytesStart::new("save")))?;
    writer
        .create_element("version")
        .with_attributes(vec![
            ("major", "4"),
            ("minor", "0"),
            ("revision", "10"),
            ("build", "400"),
        ])
        .write_empty()?;
    writer.write_event(Event::Start(BytesStart::from_content(
        r#"region id="ModuleSettings""#,
        6,
    )))?;
    writer.write_event(Event::Start(BytesStart::from_content(
        r#"node id="root""#,
        5,
    )))?;
    writer.write_event(Event::Start(BytesStart::new("children")))?;

    writer.write_event(Event::Start(BytesStart::from_content(
        r#"node id="ModOrder""#,
        5,
    )))?;
    writer.write_event(Event::Start(BytesStart::new("children")))?;
    for mod_info in mod_infos {
        writer
            .create_element("node")
            .with_attribute(("id", "Module"))
            .write_inner_content(|w| {
                w.create_element("attribute")
                    .with_attribute(("id", "UUID"))
                    .with_attribute(("type", "FixedString"))
                    .with_attribute(("value", mod_info.uuid.as_str()))
                    .write_empty()?;
                Ok(())
            })?;
    }
    writer.write_event(Event::End(BytesEnd::new("children")))?;
    writer.write_event(Event::End(BytesEnd::new("node")))?;

    writer.write_event(Event::Start(BytesStart::from_content(
        r#"node id="Mods""#,
        5,
    )))?;
    writer.write_event(Event::Start(BytesStart::new("children")))?;
    for mod_info in mod_infos {
        writer
            .create_element("node")
            .with_attribute(("id", "ModuleShortDesc"))
            .write_inner_content(|w| {
                w.create_element("attribute")
                    .with_attribute(("id", "Name"))
                    .with_attribute(("type", "LSString"))
                    .with_attribute(("value", mod_info.name.as_str()))
                    .write_empty()?;
                w.create_element("attribute")
                    .with_attribute(("id", "Folder"))
                    .with_attribute(("type", "LSString"))
                    .with_attribute(("value", mod_info.folder.as_deref().unwrap_or("")))
                    .write_empty()?;
                w.create_element("attribute")
                    .with_attribute(("id", "MD5"))
                    .with_attribute(("type", "LSString"))
                    .with_attribute(("value", mod_info.md5.as_deref().unwrap_or("")))
                    .write_empty()?;
                w.create_element("attribute")
                    .with_attribute(("id", "UUID"))
                    .with_attribute(("type", "FixedString"))
                    .with_attribute(("value", mod_info.uuid.as_str()))
                    .write_empty()?;
                w.create_element("attribute")
                    .with_attribute(("id", "Version64"))
                    .with_attribute(("type", "int64"))
                    .with_attribute(("value", mod_info.version.as_deref().unwrap_or("1")))
                    .write_empty()?;
                Ok(())
            })?;
    }
    writer.write_event(Event::End(BytesEnd::new("children")))?;
    writer.write_event(Event::End(BytesEnd::new("node")))?;

    writer.write_event(Event::End(BytesEnd::new("children")))?;
    writer.write_event(Event::End(BytesEnd::new("node")))?;
    writer.write_event(Event::End(BytesEnd::new("region")))?;
    writer.write_event(Event::End(BytesEnd::new("save")))?;
    Ok(())
}

pub fn read_mod_settings(mut reader: impl std::io::Read) -> Result<Vec<ModInfo>, quick_xml::Error> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    let mut reader = Reader::from_reader(buf.as_slice());
    let mut stack = StackPath(Vec::new());

    let mut order = BTreeMap::new();
    let mut mods = Vec::new();

    let mut folder = None;
    let mut md5 = None;
    let mut name = None;
    let mut uuid = None;
    let mut version = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.name().as_ref() == b"node" => {
                let id = e
                    .try_get_attribute(b"id")?
                    .expect("Failed to get id of node")
                    .value
                    .into_owned();
                stack.push(id);
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"node" => {
                if let Some(b"ModuleShortDesc") = stack.pop().as_deref() {
                    if let (Some(uuid), Some(name)) = (uuid, name) {
                        mods.push(ModInfo {
                            name,
                            folder,
                            md5,
                            uuid,
                            version,
                        });
                    }
                    name = None;
                    folder = None;
                    md5 = None;
                    uuid = None;
                    version = None;
                }
            }
            Ok(Event::Empty(e)) => match (stack.last().map(|r| r.as_slice()), e.name().as_ref()) {
                (Some(b"Module"), b"attribute") => {
                    let value = read_mod_attr_value(&e, b"value")?;
                    if let Some(value) = value {
                        let idx = order.len();
                        order.insert(value.to_string(), idx);
                    }
                }
                (Some(b"ModuleShortDesc"), b"attribute") => {
                    let id = read_mod_attr_value(&e, b"id")?.unwrap_or(Cow::from(""));
                    let value = read_mod_attr_value(&e, b"value")?;
                    match id.as_ref() {
                        "Name" => {
                            name = value.map(|v| v.to_string());
                        }
                        "Folder" => {
                            folder = value.map(|v| v.to_string());
                        }
                        "MD5" => {
                            md5 = value.map(|v| v.to_string());
                        }
                        "UUID" => {
                            uuid = value.map(|v| v.to_string());
                        }
                        "Version64" => {
                            version = value.map(|v| v.to_string());
                        }
                        _ => {}
                    }
                }
                _ => (),
            },
            Ok(_) => {}
            Err(e) => panic!("error: {}", e),
        }
    }

    mods.sort_by(|a, b| match (order.get(&a.uuid), order.get(&b.uuid)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a_idx), Some(b_idx)) => a_idx.cmp(b_idx),
    });

    Ok(mods)
}

pub fn read_mod_info(content: &[u8]) -> Result<Option<ModInfo>, quick_xml::Error> {
    let mut reader = Reader::from_reader(content);
    let mut stack = StackPath(Vec::new());

    let mut folder = None;
    let mut md5 = None;
    let mut name = None;
    let mut uuid = None;
    let mut version = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"node" {
                    if let Some(attr) = e.try_get_attribute(b"id")? {
                        stack.push(attr.value.into_owned());
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"node" {
                    stack.pop();
                }
            }
            Ok(Event::Empty(e)) => {
                if let (Some(b"ModuleInfo"), b"attribute") =
                    (stack.last().map(|r| r.as_slice()), e.name().as_ref())
                {
                    let id = read_mod_attr_value(&e, b"id")?.unwrap_or(Cow::from(""));
                    let value = read_mod_attr_value(&e, b"value")?;
                    match id.as_ref() {
                        "Name" => {
                            name = value.map(|v| v.to_string());
                        }
                        "Folder" => {
                            folder = value.map(|v| v.to_string());
                        }
                        "MD5" => {
                            md5 = value.map(|v| v.to_string());
                        }
                        "UUID" => {
                            uuid = value.map(|v| v.to_string());
                        }
                        "Version64" => {
                            version = value.map(|v| v.to_string());
                        }
                        _ => {}
                    }
                }
            }
            Ok(_) => {}
            Err(e) => panic!("error: {}", e),
        }
    }
    if let (Some(uuid), Some(name)) = (uuid, name) {
        let info = ModInfo {
            name,
            folder,
            md5,
            uuid,
            version,
        };
        Ok(Some(info))
    } else {
        Ok(None)
    }
}
