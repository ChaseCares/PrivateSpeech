use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Config {
    pub url: url::Url,
    pub tmp_dir: String,
    pub speaker_id: Option<String>,
    pub model_name: Option<String>,
    pub playback_speed: f32,
    pub min_length: usize,
    pub timeout: usize,
    pub substitutions: Vec<(String, String)>,
    pub strip_regexes: Vec<String>,
    pub quick_first_chunk: bool,
    pub quick_first_chunk_length: usize,
    pub split_on: Vec<char>,
}

impl Config {
    fn validate(&self) -> Result<(), String> {
        if self.url == url::Url::parse("http://[0100::0]:5002").unwrap() {
            return Err("Please set the url in the config file".to_string());
        }

        Ok(())
    }
}

impl TryFrom<&str> for Config {
    type Error = String;

    fn try_from(file_content: &str) -> Result<Self, Self::Error> {
        let config: Self = toml::from_str(file_content).unwrap();
        config.validate()?;
        Ok(config)
    }
}
