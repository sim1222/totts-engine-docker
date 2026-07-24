typedef unsigned int size_t;

static int raw_write(int fd, const void *buffer, size_t size) {
  register int r0 __asm__("r0") = fd;
  register const void *r1 __asm__("r1") = buffer;
  register size_t r2 __asm__("r2") = size;
  register int r7 __asm__("r7") = 4;
  __asm__ volatile("svc 0" : "+r"(r0) : "r"(r1), "r"(r2), "r"(r7) : "memory");
  return r0;
}

static size_t text_length(const char *text) {
  size_t length = 0;
  if (text != 0) {
    while (text[length] != '\0') {
      ++length;
    }
  }
  return length;
}

static void write_number(unsigned int value, unsigned int base) {
  char digits[16];
  size_t used = 0;
  do {
    unsigned int digit = value % base;
    digits[used++] = digit < 10 ? (char)('0' + digit) : (char)('a' + digit - 10);
    value /= base;
  } while (value != 0 && used < sizeof(digits));
  while (used != 0) {
    --used;
    raw_write(2, digits + used, 1);
  }
}

int __android_log_print(int priority, const char *tag, const char *format, ...) {
  (void)priority;
  __builtin_va_list args;
  __builtin_va_start(args, format);
  raw_write(2, tag, text_length(tag));
  raw_write(2, ": ", 2);
  for (size_t offset = 0; format != 0 && format[offset] != '\0'; ++offset) {
    if (format[offset] != '%' || format[offset + 1] == '\0') {
      raw_write(2, format + offset, 1);
      continue;
    }
    ++offset;
    if (format[offset] == 's') {
      const char *value = __builtin_va_arg(args, const char *);
      raw_write(2, value, text_length(value));
    } else if (format[offset] == 'd' || format[offset] == 'i') {
      int value = __builtin_va_arg(args, int);
      if (value < 0) {
        raw_write(2, "-", 1);
        write_number((unsigned int)-value, 10);
      } else {
        write_number((unsigned int)value, 10);
      }
    } else if (format[offset] == 'p') {
      write_number(__builtin_va_arg(args, unsigned int), 16);
    } else if (format[offset] == '%') {
      raw_write(2, "%", 1);
    } else {
      // Width/precision formats are uncommon in the diagnostic messages used
      // here; consume no argument and preserve the marker.
      raw_write(2, "%", 1);
      raw_write(2, format + offset, 1);
    }
  }
  __builtin_va_end(args);
  raw_write(2, "\n", 1);
  return 0;
}

int __android_log_write(int priority, const char *tag, const char *text) {
  return __android_log_print(priority, tag, text);
}
