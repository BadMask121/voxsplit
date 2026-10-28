cmake_minimum_required(VERSION 3.25)

# Apple Silicon only, matching the engine's ONNX Runtime build (docs/01-architecture.md).
set(CMAKE_OSX_ARCHITECTURES "arm64" CACHE STRING "")
set(CMAKE_OSX_DEPLOYMENT_TARGET "14.0" CACHE STRING "")

project(VoxSplit VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

include(FetchContent)
# Pass -DFETCHCONTENT_SOURCE_DIR_JUCE=<path> to build against a local checkout.
FetchContent_Declare(JUCE
    GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
    GIT_TAG 9.0.4
    GIT_SHALLOW TRUE)
FetchContent_MakeAvailable(JUCE)

juce_add_plugin(VoxSplit
    COMPANY_NAME "VoxSplit"
    COMPANY_WEBSITE "https://github.com/BadMask121/voxsplit"
    BUNDLE_ID com.voxsplit.plugin
    PRODUCT_NAME "VoxSplit"
    DESCRIPTION "Offline lead/backing vocal separation"
    PLUGIN_MANUFACTURER_CODE Vxsp
    PLUGIN_CODE Vxs1
    FORMATS VST3 AU Standalone
    IS_SYNTH FALSE
    NEEDS_MIDI_INPUT FALSE
    NEEDS_MIDI_OUTPUT FALSE
    IS_MIDI_EFFECT FALSE
    EDITOR_WANTS_KEYBOARD_FOCUS TRUE
    VST3_CATEGORIES Fx Tools
    AU_MAIN_TYPE kAudioUnitType_Effect
    HARDENED_RUNTIME_ENABLED TRUE
    # scripts/build-plugin.sh embeds the engine + models, signs, then installs.
    COPY_PLUGIN_AFTER_BUILD FALSE)

target_sources(VoxSplit PRIVATE
    Source/PluginProcessor.cpp
    Source/PluginEditor.cpp
    Source/SeparationJob.cpp
    Source/DropZone.cpp
    Source/EngineLocator.cpp
    Source/StemPreview.cpp
    Source/Theme.cpp
    Source/Widgets.cpp)

target_compile_definitions(VoxSplit PUBLIC
    JUCE_WEB_BROWSER=0
    JUCE_USE_CURL=0
    JUCE_VST3_CAN_REPLACE_VST2=0
    JUCE_STANDALONE_FILTER_WINDOW_USE_KIOSK_MODE=0)

target_link_libraries(VoxSplit
    PRIVATE
        juce::juce_animation
        juce::juce_audio_utils
        juce::juce_gui_extra
    PUBLIC
        juce::juce_recommended_config_flags
        juce::juce_recommended_lto_flags
        juce::juce_recommended_warning_flags)

# Headless tests for the job/preview logic (no GUI, no host):
#   cmake --build build/cmake --target VoxSplitTests && build/cmake/VoxSplitTests_artefacts/Release/VoxSplitTests
option(VOXSPLIT_BUILD_TESTS "Build the plugin's unit tests" ON)
if(VOXSPLIT_BUILD_TESTS)
    juce_add_console_app(VoxSplitTests PRODUCT_NAME "VoxSplitTests")
    target_sources(VoxSplitTests PRIVATE
        Tests/PluginTests.cpp
        Source/SeparationJob.cpp
        Source/EngineLocator.cpp
        Source/StemPreview.cpp)
    target_compile_definitions(VoxSplitTests PRIVATE
        JUCE_WEB_BROWSER=0
        JUCE_USE_CURL=0
        VOXSPLIT_REPO_ROOT="${CMAKE_CURRENT_SOURCE_DIR}/..")
    target_link_libraries(VoxSplitTests PRIVATE
        juce::juce_audio_devices
        juce::juce_audio_formats
        juce::juce_recommended_config_flags
        juce::juce_recommended_warning_flags)
endif()
