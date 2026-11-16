//! A WMS service's layers, from its GetCapabilities document, for the form's layer
//! picker.
use quick_xml::events::Event;

/// A layer a WMS can draw.
#[derive(Clone, Debug, PartialEq)]
pub struct WmsLayer {
    /// What GetMap's `LAYERS` takes.
    pub name: String,
    pub title: String,
}

/// The largest capabilities document read.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// The named layers of the WMS at `base` (its address, with or without a query).
pub async fn wms_layers(base: String) -> Result<Vec<WmsLayer>, crate::Error> {
    let joiner = if base.contains('?') { "&" } else { "?" };
    let url = format!("{base}{joiner}SERVICE=WMS&REQUEST=GetCapabilities");
    let client = crate::http::client(crate::http::Policy::UserData)?;
    let bytes = crate::http::body(client.get(&url), MAX_BYTES).await?;
    crate::cpu::run(crate::cpu::Service::Features, move || parse(&bytes)).await
}

/// The text an entity or character reference stands for (`&amp;`, `&#233;`); `None`
/// for a reference XML does not define.
pub(super) fn reference_text(reference: &quick_xml::events::BytesRef) -> Option<String> {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        return Some(character.to_string());
    }
    quick_xml::escape::resolve_predefined_entity(reference).map(str::to_owned)
}

/// The `Layer` elements that have a `Name`, with their `Title`, in document order.
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<WmsLayer>, crate::Error> {
    // Text is not trimmed by the reader and is added up: a title with `&amp;` or in
    // CDATA arrives in pieces (only its last piece used to be kept).
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut buffer = Vec::new();
    // One entry per open `Layer`: its name and title so far.
    let mut open: Vec<(Option<String>, Option<String>)> = Vec::new();
    // The element whose text is being read, when it is a layer's name or title.
    let mut field: Option<&'static str> = None;
    let mut depth_in_layer = 0usize;
    let mut layers = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(element) => {
                match element.local_name().as_ref() {
                    "Layer" => {
                        open.push((None, None));
                        depth_in_layer = 0;
                    }
                    // Only a layer's own Name and Title, not its Style's.
                    "Name" if depth_in_layer == 0 && !open.is_empty() => field = Some("name"),
                    "Title" if depth_in_layer == 0 && !open.is_empty() => field = Some("title"),
                    _ if !open.is_empty() => depth_in_layer += 1,
                    _ => {}
                }
            }
            event @ (Event::Text(_) | Event::CData(_) | Event::GeneralRef(_)) => {
                if let (Some(kind), Some(layer)) = (field, open.last_mut()) {
                    let piece = match &event {
                        Event::Text(text) => text.xml10_content().into_owned(),
                        Event::CData(text) => text.xml10_content().into_owned(),
                        Event::GeneralRef(reference) => {
                            reference_text(reference).unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    let slot = match kind {
                        "name" => &mut layer.0,
                        _ => &mut layer.1,
                    };
                    slot.get_or_insert_with(String::new).push_str(&piece);
                }
            }
            Event::End(element) => match element.local_name().as_ref() {
                "Layer" => {
                    if let Some((Some(name), title)) = open.pop() {
                        let name = name.trim().to_owned();
                        let title = title.map(|title| title.trim().to_owned());
                        if !name.is_empty() {
                            layers.push(WmsLayer {
                                title: title
                                    .filter(|title| !title.is_empty())
                                    .unwrap_or_else(|| name.clone()),
                                name,
                            });
                        }
                    }
                    depth_in_layer = 0;
                }
                "Name" | "Title" if field.is_some() => field = None,
                _ if depth_in_layer > 0 => depth_in_layer -= 1,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if layers.is_empty() {
        return Err(crate::Error::msg(
            "the service lists no named layers (is it a WMS?)",
        ));
    }
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_layers_are_listed_with_their_titles_but_not_their_styles() {
        let xml = br#"<WMS_Capabilities><Capability><Layer><Title>Root</Title>
            <Layer queryable="1"><Name>GEBCO_LATEST</Name><Title>GEBCO grid</Title>
              <Style><Name>default</Name><Title>Default</Title></Style></Layer>
            <Layer><Name>contours</Name></Layer>
          </Layer></Capability></WMS_Capabilities>"#;
        let layers = parse(xml).unwrap();
        assert_eq!(
            layers,
            [
                WmsLayer {
                    name: "GEBCO_LATEST".into(),
                    title: "GEBCO grid".into()
                },
                WmsLayer {
                    name: "contours".into(),
                    title: "contours".into()
                },
            ]
        );
        assert!(parse(b"<html></html>").is_err());
    }

    #[test]
    fn titles_keep_their_entities_and_character_data() {
        let xml = br#"<WMS_Capabilities><Capability><Layer>
            <Layer><Name>relief</Name><Title>Bathymetry &amp; topography</Title></Layer>
            <Layer><Name>caf&#233;s</Name><Title><![CDATA[Caf&eacute;s <new>]]></Title></Layer>
            <Layer><Name>  spaced  </Name><Title>
                Over two lines
            </Title></Layer>
          </Layer></Capability></WMS_Capabilities>"#;
        let layers = parse(xml).unwrap();
        let pairs: Vec<(&str, &str)> = layers
            .iter()
            .map(|layer| (layer.name.as_str(), layer.title.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("relief", "Bathymetry & topography"),
                ("caf\u{e9}s", "Caf&eacute;s <new>"),
                ("spaced", "Over two lines"),
            ]
        );
    }
}
