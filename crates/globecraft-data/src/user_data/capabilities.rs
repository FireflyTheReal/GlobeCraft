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

/// The `Layer` elements that have a `Name`, with their `Title`, in document order.
fn parse(bytes: &[u8]) -> Result<Vec<WmsLayer>, crate::Error> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
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
            Event::Text(text) => {
                if let (Some(kind), Some(layer)) = (field, open.last_mut()) {
                    let text = text.xml10_content().trim().to_owned();
                    match kind {
                        "name" => layer.0 = Some(text),
                        _ => layer.1 = Some(text),
                    }
                }
            }
            Event::End(element) => match element.local_name().as_ref() {
                "Layer" => {
                    if let Some((Some(name), title)) = open.pop() {
                        layers.push(WmsLayer {
                            title: title.unwrap_or_else(|| name.clone()),
                            name,
                        });
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
        return Err("the service lists no named layers (is it a WMS?)".into());
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
}
