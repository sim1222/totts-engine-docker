use std::{
    io::Cursor,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::{bail, Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use tokio::{
    fs,
    io::AsyncWriteExt,
    process::Command,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::timeout,
};
use tracing::{error, warn};

use crate::config::{AppConfig, Voice, VoicesConfig};

pub struct Engine {
    config: Arc<AppConfig>,
    semaphore: Arc<Semaphore>,
    waiting: AtomicUsize,
    healthy: AtomicBool,
}

pub enum EngineFailure {
    QueueFull,
    Timeout,
    Failed(anyhow::Error),
}

impl Engine {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
            config,
            waiting: AtomicUsize::new(0),
            healthy: AtomicBool::new(false),
        }
    }

    pub fn healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub fn set_healthy(&self, healthy: bool) {
        self.healthy.store(healthy, Ordering::Relaxed);
    }

    pub async fn smoke_test(&self, voices: &VoicesConfig) -> bool {
        let Some(voice) = voices.voices.first() else {
            return false;
        };
        match self.synthesize(voice, "test", 1.0, 1.0, 1.0).await {
            Ok(audio) => validate_wav(&audio).is_ok(),
            Err(EngineFailure::Failed(error)) => {
                error!(%error, "smoke test synthesis error");
                false
            }
            Err(_) => false,
        }
    }

    pub async fn synthesize(
        &self,
        voice: &Voice,
        text: &str,
        volume: f64,
        rate: f64,
        pitch: f64,
    ) -> Result<Vec<u8>, EngineFailure> {
        let permit = self.acquire().await?;
        let result = self.run(voice, text, volume, rate, pitch, permit).await;
        match result {
            Ok(bytes) => Ok(bytes),
            Err(RunFailure::Timeout) => Err(EngineFailure::Timeout),
            Err(RunFailure::Failed(error)) => Err(EngineFailure::Failed(error)),
        }
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, EngineFailure> {
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            return Ok(permit);
        }
        let admitted = self
            .waiting
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |waiting| {
                (waiting < self.config.queue_limit).then_some(waiting + 1)
            });
        if admitted.is_err() {
            return Err(EngineFailure::QueueFull);
        }
        let _waiting_guard = WaitingGuard(&self.waiting);
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| EngineFailure::Failed(error.into()))
    }

    async fn run(
        &self,
        voice: &Voice,
        text: &str,
        volume: f64,
        rate: f64,
        pitch: f64,
        _permit: OwnedSemaphorePermit,
    ) -> Result<Vec<u8>, RunFailure> {
        let work = tempfile::Builder::new()
            .prefix("request-")
            .tempdir_in(&self.config.work_dir)
            .context("create request work directory")
            .map_err(RunFailure::Failed)?;
        let pcm_path = work.path().join("output.pcm");
        let input = voice
            .engine
            .encoding
            .encode(text)
            .map_err(RunFailure::Failed)?;
        let native_volume = map_volume(volume).to_string();
        let native_rate = map_rate(rate).to_string();
        let native_pitch = map_pitch(pitch).to_string();
        let dictionary = self
            .config
            .rootfs
            .join("system/media/TTS/")
            .to_string_lossy()
            .into_owned();
        let license = self
            .config
            .rootfs
            .join("system/etc/TTS/libtts_uselimit.bin")
            .to_string_lossy()
            .into_owned();

        let mut child = Command::new(&self.config.qemu)
            .arg("-L")
            .arg(&self.config.rootfs)
            .arg(&self.config.harness)
            .args([
                &voice.engine.language,
                &voice.engine.country,
                &voice.engine.variant,
                &voice.engine.voice_id,
                &native_rate,
                &native_pitch,
                &native_volume,
            ])
            .arg(&pcm_path)
            .arg(dictionary)
            .arg(license)
            .arg("tospeak")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // Engine diagnostics can echo synthesis text. Discard them so the
            // API's structured logs never contain request text.
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("spawn qemu TTS process")
            .map_err(RunFailure::Failed)?;

        let Some(mut stdin) = child.stdin.take() else {
            return Err(RunFailure::Failed(anyhow::anyhow!("child stdin missing")));
        };
        stdin
            .write_all(&input)
            .await
            .context("write synthesis input")
            .map_err(RunFailure::Failed)?;
        drop(stdin);

        let status = match timeout(self.config.timeout, child.wait()).await {
            Ok(waited) => waited
                .context("wait for qemu TTS process")
                .map_err(RunFailure::Failed)?,
            Err(_) => {
                if let Err(error) = child.kill().await {
                    warn!(%error, "failed to kill timed-out child");
                }
                if let Err(error) = child.wait().await {
                    warn!(%error, "failed to reap timed-out child");
                }
                return Err(RunFailure::Timeout);
            }
        };
        if !status.success() {
            return Err(RunFailure::Failed(anyhow::anyhow!(
                "TTS harness exited with {status}"
            )));
        }
        let pcm = fs::read(&pcm_path)
            .await
            .context("read generated PCM")
            .map_err(RunFailure::Failed)?;
        pcm_to_wav(&pcm).map_err(RunFailure::Failed)
    }
}

enum RunFailure {
    Timeout,
    Failed(anyhow::Error),
}

struct WaitingGuard<'a>(&'a AtomicUsize);

impl Drop for WaitingGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

// ToSpeak's measured native volume scale accepts 0..100; map the API's linear
// amplitude scale directly so 0.0 is silence and 1.0 preserves full volume.
pub fn map_volume(value: f64) -> i32 {
    (value * 100.0).round() as i32
}

// ToSpeak's measured rate range is -10..10 with 0 as its natural rate. A
// logarithmic ratio makes API rate 1.0 map to 0 while 6.0 maps to +10 and
// preserves multiplicative speed semantics below 1.0.
pub fn map_rate(value: f64) -> i32 {
    (10.0 * value.ln() / 6.0_f64.ln())
        .round()
        .clamp(-10.0, 10.0) as i32
}

// ToSpeak's measured pitch range is -10..10. The API is centered at 1.0, so
// the exact linear conversion is native = (pitch - 1) * 10.
pub fn map_pitch(value: f64) -> i32 {
    ((value - 1.0) * 10.0).round() as i32
}

pub fn pcm_to_wav(pcm: &[u8]) -> Result<Vec<u8>> {
    if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
        bail!("engine returned invalid 16-bit PCM length");
    }
    let mut cursor = Cursor::new(Vec::with_capacity(pcm.len() + 44));
    {
        let mut writer = WavWriter::new(
            &mut cursor,
            WavSpec {
                channels: 1,
                sample_rate: 22_050,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
        )?;
        for bytes in pcm.chunks_exact(2) {
            writer.write_sample(i16::from_le_bytes([bytes[0], bytes[1]]))?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

fn validate_wav(bytes: &[u8]) -> Result<()> {
    let reader = hound::WavReader::new(Cursor::new(bytes))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 22_050 || spec.bits_per_sample != 16 {
        bail!("unexpected WAV format");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_parameters() {
        assert_eq!(map_volume(0.0), 0);
        assert_eq!(map_volume(1.0), 100);
        assert_eq!(map_rate(1.0), 0);
        assert_eq!(map_rate(6.0), 10);
        assert!(map_rate(0.5) < 0);
        assert_eq!(map_pitch(0.0), -10);
        assert_eq!(map_pitch(1.0), 0);
        assert_eq!(map_pitch(2.0), 10);
    }

    #[test]
    fn wraps_pcm_in_wav() {
        let wav = pcm_to_wav(&[0, 0, 0xff, 0x7f]).expect("valid PCM should wrap");
        let mut reader = hound::WavReader::new(Cursor::new(wav)).expect("WAV should parse");
        assert_eq!(reader.spec().sample_rate, 22_050);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert_eq!(reader.samples::<i16>().count(), 2);
    }
}
