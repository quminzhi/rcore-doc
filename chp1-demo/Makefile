APP := chp1-demo
TARGET := riscv64gc-unknown-none-elf
OUT_DIR := ../target/$(TARGET)/release
ELF := $(OUT_DIR)/$(APP)
BIN := $(OUT_DIR)/$(APP).bin
ASM := $(OUT_DIR)/$(APP).asm
BOOTLOADER := ../bootloader/rustsbi-qemu.bin
QEMU := qemu-system-riscv64
GDB := riscv64-elf-gdb
OBJDUMP := rust-objdump
QEMU_ARGS := \
	-machine virt \
	-nographic \
	-bios $(BOOTLOADER) \
	-device loader,file=$(BIN),addr=0x80200000

.PHONY: build clean bin asm qemu qemu-dbg gdb

build: bin

clean:
	cargo clean

bin:
	cargo build --release
	rust-objcopy --strip-all -O binary $(ELF) $(BIN)

asm: bin
	$(OBJDUMP) -d -S -l -C $(ELF) > $(ASM)

qemu: bin
	$(QEMU) $(QEMU_ARGS)

qemu-dbg: bin
	$(QEMU) $(QEMU_ARGS) -s -S

gdb:
	$(GDB)
