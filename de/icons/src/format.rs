//! Reading a `.fernicon` directory (docs/formats/icon.md) and recolouring
//! its tagged fills.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Error;
use crate::color;

/// The roles a foreground fill can have.
pub const ROLES: [&str; 3] = ["primary", "secondary", "highlight"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    format: u32,
    key: String,
    #[serde(default)]
    roles: BTreeMap<String, String>,
    #[serde(default)]
    tint: Tint,
}

/// Which layers the accent modes may recolour.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tint {
    #[serde(default = "yes")]
    pub background: bool,
    #[serde(default = "yes")]
    pub foreground: bool,
}

fn yes() -> bool {
    true
}

impl Default for Tint {
    fn default() -> Self {
        Self {
            background: true,
            foreground: true,
        }
    }
}

/// A loaded icon.
#[derive(Debug)]
pub struct Icon {
    pub dir: PathBuf,
    /// The key colour, `0xRRGGBB`.
    pub key: u32,
    /// Role name to `0xRRGGBB`.
    pub roles: BTreeMap<String, u32>,
    pub tint: Tint,
    pub background: String,
    pub foreground: String,
    pub dark_background: Option<String>,
    pub dark_foreground: Option<String>,
}

fn bad(dir: &Path, what: impl std::fmt::Display) -> Error {
    Error::BadIcon(format!("{}: {what}", dir.display()))
}

fn read_svg(dir: &Path, name: &str) -> Result<Option<String>, Error> {
    let path = dir.join(name);
    match fs::read_to_string(&path) {
        Ok(text) => {
            check_svg(&text).map_err(|e| bad(dir, format!("{name}: {e}")))?;
            Ok(Some(text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(bad(dir, format!("{name}: {e}"))),
    }
}

/// What an icon layer may not contain: text (no fonts), pictures,
/// scripts and references outside the file.
fn check_svg(text: &str) -> Result<(), String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        let name = node.tag_name().name();
        if matches!(
            name,
            "text" | "image" | "script" | "foreignObject" | "use" | "style"
        ) {
            // <use> could pull in other files; <style> would carry colours
            // the role rewrite cannot see.
            return Err(format!("<{name}> is not allowed in an icon layer"));
        }
        for a in node.attributes() {
            if a.name() == "href" && !a.value().starts_with('#') {
                return Err(format!("reference to {} outside the file", a.value()));
            }
        }
    }
    Ok(())
}

impl Icon {
    /// Loads and checks an icon directory.
    ///
    /// # Errors
    /// [`Error::BadIcon`] naming what is missing or wrong.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let text = fs::read_to_string(dir.join("icon.toml"))
            .map_err(|e| bad(dir, format!("icon.toml: {e}")))?;
        let meta: Meta = toml::from_str(&text).map_err(|e| bad(dir, format!("icon.toml: {e}")))?;
        if meta.format != 1 {
            return Err(bad(
                dir,
                format!("format {} (this engine reads format 1)", meta.format),
            ));
        }
        let key = color::parse(&meta.key)
            .ok_or_else(|| bad(dir, format!("key {:?} is not #rrggbb", meta.key)))?;
        let mut roles = BTreeMap::new();
        for (name, value) in meta.roles {
            if !ROLES.contains(&name.as_str()) {
                return Err(bad(dir, format!("unknown role {name:?}")));
            }
            let c = color::parse(&value)
                .ok_or_else(|| bad(dir, format!("role {name}: {value:?} is not #rrggbb")))?;
            roles.insert(name, c);
        }
        let background =
            read_svg(dir, "background.svg")?.ok_or_else(|| bad(dir, "no background.svg"))?;
        let foreground =
            read_svg(dir, "foreground.svg")?.ok_or_else(|| bad(dir, "no foreground.svg"))?;
        let icon = Self {
            dir: dir.to_path_buf(),
            key,
            roles,
            tint: meta.tint,
            background,
            foreground,
            dark_background: read_svg(dir, "dark-background.svg")?,
            dark_foreground: read_svg(dir, "dark-foreground.svg")?,
        };
        // Every role used has a colour.
        recolor(&icon.foreground, &icon.roles).map_err(|e| bad(dir, e))?;
        if let Some(fg) = &icon.dark_foreground {
            recolor(fg, &icon.roles).map_err(|e| bad(dir, e))?;
        }
        Ok(icon)
    }
}

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

/// The SVG with each tagged element's `fill`, `stroke` and `stop-color`
/// set to its role's colour.
pub fn recolor(svg: &str, roles: &BTreeMap<String, u32>) -> Result<String, String> {
    let doc = roxmltree::Document::parse(svg).map_err(|e| e.to_string())?;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        let Some(role) = node.attribute("data-role") else {
            continue;
        };
        let c = *roles
            .get(role)
            .ok_or_else(|| format!("role {role:?} has no colour in icon.toml"))?;
        for a in node.attributes() {
            if matches!(a.name(), "fill" | "stroke" | "stop-color") && a.value() != "none" {
                edits.push((a.range_value(), hex(c)));
            }
        }
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = svg.to_string();
    for (range, value) in edits {
        out.replace_range(range, &value);
    }
    Ok(out)
}

#[cfg(test)]
// Colours read as #rrggbb; pixel tests name their channels.
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;

    fn roles() -> BTreeMap<String, u32> {
        [
            ("primary".to_string(), 0xFFFFFF),
            ("highlight".to_string(), 0xFFC94D),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn recolours_tagged_fills_only() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
<rect data-role="primary" fill="#000000" stroke="none" width="10" height="10"/>
<circle data-role="highlight" fill="#111111" stroke="#222222" r="5"/>
<rect fill="#333333" width="4" height="4"/>
<linearGradient id="g"><stop data-role="primary" offset="0" stop-color="#444444"/></linearGradient>
</svg>"##;
        let out = recolor(svg, &roles()).unwrap();
        assert!(
            out.contains(r##"data-role="primary" fill="#ffffff" stroke="none""##),
            "{out}"
        );
        assert!(
            out.contains(r##"fill="#ffc94d" stroke="#ffc94d""##),
            "{out}"
        );
        assert!(out.contains(r##"fill="#333333""##));
        assert!(out.contains(r##"stop-color="#ffffff""##));
        // Still valid XML.
        roxmltree::Document::parse(&out).unwrap();
    }

    #[test]
    fn a_role_without_a_colour_is_an_error() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect data-role="secondary" fill="#000"/></svg>"##;
        assert!(recolor(svg, &roles()).unwrap_err().contains("secondary"));
    }

    #[test]
    fn layers_may_not_carry_text_pictures_or_outside_references() {
        for bad in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text>Hi</text></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="x.png"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><linearGradient xlink:href="other.svg#g"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style>rect{fill:red}</style></svg>"#,
        ] {
            assert!(check_svg(bad).is_err(), "{bad}");
        }
        let ok = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><linearGradient id="a"/><linearGradient xlink:href="#a"/></svg>"##;
        check_svg(ok).unwrap();
    }

    #[test]
    fn loads_an_icon_and_reports_problems() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("org.example.App.fernicon");
        fs::create_dir(&icon).unwrap();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><rect data-role="primary" fill="#fff" width="1024" height="1024"/></svg>"##;
        fs::write(icon.join("background.svg"), svg).unwrap();
        fs::write(icon.join("foreground.svg"), svg).unwrap();
        fs::write(icon.join("icon.toml"), "format = 1\nkey = \"#2D64C8\"\n").unwrap();
        // The foreground uses "primary", which has no colour.
        let e = Icon::load(&icon).unwrap_err().to_string();
        assert!(e.contains("primary"), "{e}");
        fs::write(
            icon.join("icon.toml"),
            "format = 1\nkey = \"#2D64C8\"\n[roles]\nprimary = \"#FFFFFF\"\n",
        )
        .unwrap();
        let loaded = Icon::load(&icon).unwrap();
        assert_eq!(loaded.key, 0x2D64C8);
        assert!(loaded.tint.background && loaded.tint.foreground);
        assert!(loaded.dark_background.is_none());
        fs::write(icon.join("icon.toml"), "format = 2\nkey = \"#2D64C8\"\n").unwrap();
        assert!(
            Icon::load(&icon)
                .unwrap_err()
                .to_string()
                .contains("format 2")
        );
        fs::write(icon.join("icon.toml"), "format = 1\nkey = \"blue\"\n").unwrap();
        assert!(Icon::load(&icon).is_err());
    }
}
