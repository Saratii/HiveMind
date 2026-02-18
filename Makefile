VCPKG_ROOT = C:/vcpkg

BIN_DIR = bin

CITY_MAP_TARGET = $(BIN_DIR)/city_map.exe
CITY_MAP_SRC = hive_mind_server/city_map.c hive_mind_server/city_constants.c

SERVER_TARGET = $(BIN_DIR)/server.exe
SERVER_SRC = hive_mind_server/server.c hive_mind_server/car.c

CFLAGS = -O2 -std=c11 -I$(VCPKG_ROOT)/installed/x64-windows/include
LDFLAGS = -L$(VCPKG_ROOT)/installed/x64-windows/lib \
          -luv \
          -lraylib -lopengl32 -lgdi32 -lwinmm -lm

DLL_DIR = $(VCPKG_ROOT)/installed/x64-windows/bin

all: $(CITY_MAP_TARGET) $(SERVER_TARGET)

$(CITY_MAP_TARGET): $(CITY_MAP_SRC)
	if not exist $(BIN_DIR) mkdir $(BIN_DIR)
	gcc $(CFLAGS) $(CITY_MAP_SRC) -o $(CITY_MAP_TARGET) $(LDFLAGS)
	copy "$(DLL_DIR)\*.dll" "$(BIN_DIR)\"

$(SERVER_TARGET): $(SERVER_SRC)
	if not exist $(BIN_DIR) mkdir $(BIN_DIR)
	gcc $(CFLAGS) $(SERVER_SRC) -o $(SERVER_TARGET) $(LDFLAGS)
	copy "$(DLL_DIR)\*.dll" "$(BIN_DIR)\"

clean:
	del /Q "$(BIN_DIR)\city_map.exe" "$(BIN_DIR)\server.exe" "$(BIN_DIR)\*.dll" 2>NUL
