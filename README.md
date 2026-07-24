# ToSpeak TTS API

> [!WARNING]
> This project is 100% AI Generated. Use at your own risk.

This project runs the Toshiba ToSpeak G3 engine extracted from a privately
owned storage dump and exposes it through a small authenticated HTTP API.
The proprietary engine, dictionaries, and Android libraries remain in the
host's ignored `rootfs/` directory. They are never copied into the Docker image.

## Setup

The checked-out workspace already contains the extracted private runtime when
the extraction phases have been completed. For a fresh dump, follow
[`docs/extraction.md`](docs/extraction.md) to create `rootfs/`, preserve the
original files, apply the documented compatibility dependency changes, build
the ARM log shim, and install the verified `tospeak.xml` path.

Create local configuration and start the service:

```sh
cp .env.example .env
# Replace TTS_API_TOKEN in .env with a strong random secret,
# or leave it empty to disable authentication.
docker compose up --build
```

The API listens on `http://localhost:8080`. `/` and `/healthz` are always public.
When `TTS_API_TOKEN` is non-empty, all other API routes require
`Authorization: Bearer <TTS_API_TOKEN>`. Authentication is disabled when the
variable is empty or unset.

A single-file browser client is available at `http://localhost:8080/`. Enter
the configured API token when authentication is enabled, or leave it blank when
authentication is disabled, to load voices, synthesize audio, and save the WAV.

```sh
curl http://localhost:8080/healthz
curl -H "Authorization: Bearer $TTS_API_TOKEN" \
  http://localhost:8080/voices
curl -H "Authorization: Bearer $TTS_API_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"text":"テストです。","voice_id":"female01"}' \
  http://localhost:8080/tts --output speech.wav
```

The complete API contract is [`docs/openapi.yaml`](docs/openapi.yaml).

## Architecture and dependencies

The axum/Tokio server is compiled and runs natively on amd64. It invokes an
ARMv7 softfp harness with explicit `qemu-arm-static -L /opt/tts/rootfs`; this
does not depend on host binfmt registration and avoids emulating the HTTP
server. Full-system qemu was unnecessary because synthesis needs no Binder,
audio device, kernel driver, or special ioctl.

Principal Rust crates:

- `axum`, `tokio`, and `tower-http`: HTTP routing, async child control, body
  limit, and tracing.
- `serde`, `serde_json`, and `serde_yaml`: API requests and external voice
  definitions.
- `hound`: deterministic 16-bit PCM WAV construction and validation.
- `encoding_rs`: required UTF-8 to CP932/GBK/EUC-KR conversion.
- `thiserror` and `anyhow`: typed HTTP errors and internal context.
- `uuid`, `sha2`, and `constant_time_eq`: request IDs, text-only hashes, and
  constant-time bearer-token comparison.
- `tempfile`: request work directories removed by RAII even on errors.

## Engine parameters

The native ranges were measured by actual qemu synthesis. Mappings are applied
before invoking the harness:

| API | Native ToSpeak | Mapping |
| --- | --- | --- |
| volume `0.0..1.0` | `0..100` | linear: `round(volume * 100)` |
| rate `0.5..6.0` | `-10..10` | logarithmic: `round(10 * ln(rate) / ln(6))` |
| pitch `0.0..2.0` | `-10..10` | linear: `round((pitch - 1) * 10)` |

The engine emits raw mono signed 16-bit PCM at its native 22,050 Hz. The Rust
wrapper retains that sample rate and adds a standards-compliant WAV header.

## Operational behavior

- Concurrent engine instances default to one. `TTS_QUEUE_LIMIT` bounds waiting
  requests; excess requests receive 429.
- `TTS_TIMEOUT_SEC` bounds each child execution. A timed-out qemu child is
  killed and reaped before returning 504.
- The container runs as uid/gid 10001 with a read-only root filesystem, all
  capabilities dropped, and only `/tmp/tts` writable as tmpfs.
- Request logs contain voice ID, character count, SHA-256 text hash, and UUID;
  they never contain synthesis text.

## Known limitations

- Japanese input is converted to CP932 because the engine rejects UTF-8
  Japanese. Unsupported characters return a synthesis error.
- Japanese, US English, and Chinese were successfully synthesized during
  extraction validation. The installed Korean voice lacks a Korean language
  dictionary in the dump and currently makes health-independent synthesis fail.
- Each locale contains one primary voice. The public voice ID selects the
  corresponding installed dictionary; there is no alternate speaker per locale.
- The qemu runtime uses a compatibility copy of the engine with five unused
  Android framework `DT_NEEDED` entries removed. The unmodified binary and its
  SHA-256 are retained beside it in private `rootfs/`.
