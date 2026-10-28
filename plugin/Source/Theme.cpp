#include "Theme.h"

#include <juce_animation/juce_animation.h>

#if JUCE_MAC
 #include <CoreFoundation/CoreFoundation.h>
#endif

namespace vox
{
namespace
{
    const auto easeOutCurve   = juce::Easings::createCubicBezier (0.23f, 1.0f, 0.32f, 1.0f);
    const auto easeInOutCurve = juce::Easings::createCubicBezier (0.77f, 0.0f, 0.175f, 1.0f);
}

float easeOut (float t)   { return easeOutCurve (t); }
float easeInOut (float t) { return easeInOutCurve (t); }

bool prefersReducedMotion()
{
   #if JUCE_MAC
    // NSWorkspace.accessibilityDisplayShouldReduceMotion without Objective-C.
    Boolean valid = false;
    const auto reduce = CFPreferencesGetAppBooleanValue (CFSTR ("reduceMotion"),
                                                        CFSTR ("com.apple.universalaccess"),
                                                        &valid);
    return valid && reduce;
   #else
    return false;
   #endif
}

double nowMs()
{
    return juce::Time::getMillisecondCounterHiRes();
}

juce::Font font (float height, bool bold)
{
    return juce::Font (juce::FontOptions (height, bold ? juce::Font::bold : juce::Font::plain));
}

juce::Font monoFont (float height)
{
    return juce::Font (juce::FontOptions (juce::Font::getDefaultMonospacedFontName(), height, juce::Font::plain));
}

static const juce::StringArray& supportedExtensions()
{
    static const juce::StringArray exts { "wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "aac", "ogg" };
    return exts;
}

bool isSupportedAudioFile (const juce::File& f)
{
    return f.existsAsFile()
        && supportedExtensions().contains (f.getFileExtension().trimCharactersAtStart (".").toLowerCase());
}

juce::String supportedAudioWildcard()
{
    juce::StringArray patterns;
    for (auto& e : supportedExtensions())
        patterns.add ("*." + e);
    return patterns.joinIntoString (";");
}

} // namespace vox
