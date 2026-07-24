#ifndef TTS_ENGINE_ABI_H
#define TTS_ENGINE_ABI_H

// Android's legacy TtsEngine ABI as used by the extracted Android 4.x image.
// The init() configuration argument and vtable order match AOSP commit
// b4882ca26fbf55c385fbc2b37e1bab6a13ee963a.

typedef unsigned int uint32_t;
typedef unsigned int size_t;
typedef signed char int8_t;

namespace android {

enum tts_synth_status { TTS_SYNTH_DONE = 0, TTS_SYNTH_PENDING = 1 };
enum tts_callback_status { TTS_CALLBACK_HALT = 0, TTS_CALLBACK_CONTINUE = 1 };
typedef tts_callback_status(synthDoneCB_t)(void *&, uint32_t, uint32_t, int,
                                           int8_t *&, size_t &,
                                           tts_synth_status);

enum tts_result {
  TTS_SUCCESS = 0,
  TTS_FAILURE = -1,
  TTS_FEATURE_UNSUPPORTED = -2,
  TTS_VALUE_INVALID = -3,
  TTS_PROPERTY_UNSUPPORTED = -4,
  TTS_PROPERTY_SIZE_TOO_SMALL = -5,
  TTS_MISSING_RESOURCES = -6
};

enum tts_support_result {
  TTS_LANG_COUNTRY_VAR_AVAILABLE = 2,
  TTS_LANG_COUNTRY_AVAILABLE = 1,
  TTS_LANG_AVAILABLE = 0,
  TTS_LANG_MISSING_DATA = -1,
  TTS_LANG_NOT_SUPPORTED = -2
};

class TtsEngine {
public:
  virtual ~TtsEngine() {}
  virtual tts_result init(synthDoneCB_t callback, const char *engine_config);
  virtual tts_result shutdown();
  virtual tts_result stop();
  virtual tts_support_result isLanguageAvailable(const char *lang,
                                                  const char *country,
                                                  const char *variant);
  virtual tts_result loadLanguage(const char *lang, const char *country,
                                  const char *variant);
  virtual tts_result setLanguage(const char *lang, const char *country,
                                 const char *variant);
  virtual tts_result getLanguage(char *language, char *country, char *variant);
  virtual tts_result setAudioFormat(uint32_t &encoding, uint32_t &rate,
                                    int &channels);
  virtual tts_result setProperty(const char *property, const char *value,
                                 size_t size);
  virtual tts_result getProperty(const char *property, char *value,
                                 size_t *size);
  virtual tts_result synthesizeText(const char *text, int8_t *buffer,
                                    size_t buffer_size, void *userdata);
};

extern "C" TtsEngine *getTtsEngine();

} // namespace android

#endif
