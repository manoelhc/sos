.PHONY: all build run clean

TARGET = x86_64-unknown-none
KERNEL = target/$(TARGET)/release/sos

all: build

build:
	rustup target add $(TARGET) 2>/dev/null || true
	cargo build --release --target $(TARGET)

run: build
	@echo "Creating boot image..."
	objcopy -O binary $(KERNEL) $(KERNEL).bin
	@echo "Booting QEMU (press Ctrl+A, then X to quit)..."
	qemu-system-x86_64 \
		-machine pc \
		-kernel $(KERNEL).bin \
		-m 128M \
		-display none \
		-serial stdio \
		-no-reboot

clean:
	cargo clean
	rm -f $(KERNEL).bin
