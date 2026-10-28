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

void drawMark (juce::Graphics& g, juce::Rectangle<float> area, bool tile, float alpha)
{
    // The SVG's own coordinates: the tile spans 100..924; the bars x 194..830, y 244..762.
    const auto box = tile ? juce::Rectangle<float> (100.0f, 100.0f, 824.0f, 824.0f)
                          : juce::Rectangle<float> (194.0f, 244.0f, 636.0f, 518.0f);
    const auto scale = juce::jmin (area.getWidth() / box.getWidth(), area.getHeight() / box.getHeight());
    const auto origin = area.getCentre() - box.getCentre() * scale;
    const auto map = [&] (float x, float y, float w, float h)
    {
        return juce::Rectangle<float> (origin.x + x * scale, origin.y + y * scale, w * scale, h * scale);
    };
    const auto vertical = [&] (juce::uint32 top, juce::uint32 bottom, juce::Rectangle<float> r)
    {
        return juce::ColourGradient (juce::Colour (top).withMultipliedAlpha (alpha), r.getX(), r.getY(),
                                     juce::Colour (bottom).withMultipliedAlpha (alpha), r.getX(), r.getBottom(), false);
    };

    if (tile)
    {
        const auto r = map (100.0f, 100.0f, 824.0f, 824.0f);
        g.setGradientFill (vertical (0xff2c2c34, 0xff121215, r));
        g.fillRoundedRectangle (r, 185.0f * scale);
        g.setColour (juce::Colours::white.withAlpha (0.09f * alpha));
        g.drawRoundedRectangle (r.reduced (0.5f), 185.0f * scale, 1.0f);
    }

    constexpr float heights[] = { 80, 150, 220, 270, 300, 250, 190, 130, 80 };
    constexpr float width = 44.0f, step = 74.0f, centre = 560.0f, split = 16.0f;
    for (int i = 0; i < 9; ++i)
    {
        const auto x = 194.0f + (float) i * step;
        const auto lead = map (x, centre - split - heights[i], width, heights[i]);
        g.setGradientFill (vertical (0xffffb892, 0xffff7a45, lead));
        g.fillRoundedRectangle (lead, lead.getWidth() / 2.0f);
        // Backing is quieter: 62% of the lead's height, as on the icon.
        const auto backing = map (x, centre + split, width, std::round (heights[i] * 0.62f));
        g.setGradientFill (vertical (0xff6c8cff, 0xffafc0ff, backing));
        g.fillRoundedRectangle (backing, backing.getWidth() / 2.0f);
    }
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
