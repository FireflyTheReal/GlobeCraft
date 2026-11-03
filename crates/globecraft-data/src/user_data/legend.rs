//! The legend a WMS draws for a layer (GetLegendGraphic), shown under the layer in
//! the Layers pane.
use image::RgbaImage;

/// The largest legend read, and the largest side kept.
const MAX_LEGEND_BYTES: u64 = 4 * 1024 * 1024;
const MAX_LEGEND_SIDE_PX: u32 = 2048;
/// Query keys of a GetMap request that do not carry over to GetLegendGraphic.
const GET_MAP_KEYS: [&str; 12] = [
    "service",
    "request",
    "version",
    "layers",
    "styles",
    "crs",
    "srs",
    "bbox",
    "width",
    "height",
    "format",
    "transparent",
];

/// The GetLegendGraphic address for a WMS GetMap tile template (see
/// [`super::wms_template`]): its first layer and style, the service's own extra
/// parameters (MapServer's `map=`, say) kept. `None` for other templates.
pub fn wms_legend_url(template: &str) -> Option<String> {
    let (base, query) = template.split_once('?')?;
    let params: Vec<(&str, &str)> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect();
    let value = |key: &str| {
        params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| *v)
    };
    if !value("request")?.eq_ignore_ascii_case("GetMap") {
        return None;
    }
    let layer = value("layers")?
        .split(',')
        .next()
        .filter(|l| !l.is_empty())?;
    let style = value("styles")
        .and_then(|styles| styles.split(',').next())
        .filter(|s| !s.is_empty());
    let version = value("version").unwrap_or("1.3.0");
    let mut url = format!("{base}?");
    for (key, value) in &params {
        if !GET_MAP_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k)) {
            url.push_str(&format!("{key}={value}&"));
        }
    }
    url.push_str(&format!(
        "SERVICE=WMS&REQUEST=GetLegendGraphic&VERSION={version}&FORMAT=image/png&LAYER={layer}&SLD_VERSION=1.1.0"
    ));
    if let Some(style) = style {
        url.push_str(&format!("&STYLE={style}"));
    }
    Some(url)
}

/// Fetches and decodes a legend image.
pub async fn legend_image(url: String) -> Result<RgbaImage, crate::Error> {
    let client = crate::http::client(crate::http::Policy::UserData)?;
    let bytes = crate::http::body(client.get(&url), MAX_LEGEND_BYTES).await?;
    let image = crate::cpu::run(crate::cpu::Service::Tiles, move || {
        super::raster::decode(&bytes)
    })
    .await?;
    if image.width() > MAX_LEGEND_SIDE_PX || image.height() > MAX_LEGEND_SIDE_PX {
        return Err(format!(
            "the legend is {}×{} pixels, larger than {MAX_LEGEND_SIDE_PX}",
            image.width(),
            image.height()
        )
        .into());
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_get_map_template_gives_its_first_layer_legend() {
        let template = super::super::wms_template(
            "https://ows.example/wms?map=/srv/depth.map",
            "emodnet:mean_2018,coastlines",
        );
        let url = wms_legend_url(&template).unwrap();
        assert_eq!(
            url,
            "https://ows.example/wms?map=/srv/depth.map&SERVICE=WMS&REQUEST=GetLegendGraphic\
             &VERSION=1.3.0&FORMAT=image/png&LAYER=emodnet:mean_2018&SLD_VERSION=1.1.0"
        );
        assert!(!url.contains("{bbox}"));
        let styled = template.replace("STYLES=", "STYLES=shaded");
        assert!(wms_legend_url(&styled).unwrap().ends_with("&STYLE=shaded"));
    }

    #[test]
    fn tile_templates_have_no_wms_legend() {
        assert_eq!(
            wms_legend_url("https://tiles.example/{z}/{x}/{y}.png"),
            None
        );
        assert_eq!(
            wms_legend_url("https://tiles.example/{z}/{x}/{y}.png?key=abc"),
            None
        );
    }

    /// The legends of the WMS presets load (network: run on a machine that reaches
    /// the services; this build container does not).
    #[tokio::test]
    #[ignore = "network: fetches the legends of the WMS presets"]
    async fn the_wms_presets_have_legends() {
        for preset in super::super::presets::PRESETS {
            let super::super::UserSource::Tiles { template, .. } = preset.source() else {
                continue;
            };
            let Some(url) = wms_legend_url(&template) else {
                continue;
            };
            let image = legend_image(url.clone())
                .await
                .unwrap_or_else(|error| panic!("{}: {url}: {error}", preset.code));
            println!("{}: {}×{}", preset.code, image.width(), image.height());
            assert!(image.width() > 0 && image.height() > 0);
        }
    }
}
