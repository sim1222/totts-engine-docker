use std::{env, path::PathBuf, time::Duration};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug)]
pub struct AppConfig {
    pub api_token: String,
    pub max_concurrency: usize,
    pub queue_limit: usize,
    pub timeout: Duration,
    pub work_dir: PathBuf,
    pub rootfs: PathBuf,
    pub qemu: PathBuf,
    pub harness: PathBuf,
    pub voices_path: PathBuf,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let api_token = env::var("TTS_API_TOKEN").context("TTS_API_TOKEN is required")?;
        if api_token.is_empty() {
            bail!("TTS_API_TOKEN must not be empty");
        }
        let max_concurrency = parse_usize("TTS_MAX_CONCURRENCY", 1)?;
        if max_concurrency == 0 {
            bail!("TTS_MAX_CONCURRENCY must be at least 1");
        }
        Ok(Self {
            api_token,
            max_concurrency,
            queue_limit: parse_usize("TTS_QUEUE_LIMIT", 8)?,
            timeout: Duration::from_secs(parse_u64("TTS_TIMEOUT_SEC", 30)?),
            work_dir: env_path("TTS_WORK_DIR", "/tmp/tts"),
            rootfs: env_path("TTS_ROOTFS", "/opt/tts/rootfs"),
            qemu: env_path("TTS_QEMU", "/usr/bin/qemu-arm-static"),
            harness: env_path("TTS_HARNESS", "/usr/local/bin/tts-harness"),
            voices_path: env_path("TTS_VOICES_CONFIG", "/etc/tts/voices.yaml"),
        })
    }
}

fn env_path(name: &str, default: &str) -> PathBuf {
    PathBuf::from(env::var(name).unwrap_or_else(|_| default.to_owned()))
}

fn parse_usize(name: &str, default: usize) -> Result<usize> {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .with_context(|| format!("{name} must be a non-negative integer"))
}

fn parse_u64(name: &str, default: u64) -> Result<u64> {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .with_context(|| format!("{name} must be a non-negative integer"))
}

#[derive(Clone, Debug, Deserialize)]
pub struct VoicesConfig {
    pub voices: Vec<Voice>,
}

impl VoicesConfig {
    pub async fn load(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path).await?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let config: Self = serde_yaml::from_str(content)?;
        if config.voices.is_empty() {
            bail!("voices list must not be empty");
        }
        for voice in &config.voices {
            if voice.id.is_empty() || voice.language.is_empty() {
                bail!("voice id and language must not be empty");
            }
        }
        Ok(config)
    }

    pub fn find(&self, id: &str) -> Option<&Voice> {
        self.voices.iter().find(|voice| voice.id == id)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Voice {
    pub id: String,
    pub display_name: String,
    pub language: String,
    pub gender: Gender,
    pub description: String,
    #[serde(skip_serializing)]
    pub engine: EngineVoice,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Gender {
    Male,
    Female,
    Unknown,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EngineVoice {
    pub language: String,
    pub country: String,
    #[serde(default)]
    pub variant: String,
    pub voice_id: String,
    pub encoding: TextEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEncoding {
    Utf8,
    Cp932,
    Gbk,
    EucKr,
}

impl TextEncoding {
    pub fn encode(self, text: &str) -> Result<Vec<u8>> {
        let encoding = match self {
            Self::Utf8 => return Ok(text.as_bytes().to_vec()),
            Self::Cp932 => encoding_rs::SHIFT_JIS,
            Self::Gbk => encoding_rs::GBK,
            Self::EucKr => encoding_rs::EUC_KR,
        };
        let (output, _, had_errors) = encoding.encode(text);
        if had_errors {
            bail!("text contains characters unsupported by the selected voice encoding");
        }
        Ok(output.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_voice_yaml_and_cp932() {
        let yaml = r#"voices:
  - id: ja
    display_name: Japanese
    language: ja-JP
    gender: female
    description: test
    engine:
      language: jpn
      country: JPN
      voice_id: female01
      encoding: cp932
"#;
        let config = VoicesConfig::parse(yaml).expect("test YAML should parse");
        assert_eq!(config.voices.len(), 1);
        assert_eq!(config.voices[0].gender, Gender::Female);
        let encoded = config.voices[0]
            .engine
            .encoding
            .encode("テスト")
            .expect("CP932 test text should encode");
        assert_eq!(encoded, vec![0x83, 0x65, 0x83, 0x58, 0x83, 0x67]);
    }
}
