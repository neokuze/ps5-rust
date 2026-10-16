PS5_PAYLOAD_SDK ?= /opt/ps5-payload-sdk
PS5_HOST ?= ps5
PS5_PORT ?= 9021
export PS5_PAYLOAD_SDK

# Payload to build/deploy: make PAYLOAD=hello_notify
PAYLOAD ?= hello_notify
# Sony libs beyond the SDK defaults, one line in payloads/<name>/sce_libs
# (each ps5 module's doc comment says which it needs).
SCE_LIBS ?= $(shell cat payloads/$(PAYLOAD)/sce_libs 2>/dev/null)

ELF := $(PAYLOAD).elf
LIB := target/x86_64-unknown-linux-gnu/release/lib$(PAYLOAD).a

all: $(ELF)

$(LIB): FORCE
	cargo build --release -p $(PAYLOAD)

$(ELF): $(LIB)
	$(PS5_PAYLOAD_SDK)/bin/prospero-clang -o $@ $< $(SCE_LIBS)
	$(PS5_PAYLOAD_SDK)/bin/prospero-strip $@

test: $(ELF)
	$(PS5_PAYLOAD_SDK)/bin/prospero-deploy -h $(PS5_HOST) -p $(PS5_PORT) $<

clean:
	cargo clean; rm -f *.elf

FORCE:
.PHONY: all test clean FORCE
