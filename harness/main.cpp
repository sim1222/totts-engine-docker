#include "tts_engine_abi.h"

extern "C" int open(const char *, int, ...);
extern "C" int close(int);
extern "C" int read(int, void *, unsigned int);
extern "C" int write(int, const void *, unsigned int);
extern "C" unsigned int strlen(const char *);

extern "C" int __android_log_print(int, const char *tag, const char *format,
                                   ...) {
  if (tag != 0) {
    write(2, tag, strlen(tag));
    write(2, ": ", 2);
  }
  if (format != 0) {
    write(2, format, strlen(format));
  }
  write(2, "\n", 1);
  return 0;
}

static const int O_WRONLY = 1;
static const int O_CREAT = 0100;
static const int O_TRUNC = 01000;
static int output_fd = -1;
static bool write_failed = false;

static android::tts_callback_status synth_callback(
    void *&, uint32_t, uint32_t, int, int8_t *&audio, size_t &audio_size,
    android::tts_synth_status) {
  unsigned int written = 0;
  while (written < audio_size) {
    int result = write(output_fd, audio + written, audio_size - written);
    if (result <= 0) {
      write_failed = true;
      return android::TTS_CALLBACK_HALT;
    }
    written += static_cast<unsigned int>(result);
  }
  return android::TTS_CALLBACK_CONTINUE;
}

static int set_property(android::TtsEngine *engine, const char *name,
                        const char *value) {
  return engine->setProperty(name, value, strlen(value));
}

extern "C" int main(int argc, char **argv) {
  // lang, country, variant, voice, rate, pitch, volume, output, config, license,
  // preference
  if (argc != 12) {
    return 63;
  }

  static char text[16385];
  unsigned int used = 0;
  while (used < sizeof(text) - 1) {
    int count = read(0, text + used, sizeof(text) - 1 - used);
    if (count < 0) {
      return 64;
    }
    if (count == 0) {
      break;
    }
    used += static_cast<unsigned int>(count);
  }
  text[used] = '\0';
  if (used == 0) {
    return 65;
  }

  output_fd = open(argv[8], O_WRONLY | O_CREAT | O_TRUNC, 0600);
  if (output_fd < 0) {
    return 66;
  }

  android::TtsEngine *engine = android::getTtsEngine();
  if (engine == 0) {
    close(output_fd);
    return 67;
  }
  if (engine->init(synth_callback, argv[9]) != android::TTS_SUCCESS) {
    close(output_fd);
    return 68;
  }
  unsigned char license[256];
  int license_fd = open(argv[10], 0);
  if (license_fd < 0 || read(license_fd, license, sizeof(license)) != 256) {
    if (license_fd >= 0) {
      close(license_fd);
    }
    engine->shutdown();
    close(output_fd);
    return 75;
  }
  close(license_fd);
  if (engine->setProperty("libinfo", reinterpret_cast<const char *>(license),
                          sizeof(license)) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 76;
  }
  if (set_property(engine, "preference", argv[11]) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 77;
  }
  if (set_property(engine, "voiceid", argv[4]) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 71;
  }
  if (engine->setLanguage(argv[1], argv[2], argv[3]) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 69;
  }
  if (set_property(engine, "rate", argv[5]) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 72;
  }
  if (set_property(engine, "pitch", argv[6]) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 73;
  }
  if (set_property(engine, "volume", argv[7]) != android::TTS_SUCCESS) {
    engine->shutdown();
    close(output_fd);
    return 74;
  }

  static int8_t audio_buffer[8192];
  int result = engine->synthesizeText(text, audio_buffer, sizeof(audio_buffer), 0);
  engine->shutdown();
  close(output_fd);
  return result == android::TTS_SUCCESS && !write_failed ? 0 : 70;
}
