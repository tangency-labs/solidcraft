# solidcore C++ reference build of NVIDIA SMLib + SM_API (Apache-2.0).
#
# Mirrors the upstream premake files:
#   upstream/premake5.lua                     (workspace defines/flags, linux release)
#   upstream/source/SMLib/premake5.lua        (SMLib: src/*.cpp, inc/)
#   upstream/source/SM_API/premake5.lua       (SM_API: src/*.cpp, SM_API_EXPORTS)
#   upstream/tests/SM_API_test/premake5.lua   (SM_API_test_app, SM_API_TEST_STANDALONE)
#   upstream/tests/prog_test/premake5.lua     (prog_test + prog_test_app)
#
# Differences from upstream (see plan/baseline.md):
#   * Static libraries instead of shared ones (for linking into solidcore-ffi).
#   * TBB is not used: upstream only compiles TBB code under SM_USE_TBB /
#     SM_USE_TBB_SCALABLE, which no build file defines; the serial code paths
#     are the default. We therefore define neither and link no TBB.
#   * -O2 -ffp-contract=off (no FMA contraction) to reduce FP divergence vs Rust.
#   * -fPIC so the static libs can be linked into Rust test executables.
#   * No -Werror (upstream uses FatalCompileWarnings).
#   * SM_API_test: the two tests that need upstream/Examples (outside the
#     licence allow-list for this port) are replaced by stubs in stubs/.
#
# Do not invoke directly; use build.sh, which puts everything under bin/cpp.

cmake_minimum_required(VERSION 3.20)
project(solidcore_smlib_ref CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()
# Override CMake's default Release flags (-O3) with -O2, keep NDEBUG, add -g
# so the reference kernel can be stepped in a debugger.
set(CMAKE_CXX_FLAGS_RELEASE "-O2 -g -DNDEBUG")

set(SMLIB_UPSTREAM "" CACHE PATH "Path to the upstream SMLib checkout")
if(NOT SMLIB_UPSTREAM)
  get_filename_component(SMLIB_UPSTREAM "${CMAKE_CURRENT_SOURCE_DIR}/../../upstream" ABSOLUTE)
endif()
set(UP "${SMLIB_UPSTREAM}")
if(NOT EXISTS "${UP}/source/SMLib/inc/SmSmlibAll.h")
  message(FATAL_ERROR "Upstream SMLib not found at ${UP}")
endif()

option(SC_BUILD_TESTS "Build SM_API_test_app and prog_test_app" ON)

# Workspace-level settings from upstream/premake5.lua (linux, release).
add_compile_definitions(NDEBUG NOMINMAX SM_GRAPHICS_CALLBACKS=1 _GLIBCXX_USE_CXX11_ABI=1)
add_compile_options(-ffp-contract=off -fno-fast-math -fexceptions -frtti -w)

# ---------------------------------------------------------------- SMLib
file(GLOB SMLIB_SRC CONFIGURE_DEPENDS "${UP}/source/SMLib/src/*.cpp")
add_library(SMLib STATIC ${SMLIB_SRC})
target_include_directories(SMLib PUBLIC "${UP}/source/SMLib/inc")
set_target_properties(SMLib PROPERTIES OUTPUT_NAME SMLib)

# ---------------------------------------------------------------- SM_API
file(GLOB SMAPI_SRC CONFIGURE_DEPENDS "${UP}/source/SM_API/src/*.cpp")
add_library(SM_API STATIC ${SMAPI_SRC})
target_include_directories(SM_API BEFORE PUBLIC "${UP}/source/SM_API/inc")
target_compile_definitions(SM_API PRIVATE SM_API_EXPORTS)
target_link_libraries(SM_API PUBLIC SMLib)
set_target_properties(SM_API PROPERTIES OUTPUT_NAME SM_API)

if(SC_BUILD_TESTS)
  # -------------------------------------------------------------- SM_API_test_app
  file(GLOB SMAPITEST_SRC CONFIGURE_DEPENDS "${UP}/tests/SM_API_test/src/*.cpp")
  # These two need upstream/Examples/*; replaced by stubs/sm_api_test_examples_stub.cpp.
  list(FILTER SMAPITEST_SRC EXCLUDE REGEX "TestSmBooleanBoundsExample\\.cpp$")
  list(FILTER SMAPITEST_SRC EXCLUDE REGEX "TestSmSTEPDerivativesExample\\.cpp$")
  add_executable(SM_API_test_app ${SMAPITEST_SRC}
    "${CMAKE_CURRENT_SOURCE_DIR}/stubs/sm_api_test_examples_stub.cpp")
  target_include_directories(SM_API_test_app BEFORE PRIVATE "${UP}/tests/SM_API_test/inc")
  target_compile_definitions(SM_API_test_app PRIVATE SM_API_TEST_STANDALONE)
  target_link_libraries(SM_API_test_app PRIVATE SM_API)

  # -------------------------------------------------------------- prog_test_app
  file(GLOB PROGTEST_SRC CONFIGURE_DEPENDS "${UP}/tests/prog_test/src/*.cpp")
  add_executable(prog_test_app ${PROGTEST_SRC})
  target_include_directories(prog_test_app BEFORE PRIVATE "${UP}/tests/prog_test/inc")
  target_link_libraries(prog_test_app PRIVATE SMLib)
endif()

install(TARGETS SMLib SM_API ARCHIVE DESTINATION lib)
if(SC_BUILD_TESTS)
  install(TARGETS SM_API_test_app prog_test_app RUNTIME DESTINATION tests)
endif()
