#pragma once

#include <juce_gui_basics/juce_gui_basics.h>

// Shared look and motion for the plugin UI. The SwiftUI app mirrors these
// values (macos/Sources/VoxSplit/Theme.swift) so both frontends feel the same.
namespace vox
{
namespace colours
{
    inline const juce::Colour background { 0xff121214 };
    inline const juce::Colour surface    { 0xff1a1a1e };
    inline const juce::Colour surfaceHi  { 0xff24242a };
    inline const juce::Colour border     { 0xff2e2e35 };
    inline const juce::Colour text       { 0xfff2f2f4 };
    inline const juce::Colour textDim    { 0xff9b9ba4 };
    inline const juce::Colour lead       { 0xffff8a5b };
    inline const juce::Colour backing    { 0xff7c9cff };
    inline const juce::Colour instrumental { 0xff4fd1c5 };
    inline const juce::Colour success    { 0xff4cd38a };
    inline const juce::Colour danger     { 0xffff6359 };
}

namespace motion
{
    // Durations in ms. UI motion stays under 300 ms; exits are faster than entrances.
    constexpr double press   = 100.0;
    constexpr double release = 160.0;
    constexpr double hover   = 150.0;
    constexpr double hoverOut = 110.0;
    constexpr double enter   = 220.0;
    constexpr double stagger = 60.0;
    constexpr double fade    = 180.0;
}

/** cubic-bezier(0.23, 1, 0.32, 1): a strong ease-out for anything entering or responding. */
float easeOut (float t);

/** cubic-bezier(0.77, 0, 0.175, 1): for elements moving between two on-screen positions. */
float easeInOut (float t);

/** macOS "Reduce motion": keep opacity changes, drop movement and scaling. */
bool prefersReducedMotion();

double nowMs();

/** A single animated float. Call tick() once per frame; it returns true while moving. */
struct Tween
{
    float value = 0.0f;

    void animateTo (float target, double durationMs, float (*ease) (float) = easeOut, double delayMs = 0.0)
    {
        if (juce::approximatelyEqual (target, to) && (running || juce::approximatelyEqual (value, target)))
            return;

        from = value;
        to = target;
        start = nowMs() + delayMs;
        duration = juce::jmax (1.0, durationMs);
        easing = ease;
        running = true;
    }

    void snap (float v)
    {
        value = from = to = v;
        running = false;
    }

    bool tick (double now)
    {
        if (! running)
            return false;

        const auto t = (float) juce::jlimit (0.0, 1.0, (now - start) / duration);
        value = from + (to - from) * easing (t);

        if (t >= 1.0f)
        {
            value = to;
            running = false;
        }

        return true;
    }

    float target() const noexcept { return to; }
    bool isRunning() const noexcept { return running; }

private:
    float from = 0.0f, to = 0.0f;
    double start = 0.0, duration = 1.0;
    float (*easing) (float) = easeOut;
    bool running = false;
};

juce::Font font (float height, bool bold = false);
juce::Font monoFont (float height);

/** File types the engine can decode. */
bool isSupportedAudioFile (const juce::File& f);
juce::String supportedAudioWildcard();

} // namespace vox
