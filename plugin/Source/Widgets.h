#pragma once

#include <juce_gui_basics/juce_gui_basics.h>

#include "Theme.h"

namespace vox
{

/** Truncates in the middle ("my very lo…mix.wav") so file extensions stay visible. */
juce::String middleEllipsis (const juce::String& text, const juce::Font& font, float maxWidth);

/** A button that answers the press: scales to 0.97 while held and crossfades label changes. */
class PressableButton final : public juce::Button
{
public:
    enum class Style { primary, ghost, link };

    PressableButton (const juce::String& label, Style style);

    void setStyle (Style newStyle);
    void setLabel (const juce::String& newLabel);

private:
    void paintButton (juce::Graphics&, bool highlighted, bool down) override;
    void buttonStateChanged() override;
    void tick();

    Style style;
    juce::String label, previousLabel;
    Tween press, hover, swap;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};

/** Round play/pause toggle. The glyphs crossfade (never scale from zero) and the button answers presses. */
class PlayButton final : public juce::Button
{
public:
    explicit PlayButton (juce::Colour accent);
    void setPlaying (bool);

private:
    void paintButton (juce::Graphics&, bool highlighted, bool down) override;
    void buttonStateChanged() override;
    void tick();

    juce::Colour accent;
    bool playing = false;
    Tween press, hover, morph;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};

/** Two or more mutually exclusive options; the highlight slides to the selection. */
class SegmentedToggle final : public juce::Component
{
public:
    explicit SegmentedToggle (const juce::StringArray& labels);

    std::function<void (int)> onChange;

    void setSelected (int index, bool animate);
    int getSelected() const noexcept { return selected; }

private:
    void paint (juce::Graphics&) override;
    void mouseDown (const juce::MouseEvent&) override;
    void enablementChanged() override { repaint(); }
    juce::Rectangle<float> segmentBounds (float position) const;
    void tick();

    juce::StringArray labels;
    int selected = 0;
    Tween pill;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};

/** A discrete slider (e.g. 1-5). The thumb follows the pointer while dragging and
    snaps to the nearest tick on release; arrow keys step instantly. */
class PassSlider final : public juce::Component
{
public:
    PassSlider (int minValue, int maxValue);

    std::function<void (int)> onChange;

    void setValue (int value, bool animate);
    int getValue() const noexcept { return value; }

private:
    void paint (juce::Graphics&) override;
    void mouseDown (const juce::MouseEvent&) override;
    void mouseDrag (const juce::MouseEvent&) override;
    void mouseUp (const juce::MouseEvent&) override;
    void mouseEnter (const juce::MouseEvent&) override;
    void mouseExit (const juce::MouseEvent&) override;
    bool keyPressed (const juce::KeyPress&) override;
    void enablementChanged() override { repaint(); }

    float xFor (float position) const;
    float positionAt (float x) const;
    void dragTo (float x);
    void tick();

    int minValue, maxValue, value;
    float thumb = 0.0f; // continuous position in value units
    bool dragging = false;
    Tween settle, grow, hover;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};

/** Overall progress with a smoothed, linear-feeling fill and an indeterminate mode. */
class ProgressStrip final : public juce::Component
{
public:
    enum class Mode { indeterminate, determinate, done };

    void setState (Mode mode, float fraction, const juce::String& label, const juce::String& detail);

private:
    void paint (juce::Graphics&) override;
    void tick();

    Mode mode = Mode::indeterminate;
    float target = 0.0f, shown = 0.0f;
    double lastTick = 0.0;
    juce::String label, detail;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};

/** One finished stem: preview it, drag it into the DAW, or reveal it in Finder. */
class StemRow final : public juce::Component
{
public:
    StemRow (const juce::String& title, juce::Colour accent, const juce::File& file);

    std::function<void()> onTogglePreview;

    /** Fade and lift into place after `index` stagger steps. */
    void enter (int index);

    /** Called every frame by the editor with the preview's state for this row. */
    void setPlayback (bool playing, float progress);

    const juce::File& getFile() const noexcept { return file; }

private:
    void paint (juce::Graphics&) override;
    void resized() override;
    void mouseEnter (const juce::MouseEvent&) override;
    void mouseExit (const juce::MouseEvent&) override;
    void mouseDrag (const juce::MouseEvent&) override;
    void tick();

    juce::String title;
    juce::Colour accent;
    juce::File file;
    PlayButton play;
    PressableButton reveal { "Reveal", PressableButton::Style::ghost };
    Tween appear, hover, playingAmount;
    float playProgress = 0.0f;
    bool dragging = false;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};

} // namespace vox
