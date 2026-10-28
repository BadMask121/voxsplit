#pragma once

#include <juce_core/juce_core.h>

/** Where the engine binary and its models live (docs/01-architecture.md). */
struct EngineLocation
{
    juce::File engine, modelsDir;
    juce::String problem; // empty when everything was found

    bool isValid() const { return problem.isEmpty(); }
};

/** Looks inside this plugin's own bundle first, then in
    ~/Library/Application Support/VoxSplit (dev builds / shared installs). */
EngineLocation locateEngine();
