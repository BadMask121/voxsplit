#include "EngineLocator.h"

namespace
{
   #if JUCE_WINDOWS
    constexpr const char* engineName = "voxsplit-engine.exe";
   #else
    constexpr const char* engineName = "voxsplit-engine";
   #endif

    // Every model the engine may load, for either quality.
    constexpr const char* requiredModels[] = { "Kim_Vocal_2", "UVR_MDXNET_KARA_2", "BS_Roformer_Karaoke" };

    struct Candidate
    {
        juce::File engine, modelsDir;
    };

    juce::Array<Candidate> candidates()
    {
        juce::Array<Candidate> list;

        // For a plugin this is the plugin binary itself, not the host:
        //   macOS:   VoxSplit.vst3/Contents/MacOS/VoxSplit
        //   Windows: VoxSplit.vst3/Contents/x86_64-win/VoxSplit.vst3
        const auto binary = juce::File::getSpecialLocation (juce::File::currentExecutableFile);
        const auto contents = binary.getParentDirectory().getParentDirectory();

       #if JUCE_WINDOWS
        list.add ({ binary.getSiblingFile (engineName), contents.getChildFile ("Resources/models") });
       #else
        list.add ({ contents.getChildFile ("Helpers").getChildFile (engineName),
                    contents.getChildFile ("Resources/models") });
       #endif

        // Shared installs: per user (`install.sh --cli`), then system-wide (the .pkg).
        for (auto domain : { juce::File::userApplicationDataDirectory, juce::File::commonApplicationDataDirectory })
        {
            const auto shared = juce::File::getSpecialLocation (domain).getChildFile ("Application Support/VoxSplit");
            list.add ({ shared.getChildFile (engineName), shared.getChildFile ("models") });
        }
        return list;
    }
}

EngineLocation locateEngine()
{
    for (const auto& c : candidates())
    {
        if (! c.engine.existsAsFile())
            continue;

        EngineLocation loc { c.engine, c.modelsDir, {} };
        for (auto* name : requiredModels)
            if (! c.modelsDir.getChildFile (juce::String (name) + ".onnx").existsAsFile()
                || ! c.modelsDir.getChildFile (juce::String (name) + ".json").existsAsFile())
                loc.problem = "VoxSplit's model files are missing from " + c.modelsDir.getFullPathName()
                            + ". Reinstall VoxSplit from voxsplit.jeffrey.build.";
        return loc;
    }

    EngineLocation missing;
    missing.problem = "VoxSplit's engine isn't installed. Reinstall VoxSplit from voxsplit.jeffrey.build.";
    return missing;
}
