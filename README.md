# Server Testing
bin\server.exe
curl http://localhost:8080
powershell: Invoke-WebRequest -Method POST -Uri "http://localhost:8080/register-car" -Body "license=ABC123&start_x=0&start_y=0&dest_x=10&dest_y=20"

# Installation
vcpkg install libuv:x64-windows
vcpkg install raylib:x64-windows

# Map Generation

bin\city_map.exe generate 12345
bin\city_map.exe export 12345 hive_mind_server/city.json
bin\city_map.exe load hive_mind_server/city.json

All coordinates are expressed in meters.
Each [x, y] value represents a position in world space measured in meters.
City is centered at 0, 0

The origin (0,0) is arbitrary but must be shared between the simulation server and renderer.
1 unit = 1 meter

City Map JSON Format

{
  "segments": [
    {
      "id": 1,
      "pts": [
        [0.0, 0.0],
        [50.0, 0.0],
        [50.0, 20.0]
      ]
    }
  ]
}

segments: array of road segments  
id: integer segment identifier  
pts: array of [x, y] world-space points

```.vscode/c_cpp_properties.json
{
    "configurations": [
        {
            "name": "Win32",
            "includePath": [
                "${workspaceFolder}/**",
                "C:/vcpkg/installed/x64-windows/include"
            ],
            "defines": [],
            "compilerPath": "C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/*/bin/Hostx64/x64/cl.exe",
            "cStandard": "c17",
            "cppStandard": "c++17",
            "intelliSenseMode": "windows-msvc-x64"
        },
        {
            "name": "MinGW-GCC",
            "includePath": [
                "${workspaceFolder}/**",
                "C:/vcpkg/installed/x64-windows/include"
            ],
            "defines": [],
            "compilerPath": "C:/msys64/mingw64/bin/gcc.exe",
            "cStandard": "c11",
            "intelliSenseMode": "windows-gcc-x64"
        }
    ],
    "version": 4
}
```
