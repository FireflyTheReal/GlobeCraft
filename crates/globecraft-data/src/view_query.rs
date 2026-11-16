//! Where the camera looks, in the one shape the live services and tile streams ask for.
use globecraft_core::camera::Cam;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewQuery {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_km: f64,
    /// From straight down, degrees.
    pub tilt_deg: f64,
    /// The bearing the top of the screen points to, degrees.
    pub heading_deg: f64,
}

impl From<Cam> for ViewQuery {
    fn from(camera: Cam) -> Self {
        Self {
            latitude: camera.lat,
            longitude: camera.lon,
            altitude_km: camera.alt,
            tilt_deg: camera.tilt,
            heading_deg: camera.heading,
        }
    }
}
