extern crate toml;

#[derive(Deserialize)]
pub struct SetupParams {
    name: String,
    device: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
pub struct AudoParms {
    device: String, // Output device
    bitrate: Option<i8>,
    mixer_name: Option<String>,
    mixer_card: Option<String>,
    mixer_index: Option<String>,
    // Volume params
    initial_volume: Option<i8>,
    volume_normalisation: Option<bool>,
    normalisation_pregain: Option<bool>,
    logarithmic_volume: Option<bool>,
}
