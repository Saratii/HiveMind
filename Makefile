# Mac-friendly Makefile (vehicle emulator / car node)
# Usage:
#   make car_node
#   make clean

BIN_DIR := bin

CAR_NODE_TARGET := $(BIN_DIR)/car_node
CAR_NODE_SRC := vehicle_emulator/cars_node.cpp vehicle_emulator/cars.c

# Homebrew prefixes differ between Apple Silicon and Intel
BREW_PREFIX := $(shell brew --prefix 2>/dev/null)
CFLAGS := -O2 -std=c++17 -I$(BREW_PREFIX)/include
LDFLAGS := -L$(BREW_PREFIX)/lib -luv -lm

.PHONY: all car_node clean

all: car_node

car_node: $(CAR_NODE_TARGET)

$(CAR_NODE_TARGET): $(CAR_NODE_SRC)
	@mkdir -p $(BIN_DIR)
	c++ $(CFLAGS) $(CAR_NODE_SRC) -o $(CAR_NODE_TARGET) $(LDFLAGS)

clean:
	rm -f $(CAR_NODE_TARGET)
