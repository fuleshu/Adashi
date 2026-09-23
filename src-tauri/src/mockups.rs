//! Backend-neutral models and shared domain helpers.
pub use adashi_storage_api::mockups::*;
use base64::Engine as _;
use std::collections::HashSet;

pub fn render_png(svg: &str, width: i64, height: i64) -> Result<Vec<u8>, String> {
    validate_viewport(width, height)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|err| format!("SVG parse failed: {err}"))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width as u32, height as u32)
        .ok_or("Could not allocate PNG surface")?;
    let size = tree.size();
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|err| err.to_string())
}

pub fn validate_svg(svg: &str, width: i64, height: i64) -> Result<String, String> {
    validate_viewport(width, height)?;
    let document =
        roxmltree::Document::parse(svg).map_err(|err| format!("Invalid SVG XML: {err}"))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err("Mockup source must have an <svg> root".into());
    }
    let allowed = [
        "svg",
        "g",
        "rect",
        "circle",
        "ellipse",
        "line",
        "polyline",
        "polygon",
        "path",
        "text",
        "tspan",
        "image",
        "defs",
        "clipPath",
        "mask",
        "linearGradient",
        "radialGradient",
        "stop",
        "title",
        "desc",
    ];
    let identified = [
        "g", "rect", "circle", "ellipse", "line", "polyline", "polygon", "path", "text", "image",
    ];
    let mut ids = HashSet::new();
    let mut count = 0;
    for node in document.descendants().filter(|node| node.is_element()) {
        let name = node.tag_name().name();
        if !allowed.contains(&name) {
            return Err(format!("Unsafe or unsupported SVG element: <{name}>"));
        }
        for attribute in node.attributes() {
            let attr = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim().to_ascii_lowercase();
            if attr.starts_with("on")
                || attr == "style" && (value.contains("url(") || value.contains("expression("))
            {
                return Err(format!("Unsafe SVG attribute: {}", attribute.name()));
            }
            if attr == "href" || attr.ends_with(":href") {
                if !(value.starts_with('#')
                    || value.starts_with("data:image/png")
                    || value.starts_with("data:image/jpeg")
                    || value.starts_with("data:image/webp")
                    || value.starts_with("data:image/gif"))
                {
                    return Err("SVG external resources are not allowed".into());
                }
            }
            if value.contains("javascript:") {
                return Err("SVG javascript URLs are not allowed".into());
            }
        }
        if identified.contains(&name)
            && node
                .ancestors()
                .all(|ancestor| ancestor.tag_name().name() != "defs")
        {
            let id = node
                .attribute("data-adashi-id")
                .ok_or_else(|| format!("<{name}> requires a stable data-adashi-id"))?;
            if id.trim().is_empty() || !ids.insert(id.to_string()) {
                return Err(format!("Duplicate or empty data-adashi-id: {id}"));
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err("Mockup SVG must contain at least one identified visual element".into());
    }
    Ok(svg.trim().to_string())
}

pub(crate) fn validate_viewport(width: i64, height: i64) -> Result<(), String> {
    if (1..=8192).contains(&width) && (1..=8192).contains(&height) {
        Ok(())
    } else {
        Err("Mockup viewport width and height must be between 1 and 8192".into())
    }
}

pub fn preview_png_uncached(mockup: &UiMockup, variant: &str) -> Result<Vec<u8>, String> {
    let (_revision, svg, width, height) = match variant {
        "accepted" => (
            mockup.accepted_revision,
            mockup.accepted_svg.as_str(),
            mockup.manifest.viewport_width,
            mockup.manifest.viewport_height,
        ),
        "working" => (
            mockup.base_revision.unwrap_or(mockup.accepted_revision),
            mockup
                .working_svg
                .as_deref()
                .ok_or("Mockup has no working SVG")?,
            mockup.manifest.viewport_width,
            mockup.manifest.viewport_height,
        ),
        "proposed" => {
            let proposal = mockup
                .proposal
                .as_ref()
                .ok_or("Mockup has no proposed SVG")?;
            (
                proposal.base_revision,
                proposal.proposed_svg.as_str(),
                proposal.proposed_manifest.viewport_width,
                proposal.proposed_manifest.viewport_height,
            )
        }
        _ => return Err("Preview variant must be accepted, working, or proposed".into()),
    };
    render_png(svg, width, height)
}

pub fn preview_base64_uncached(mockup: &UiMockup, variant: &str) -> Result<String, String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(preview_png_uncached(mockup, variant)?))
}
#[cfg(test)]
pub(crate) use crate::storage::sqlite::mockups::*;
