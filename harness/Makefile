CXX := arm-linux-gnueabi-g++
CC := arm-linux-gnueabi-gcc
ROOTFS ?= ../rootfs
CXXFLAGS := -march=armv7-a -mfloat-abi=softfp -mfpu=neon -Os -fPIC \
	-fno-exceptions -fno-rtti -fno-stack-protector -nostdinc++ -Wall -Wextra
LDFLAGS := -nostdlib -pie -Wl,-e,_start \
	-Wl,--dynamic-linker,/system/bin/linker -Wl,-rpath,/system/lib \
	-Wl,--hash-style=sysv,--disable-new-dtags,-z,now \
	-Wl,-E \
	-L$(ROOTFS)/system/lib -Wl,-rpath-link,$(ROOTFS)/system/lib \
	-Wl,--allow-shlib-undefined -Wl,--no-as-needed

all: tts-harness liblog_stub.so

tts-harness: start.o main.o
	$(CXX) $(LDFLAGS) -o $@ $^ -lTtsToSpeakG3V1_JP -lc -ldl -lm -lstdc++ -lgcc

start.o: start.S
	$(CC) -march=armv7-a -mfloat-abi=softfp -mfpu=neon -c -o $@ $<

main.o: main.cpp tts_engine_abi.h
	$(CXX) $(CXXFLAGS) -c -o $@ $<

liblog_stub.so: log_stub.c
	$(CC) -march=armv7-a -mfloat-abi=softfp -fPIC -fno-stack-protector \
		-nostdlib -shared -Wl,--hash-style=sysv,-soname,liblog.so \
		-L$(ROOTFS)/system/lib -o $@ $< -lgcc -lc

clean:
	rm -f tts-harness liblog_stub.so start.o main.o

.PHONY: all clean
