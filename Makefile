.PHONY: all build run clean

TARGET = x86_64-unknown-none
KERNEL = target/$(TARGET)/release/sos

all: build

build:
	rustup target add $(TARGET) 2>/dev/null || true
	cargo build --release --target $(TARGET)

run: build
	@echo "Booting QEMU (press Ctrl+A, then X to quit)..."
	@echo "Tip: Run 'tail -f serial.log' in another terminal to see kernel output"
	@rm -f serial.log
	./boot_qemu.sh 

clean:
	cargo clean
	rm -f $(KERNEL).bin
