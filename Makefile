# Thin wrapper over cargo: builds the examples, strips them and copies them to
# build/<name>.elf (no_std) or build/std/<name>.elf (std).
#   make                                     build all no_std examples
#   make EXAMPLE=hwinfo test                 build + send one to the PS5 loader
#   make std                                 build all std examples
#   make std-test STD_EXAMPLE=tokio_server   build + send one std example; its
#                                            println! output shows up here
#   make std-run STD_EXAMPLE=println_demo    same, plus your keyboard on its stdin
# The console's address: PS5_HOST=192.168.1.49 (or set it once with
# `export PS5_HOST=192.168.1.49`).
PS5_PAYLOAD_SDK ?= /opt/ps5-payload-sdk
PS5_HOST ?= ps5
PS5_PORT ?= 9021
EXAMPLES := hello_notify hwinfo http_test http_server threads
EXAMPLE  ?= hello_notify
STD_EXAMPLES := $(shell sed -n 's/^members = \[\(.*\)\]/\1/p' std-examples/Cargo.toml | tr -d '",')
STD_EXAMPLE  ?= hello_std
export PS5_PAYLOAD_SDK

OUT     := target/x86_64-sie-ps5/release
STD_OUT := std-examples/target/x86_64-sie-ps5-std/release
DEPLOY  := $(PS5_PAYLOAD_SDK)/bin/prospero-deploy -h $(PS5_HOST) -p $(PS5_PORT)

# strip <input> <output>: strips into the system temp dir and copies only the
# contents, because on a WSL /mnt/c checkout a normal user can't chmod files
# there, which strip does to its output.
STRIP = tmp=$$(mktemp) && $(PS5_PAYLOAD_SDK)/bin/prospero-strip -o $$tmp $(1) && cat $$tmp > $(2); rc=$$?; rm -f $$tmp; test $$rc = 0

all:
	cargo build --release $(addprefix -p ,$(EXAMPLES))
	@mkdir -p build
	@for e in $(EXAMPLES); do \
	  $(call STRIP,$(OUT)/$$e,build/$$e.elf) && echo "build/$$e.elf"; \
	done

test: all
	$(DEPLOY) build/$(EXAMPLE).elf

# std-examples is its own workspace (own target and build-std set in its
# .cargo/config.toml), so cargo runs from there.
std:
	cd std-examples && cargo build --release $(addprefix -p ,$(STD_EXAMPLES))
	@mkdir -p build/std
	@for e in $(STD_EXAMPLES); do \
	  $(call STRIP,$(STD_OUT)/$$e,build/std/$$e.elf) && echo "build/std/$$e.elf $$(du -h build/std/$$e.elf | cut -f1)"; \
	done

std-test:
	cd std-examples && cargo build --release -p $(STD_EXAMPLE)
	@mkdir -p build/std
	@$(call STRIP,$(STD_OUT)/$(STD_EXAMPLE),build/std/$(STD_EXAMPLE).elf)
	$(DEPLOY) build/std/$(STD_EXAMPLE).elf

# Like std-test, but also forwards your keyboard to the payload's stdin
# (scripts/run.sh: returns as soon as the payload exits).
std-run:
	cd std-examples && cargo build --release -p $(STD_EXAMPLE)
	@mkdir -p build/std
	@$(call STRIP,$(STD_OUT)/$(STD_EXAMPLE),build/std/$(STD_EXAMPLE).elf)
	scripts/run.sh -h $(PS5_HOST) -p $(PS5_PORT) build/std/$(STD_EXAMPLE).elf

clean:
	cargo clean; cd std-examples && cargo clean; rm -rf build

.PHONY: all test std std-test std-run clean
