.PHONY: all build run iso run-iso phase4-stress clean

TARGET = x86_64-unknown-none
KERNEL = target/$(TARGET)/release/sos
ISO = target/sos.iso
ITER ?= 100

all: build

build:
	rustup target add $(TARGET) 2>/dev/null || true
	cargo build --release --target $(TARGET)

run: build
	@echo "Booting QEMU (press Ctrl+A, then X to quit)..."
	@echo "Tip: Run 'tail -f serial.log' in another terminal to see kernel output"
	@rm -f serial.log
	./boot_qemu.sh 

iso: build
	./build_iso.sh

run-iso: iso
	@echo "Booting QEMU from GRUB ISO (press Ctrl+A, then X to quit)..."
	@echo "Tip: Run 'tail -f serial.log' in another terminal to see kernel output"
	@rm -f serial.log
	./boot_iso_qemu.sh

phase4-stress:
	./scripts/phase4-stress.sh $(ITER)

clean:
	cargo clean
	rm -f $(KERNEL).bin $(ISO)
