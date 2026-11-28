# Builds OCCT (static, modelling + STEP only) and the occt_capi module on top of it.
#
#   cmake -DOUT_DIR=<dir> [-DOCCT_SOURCE_DIR=<dir>] [-DWASM=ON]
#         [-DTOOLCHAIN=<toolchain file>] -P build.cmake
#
# Output: <OUT_DIR>/lib, a single shared library (native) or occt_capi.{mjs,wasm} (WASM=ON).
# This is the only build description; build.rs and the wasm instructions both call it.
cmake_minimum_required(VERSION 3.21)

set(OCCT_VERSION "8.0.1")
set(OCCT_URL "https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/V${OCCT_VERSION}.tar.gz")
set(OCCT_SHA256 "6297cc55a1720523a437c54d07dc16b9da8c5f5ab5800da3c6ebded568d53c18")

if(NOT OUT_DIR)
  message(FATAL_ERROR "OUT_DIR is required")
endif()
get_filename_component(HERE "${CMAKE_SCRIPT_MODE_FILE}" DIRECTORY)

if(NOT OCCT_SOURCE_DIR)
  set(tarball "${OUT_DIR}/occt-${OCCT_VERSION}.tar.gz")
  set(OCCT_SOURCE_DIR "${OUT_DIR}/occt-src/OCCT-${OCCT_VERSION}")
  if(NOT EXISTS "${OCCT_SOURCE_DIR}/CMakeLists.txt")
    file(DOWNLOAD "${OCCT_URL}" "${tarball}" EXPECTED_HASH SHA256=${OCCT_SHA256} SHOW_PROGRESS)
    file(ARCHIVE_EXTRACT INPUT "${tarball}" DESTINATION "${OUT_DIR}/occt-src")
  endif()
endif()

set(common -DCMAKE_BUILD_TYPE=Release -DCMAKE_POSITION_INDEPENDENT_CODE=ON)
if(TOOLCHAIN)
  list(APPEND common "-DCMAKE_TOOLCHAIN_FILE=${TOOLCHAIN}")
endif()
if(WASM)
  # OCCT defines OCC_CONVERT_SIGNALS to turn signals into exceptions through setjmp. Wasm has
  # no signals, and with those setjmps LLVM (emsdk 6.0.13) emits an invalid module. CMake puts
  # these flags after OCCT's definitions, so the -U wins.
  set(wasm_flags "-fwasm-exceptions -UOCC_CONVERT_SIGNALS")
  list(APPEND common "-DCMAKE_CXX_FLAGS=${wasm_flags}" "-DCMAKE_C_FLAGS=${wasm_flags}")
endif()

# OCCT translation units are heavy: at one job per core a build can exhaust RAM and
# trigger the OOM killer. Allow one job per 3 GiB available unless OCCT_BUILD_JOBS is set.
if(DEFINED ENV{OCCT_BUILD_JOBS})
  set(jobs "$ENV{OCCT_BUILD_JOBS}")
else()
  cmake_host_system_information(RESULT cores QUERY NUMBER_OF_LOGICAL_CORES)
  cmake_host_system_information(RESULT available_mb QUERY AVAILABLE_PHYSICAL_MEMORY)
  math(EXPR jobs "${available_mb} / 3072")
  if(jobs GREATER cores)
    set(jobs ${cores})
  endif()
  if(jobs LESS 1)
    set(jobs 1)
  endif()
endif()
message(STATUS "Building with ${jobs} parallel jobs")

function(run)
  execute_process(COMMAND ${ARGN} COMMAND_ERROR_IS_FATAL ANY)
endfunction()

set(occt_prefix "${OUT_DIR}/occt")
# Bump when the toolkit list or OCCT options change, so existing builds are refreshed.
set(occt_build_id "2")
set(stamp "${occt_prefix}/.complete-${occt_build_id}")
if(NOT EXISTS "${stamp}")
  run("${CMAKE_COMMAND}" -S "${OCCT_SOURCE_DIR}" -B "${OUT_DIR}/occt-build" ${common}
      "-DCMAKE_INSTALL_PREFIX=${occt_prefix}"
      -DINSTALL_DIR_LAYOUT=Unix
      -DBUILD_LIBRARY_TYPE=Static
      -DBUILD_MODULE_ApplicationFramework=OFF
      -DBUILD_MODULE_DataExchange=OFF
      -DBUILD_MODULE_Draw=OFF
      -DBUILD_MODULE_Visualization=OFF
      "-DBUILD_ADDITIONAL_TOOLKITS=TKDESTEP TKDEIGES TKHLR TKFeat TKOffset TKShHealing"
      # Keep OCCT's argument checks: the shim turns them into errors. Without
      # them bad input is undefined behaviour instead of an Err.
      -DBUILD_RELEASE_DISABLE_EXCEPTIONS=OFF
      -DBUILD_DOC_Overview=OFF
      -DUSE_FREETYPE=OFF -DUSE_TK=OFF -DUSE_XLIB=OFF -DUSE_OPENGL=OFF -DUSE_GLES2=OFF
      -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF -DUSE_TBB=OFF -DUSE_VTK=OFF)
  run("${CMAKE_COMMAND}" --build "${OUT_DIR}/occt-build" --config Release --parallel ${jobs})
  run("${CMAKE_COMMAND}" --install "${OUT_DIR}/occt-build" --config Release)
  file(TOUCH "${stamp}")
endif()

run("${CMAKE_COMMAND}" -S "${HERE}" -B "${OUT_DIR}/capi-build" ${common}
    "-DCMAKE_INSTALL_PREFIX=${OUT_DIR}"
    "-DOpenCASCADE_DIR=${occt_prefix}/lib/cmake/opencascade")
run("${CMAKE_COMMAND}" --build "${OUT_DIR}/capi-build" --config Release --parallel ${jobs})
run("${CMAKE_COMMAND}" --install "${OUT_DIR}/capi-build" --config Release)
