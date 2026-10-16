# Thin wrapper over cargo: builds an example and copies it to build/<name>.elf.
#   make                         build all examples
#   make EXAMPLE=hwinfo test     build + send to the PS5 loader
PS5_PAYLOAD_SDK ?= /opt/ps5-payload-sdk
PS5_HOST ?= ps5
PS5_PORT ?= 9021
EXAMPLES := hello_notify hwinfo http_test http_server threads
EXAMPLE  ?= hello_notify
export PS5_PAYLOAD_SDK

OUT := target/x86_64-sie-ps5/release

all:
	cargo build --release $(addprefix -p ,$(EXAMPLES))
	@mkdir -p build
	@for e in $(EXAMPLES); do \
	  $(PS5_PAYLOAD_SDK)/bin/prospero-strip -o build/$$e.elf $(OUT)/$$e && echo "build/$$e.elf"; \
	done

test: all
	$(PS5_PAYLOAD_SDK)/bin/prospero-deploy -h $(PS5_HOST) -p $(PS5_PORT) build/$(EXAMPLE).elf

clean:
	cargo clean; rm -rf build

.PHONY: all test clean
