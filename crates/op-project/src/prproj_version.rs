//! Copy-only compatibility patch, inspired by downgrade-premiere-prproj (MIT).
//! This changes one serialization-version attribute in the original XML. It is not an
//! exporter for OpenPremier projects and does not translate newer effects or schemas.

use std::io::Write;
use std::path::Path;

use quick_xml::events::Event;

use crate::{ProjectError, prproj, xml};

const PROJECT_CLASS: &str = "62ad66dd-0dcd-42da-a660-6d8fbde94876";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionInfo {
    pub container_version: String,
    pub project_version: u32,
}

fn checked(text: &str) -> Result<VersionInfo, ProjectError> {
    let doc = xml::parse(text, xml::Limits::default())
        .map_err(|e| ProjectError::Format(e.to_string()))?;
    if doc.name(0) != "PremiereData" {
        return Err(ProjectError::Format(
            "the XML root is not PremiereData".into(),
        ));
    }
    let projects: Vec<_> = doc
        .children(0)
        .filter(|n| {
            doc.name(*n) == "Project"
                && doc.attr(*n, "ClassID") == Some(PROJECT_CLASS)
                && doc.attr(*n, "ObjectID").is_some()
        })
        .collect();
    if projects.len() != 1 {
        return Err(ProjectError::Format(
            "expected exactly one root Project object of the known class".into(),
        ));
    }
    let project_version = doc
        .attr(projects[0], "Version")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| ProjectError::Format("invalid Project serialization version".into()))?;
    Ok(VersionInfo {
        container_version: doc.attr(0, "Version").unwrap_or("").into(),
        project_version,
    })
}

pub fn inspect(bytes: &[u8]) -> Result<VersionInfo, ProjectError> {
    checked(&prproj::project_xml(bytes)?)
}

/// Preserves all XML bytes except the root Project's version value. Output is gzip.
pub fn downgrade(bytes: &[u8], target: u32) -> Result<Vec<u8>, ProjectError> {
    let mut text = prproj::project_xml(bytes)?;
    let info = checked(&text)?;
    if target == 0 || target > info.project_version {
        return Err(ProjectError::Format(
            "target must be between 1 and the current Project version".into(),
        ));
    }
    let mut reader = quick_xml::Reader::from_str(&text);
    let mut depth = 0usize;
    let mut patch = None;
    loop {
        let offset = reader.buffer_position() as usize;
        let ev = reader
            .read_event()
            .map_err(|e| ProjectError::Format(e.to_string()))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                if depth == 1 && e.name().as_ref() == "Project" {
                    let attrs = e
                        .attributes()
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| ProjectError::Format(e.to_string()))?;
                    if attrs
                        .iter()
                        .any(|a| a.key.as_ref() == "ClassID" && a.value.as_ref() == PROJECT_CLASS)
                        && attrs.iter().any(|a| a.key.as_ref() == "ObjectID")
                    {
                        let attr = attrs
                            .iter()
                            .find(|a| a.key.as_ref() == "Version")
                            .ok_or_else(|| {
                                ProjectError::Format("missing Project version".into())
                            })?;
                        // Slice reader events borrow the original UTF-8 string: use the value's
                        // byte offset, so quotes, spacing, CDATA and all unknown data stay intact.
                        let start = attr.value.as_ptr() as usize - text.as_ptr() as usize;
                        if start < offset
                            || start + attr.value.len() > reader.buffer_position() as usize
                        {
                            return Err(ProjectError::Format(
                                "could not locate the original version attribute".into(),
                            ));
                        }
                        patch = Some(start..start + attr.value.len());
                        break;
                    }
                }
                if matches!(ev, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    let range =
        patch.ok_or_else(|| ProjectError::Format("missing Project version attribute".into()))?;
    text.replace_range(range, &target.to_string());
    checked(&text)?;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(text.as_bytes())?;
    Ok(gzip.finish()?)
}

/// Saves a new copy, refuses existing files (including the source), and publishes atomically.
pub fn save_copy(bytes: &[u8], target: u32, output: &Path) -> Result<(), ProjectError> {
    let data = downgrade(bytes, target)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&data)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(output)
        .map_err(|e| ProjectError::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        format!(
            r#"<?xml version="1.0"?><PremiereData Version="3">
<!-- Version="99" -->
<Project Extra="keep" Version = '42' ObjectID="1" ClassID="{PROJECT_CLASS}"><Unknown><![CDATA[Version="77"]]></Unknown><Project Version="12"/></Project>
<Effect Version="9"/></PremiereData>"#
        )
    }

    #[test]
    fn preserves_everything_except_the_root_project_version() {
        let text = format!("\u{feff}{}", fixture());
        for input in [text.as_bytes().to_vec(), {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(text.as_bytes()).unwrap();
            gz.finish().unwrap()
        }] {
            let out = downgrade(&input, 1).unwrap();
            assert_eq!(
                prproj::decompress(&out).unwrap(),
                text.replace("Version = '42'", "Version = '1'")
            );
            assert_eq!(inspect(&out).unwrap().project_version, 1);
        }
    }

    #[test]
    fn rejects_bad_input_and_upgrades() {
        let text = fixture();
        for input in [
            "garbage".to_string(),
            text.replace(PROJECT_CLASS, "unknown"),
            text.replace("'42'", "'bad'"),
            text.replace(
                "<Effect Version=\"9\"/>",
                &format!("<Project ObjectID='2' ClassID='{PROJECT_CLASS}' Version='41'/>"),
            ),
        ] {
            assert!(downgrade(input.as_bytes(), 1).is_err());
        }
        assert!(downgrade(text.as_bytes(), 0).is_err());
        assert!(downgrade(text.as_bytes(), 43).is_err());
        let mut out = downgrade(text.as_bytes(), 1).unwrap();
        out[15] ^= 0xff;
        assert!(downgrade(&out, 1).is_err());
    }

    #[test]
    fn never_overwrites_an_existing_copy_or_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.prproj");
        let text = fixture();
        std::fs::write(&path, &text).unwrap();
        assert!(save_copy(text.as_bytes(), 1, &path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        let copy = dir.path().join("copy.prproj");
        save_copy(text.as_bytes(), 1, &copy).unwrap();
        assert_eq!(
            inspect(&std::fs::read(copy).unwrap())
                .unwrap()
                .project_version,
            1
        );
    }
}
