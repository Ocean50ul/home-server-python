use cpal::SupportedStreamConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StreamMetadata {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String
}

impl StreamMetadata {
    pub fn from_cpal_config(config: &SupportedStreamConfig) -> Self {
        Self {
            sample_rate: config.sample_rate().0,
            channels: config.channels(),
            format: config.sample_format().to_string()
        }
    }
}