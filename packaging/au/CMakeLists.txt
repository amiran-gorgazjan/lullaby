cmake_minimum_required(VERSION 3.27)
project(LullabyAU LANGUAGES C CXX OBJC OBJCXX)

if(NOT APPLE)
    message(FATAL_ERROR "Audio Units can only be built on macOS")
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CLAP_WRAPPER_DOWNLOAD_DEPENDENCIES OFF)
set(CLAP_WRAPPER_COPY_AFTER_BUILD OFF)
set(CLAP_SDK_ROOT "${LULLABY_AU_DEPS}/clap")
set(AUDIOUNIT_SDK_ROOT "${LULLABY_AU_DEPS}/AudioUnitSDK")
add_subdirectory("${LULLABY_AU_DEPS}/clap-wrapper" clap-wrapper EXCLUDE_FROM_ALL)

add_library(lullaby_au MODULE)
target_add_auv2_wrapper(
    TARGET lullaby_au
    OUTPUT_NAME "Lullaby"
    BUNDLE_IDENTIFIER "audio.lullaby.resonance.au"
    BUNDLE_VERSION "${LULLABY_VERSION}"
    MANUFACTURER_NAME "Lullaby Audio"
    MANUFACTURER_CODE "Llby"
    SUBTYPE_CODE "Lulb"
    INSTRUMENT_TYPE "aufx"
    MACOS_EMBEDDED_CLAP_LOCATION "${LULLABY_CLAP}"
)

# Local lifecycle compatibility fixes are applied to a build-directory copy;
# the pinned upstream checkout stays unchanged. See docs/audio-unit.md.
get_target_property(wrapper_sources lullaby_au SOURCES)
list(REMOVE_ITEM wrapper_sources "${LULLABY_AU_DEPS}/clap-wrapper/src/wrapasauv2.cpp")
set_property(TARGET lullaby_au PROPERTY SOURCES "${wrapper_sources}")
target_sources(lullaby_au PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/wrapper-src/src/wrapasauv2.cpp")

# PRE_BUILD does not run on an up-to-date target with Makefiles. Relink when the
# Rust binary changes so incremental builds cannot embed an older DSP/editor.
set_property(TARGET lullaby_au APPEND PROPERTY LINK_DEPENDS
    "${LULLABY_CLAP}/Contents/MacOS/Lullaby")
set_property(TARGET lullaby_au-build-helper APPEND PROPERTY LINK_DEPENDS
    "${LULLABY_CLAP}/Contents/MacOS/Lullaby"
    "${CMAKE_CURRENT_BINARY_DIR}/lullaby_au-build-helper-output/auv2_infoplist_top")

add_executable(lullaby_au_smoke "../../scripts/macos_au_smoke.mm")
target_compile_options(lullaby_au_smoke PRIVATE -Wall -Wextra -Werror)
target_link_libraries(lullaby_au_smoke PRIVATE "-framework AudioToolbox" "-framework Cocoa")
