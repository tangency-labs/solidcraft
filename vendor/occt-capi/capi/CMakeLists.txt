cmake_minimum_required(VERSION 3.21)
project(occt_capi LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)

find_package(OpenCASCADE REQUIRED CONFIG)

set(occt_libs TKDESTEP TKDEIGES TKXSBase TKXCAF TKCAF TKLCAF TKHLR TKFeat TKFillet TKBool TKBO
              TKPrim TKOffset TKMesh TKShHealing TKTopAlgo TKGeomAlgo TKBRep TKGeomBase TKG3d
              TKG2d TKMath TKernel)

# Each domain has its own file. Entry points start a line with `API `, and the wasm
# export list comes from that, so there is no second list to keep in step.
file(GLOB occt_capi_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp")
set(occt_capi_exports "")
foreach(source ${occt_capi_sources})
  set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${source}")
  file(STRINGS "${source}" api_lines REGEX "^API ")
  foreach(line ${api_lines})
    string(REGEX MATCH "(occt_[a-z0-9_]+)[(]" _ "${line}")
    list(APPEND occt_capi_exports "\"_${CMAKE_MATCH_1}\"")
  endforeach()
endforeach()
list(JOIN occt_capi_exports "," occt_capi_exports_json)
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/exports.json" "[${occt_capi_exports_json}]")

if(EMSCRIPTEN)
  add_executable(occt_capi ${occt_capi_sources})
  target_link_options(occt_capi PRIVATE
    -fwasm-exceptions -O3
    -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createOcctCapi
    -sENVIRONMENT=web,worker,node
    -sALLOW_MEMORY_GROWTH=1
    -sSTACK_SIZE=8MB
    -sNO_EXIT_RUNTIME=1
    "-sEXPORTED_RUNTIME_METHODS=['HEAPU8']"
    "-sEXPORTED_FUNCTIONS=@${CMAKE_CURRENT_BINARY_DIR}/exports.json")
  set_target_properties(occt_capi PROPERTIES SUFFIX ".mjs")
  install(TARGETS occt_capi RUNTIME DESTINATION lib)
  install(FILES "$<TARGET_FILE_DIR:occt_capi>/occt_capi.wasm" DESTINATION lib)
else()
  add_library(occt_capi SHARED ${occt_capi_sources})
  if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    # Only the C ABI leaves the module, no OCCT or libstdc++ symbol interposition.
    target_link_options(occt_capi PRIVATE -Wl,--exclude-libs,ALL -Wl,--no-undefined)
  endif()
  install(TARGETS occt_capi LIBRARY DESTINATION lib RUNTIME DESTINATION lib ARCHIVE DESTINATION lib)
endif()

target_compile_definitions(occt_capi PRIVATE OCCT_CAPI_BUILD)
target_include_directories(occt_capi PRIVATE "${OpenCASCADE_INCLUDE_DIR}")
target_link_libraries(occt_capi PRIVATE ${occt_libs})
