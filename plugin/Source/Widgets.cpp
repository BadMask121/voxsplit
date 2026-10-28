#include "Widgets.h"

namespace vox
{

juce::String middleEllipsis (const juce::String& text, const juce::Font& font, float maxWidth)
{
    const auto width = [&] (const juce::String& s) { return juce::GlyphArrangement::getStringWidth (font, s); };

    if (width (text) <= maxWidth)
        return text;

    int keep = text.length();
    juce::String candidate;
    while (keep > 2)
    {
        --keep;
        const int head = (keep + 1) / 2, tail = keep / 2;
        candidate = text.substring (0, head) + juce::String::fromUTF8 ("\xe2\x80\xa6") + text.substring (text.length() - tail);
        if (width (candidate) <= maxWidth)
            return candidate;
    }
    return candidate;
}

//==============================================================================
PressableButton::PressableButton (const juce::String& labelIn, Style s)
    : juce::Button (labelIn), style (s), label (labelIn)
{
    swap.snap (1.0f);
    setMouseCursor (juce::MouseCursor::PointingHandCursor);
    setWantsKeyboardFocus (false);
    setTitle (labelIn);
}

void PressableButton::setStyle (Style newStyle)
{
    if (style != newStyle)
    {
        style = newStyle;
        repaint();
    }
}

void PressableButton::setLabel (const juce::String& newLabel)
{
    if (newLabel == label)
        return;

    previousLabel = label;
    label = newLabel;
    setTitle (newLabel);
    swap.snap (0.0f);
    swap.animateTo (1.0f, motion::fade);
    repaint();
}

void PressableButton::buttonStateChanged()
{
    const bool down = isDown() && isEnabled();
    press.animateTo (down ? 1.0f : 0.0f, down ? motion::press : motion::release);

    const bool over = isOver() && isEnabled();
    hover.animateTo (over ? 1.0f : 0.0f, over ? motion::hover : motion::hoverOut);
}

void PressableButton::tick()
{
    const auto now = nowMs();
    const bool a = press.tick (now), b = hover.tick (now), c = swap.tick (now);
    if (a || b || c)
        repaint();
}

void PressableButton::paintButton (juce::Graphics& g, bool, bool)
{
    auto bounds = getLocalBounds().toFloat().reduced (0.5f);

    if (! prefersReducedMotion())
    {
        const auto scale = 1.0f - 0.03f * press.value;
        g.addTransform (juce::AffineTransform::scale (scale, scale, bounds.getCentreX(), bounds.getCentreY()));
    }

    float alpha = isEnabled() ? 1.0f : 0.35f;
    juce::Colour fill, outline, ink;

    switch (style)
    {
        case Style::primary:
            if (isEnabled())
            {
                fill = colours::text.interpolatedWith (juce::Colours::white, hover.value);
                ink = colours::background;
            }
            else
            {
                // A washed-out white reads as broken; disabled is a quiet surface instead.
                fill = colours::surfaceHi;
                ink = colours::textDim.withAlpha (0.55f);
                alpha = 1.0f;
            }
            break;
        case Style::ghost:
            fill = colours::surfaceHi.withAlpha (hover.value);
            outline = colours::border.interpolatedWith (colours::textDim, 0.35f * hover.value);
            ink = colours::textDim.interpolatedWith (colours::text, hover.value);
            break;
        case Style::link:
            ink = colours::textDim.interpolatedWith (colours::text, hover.value);
            break;
    }

    if (! fill.isTransparent())
    {
        g.setColour (fill.withMultipliedAlpha (alpha));
        g.fillRoundedRectangle (bounds, 8.0f);
    }
    if (! outline.isTransparent())
    {
        g.setColour (outline.withMultipliedAlpha (alpha));
        g.drawRoundedRectangle (bounds, 8.0f, 1.0f);
    }

    g.setFont (font (style == Style::link ? 12.5f : 13.5f, style == Style::primary));
    const auto justification = style == Style::link ? juce::Justification::centredLeft : juce::Justification::centred;
    const auto shift = prefersReducedMotion() ? 0.0f : 4.0f;
    const auto textArea = getLocalBounds().toFloat();

    if (swap.isRunning())
    {
        g.setColour (ink.withMultipliedAlpha (alpha * (1.0f - swap.value)));
        g.drawText (previousLabel, textArea.translated (0.0f, -shift * swap.value), justification);
    }
    g.setColour (ink.withMultipliedAlpha (alpha * swap.value));
    g.drawText (label, textArea.translated (0.0f, shift * (1.0f - swap.value)), justification);
}

//==============================================================================
PlayButton::PlayButton (juce::Colour accentIn) : juce::Button ("Preview"), accent (accentIn)
{
    setMouseCursor (juce::MouseCursor::PointingHandCursor);
    setWantsKeyboardFocus (false);
    setTitle ("Preview");
}

void PlayButton::setPlaying (bool shouldPlay)
{
    if (playing == shouldPlay)
        return;

    playing = shouldPlay;
    setTitle (playing ? "Stop preview" : "Preview");
    morph.animateTo (playing ? 1.0f : 0.0f, motion::fade);
}

void PlayButton::buttonStateChanged()
{
    const bool down = isDown() && isEnabled();
    press.animateTo (down ? 1.0f : 0.0f, down ? motion::press : motion::release);
    const bool over = isOver() && isEnabled();
    hover.animateTo (over ? 1.0f : 0.0f, over ? motion::hover : motion::hoverOut);
}

void PlayButton::tick()
{
    const auto now = nowMs();
    const bool a = press.tick (now), b = hover.tick (now), c = morph.tick (now);
    if (a || b || c)
        repaint();
}

void PlayButton::paintButton (juce::Graphics& g, bool, bool)
{
    auto bounds = getLocalBounds().toFloat().reduced (0.5f);
    const bool reduce = prefersReducedMotion();

    if (! reduce)
    {
        const auto scale = 1.0f - 0.04f * press.value;
        g.addTransform (juce::AffineTransform::scale (scale, scale, bounds.getCentreX(), bounds.getCentreY()));
    }

    auto circle = bounds.withSizeKeepingCentre (bounds.getHeight(), bounds.getHeight());
    g.setColour (colours::surfaceHi.interpolatedWith (accent, 0.18f * morph.value + 0.08f * hover.value));
    g.fillEllipse (circle);
    g.setColour (colours::border.interpolatedWith (accent, 0.6f * morph.value + 0.3f * hover.value));
    g.drawEllipse (circle.reduced (0.5f), 1.0f);

    const auto c = circle.getCentre();
    const auto glyphScale = [&] (float visible) { return reduce ? 1.0f : 0.8f + 0.2f * visible; };

    // Play: a triangle, optically nudged right of centre.
    {
        const auto v = 1.0f - morph.value;
        juce::Path tri;
        tri.addTriangle (c.x - 3.5f, c.y - 5.5f, c.x - 3.5f, c.y + 5.5f, c.x + 5.5f, c.y);
        tri.applyTransform (juce::AffineTransform::scale (glyphScale (v), glyphScale (v), c.x, c.y));
        g.setColour (colours::text.withAlpha (v));
        g.fillPath (tri);
    }
    // Pause: two bars.
    {
        const auto v = morph.value;
        juce::Path bars;
        bars.addRoundedRectangle (c.x - 4.5f, c.y - 5.5f, 3.0f, 11.0f, 1.0f);
        bars.addRoundedRectangle (c.x + 1.5f, c.y - 5.5f, 3.0f, 11.0f, 1.0f);
        bars.applyTransform (juce::AffineTransform::scale (glyphScale (v), glyphScale (v), c.x, c.y));
        g.setColour (colours::text.withAlpha (v));
        g.fillPath (bars);
    }
}

//==============================================================================
SegmentedToggle::SegmentedToggle (const juce::StringArray& labelsIn) : labels (labelsIn)
{
    setMouseCursor (juce::MouseCursor::PointingHandCursor);
    setTitle ("Quality");
}

void SegmentedToggle::setSelected (int index, bool animate)
{
    index = juce::jlimit (0, labels.size() - 1, index);
    selected = index;
    // The highlight moves between two on-screen positions: ease-in-out, not ease-out.
    if (animate && ! prefersReducedMotion())
        pill.animateTo ((float) index, 200.0, easeInOut);
    else
        pill.snap ((float) index);
    setDescription (labels[index]);
    repaint();
}

void SegmentedToggle::mouseDown (const juce::MouseEvent& e)
{
    if (! isEnabled())
        return;
    const auto index = juce::jlimit (0, labels.size() - 1, (int) (e.position.x / ((float) getWidth() / (float) labels.size())));
    if (index == selected)
        return;
    setSelected (index, true);
    if (onChange)
        onChange (index);
}

juce::Rectangle<float> SegmentedToggle::segmentBounds (float position) const
{
    const auto inner = getLocalBounds().toFloat().reduced (3.0f);
    const auto width = inner.getWidth() / (float) labels.size();
    return { inner.getX() + width * position, inner.getY(), width, inner.getHeight() };
}

void SegmentedToggle::tick()
{
    if (pill.tick (nowMs()))
        repaint();
}

void SegmentedToggle::paint (juce::Graphics& g)
{
    const float alpha = isEnabled() ? 1.0f : 0.45f;
    auto bounds = getLocalBounds().toFloat().reduced (0.5f);
    g.setColour (colours::surface.withMultipliedAlpha (alpha));
    g.fillRoundedRectangle (bounds, 8.0f);
    g.setColour (colours::border.withMultipliedAlpha (alpha));
    g.drawRoundedRectangle (bounds, 8.0f, 1.0f);

    g.setColour (colours::surfaceHi.brighter (0.15f).withMultipliedAlpha (alpha));
    g.fillRoundedRectangle (segmentBounds (pill.value), 6.0f);

    g.setFont (font (12.5f, true));
    for (int i = 0; i < labels.size(); ++i)
    {
        // Text brightens as the highlight arrives under it.
        const auto closeness = juce::jlimit (0.0f, 1.0f, 1.0f - std::abs (pill.value - (float) i));
        g.setColour (colours::textDim.interpolatedWith (colours::text, closeness).withMultipliedAlpha (alpha));
        g.drawText (labels[i], segmentBounds ((float) i), juce::Justification::centred);
    }
}

//==============================================================================
void ProgressStrip::setState (Mode newMode, float fraction, const juce::String& newLabel, const juce::String& newDetail)
{
    // A new run starts from zero; otherwise progress only ever moves forward.
    if (fraction < target - 0.001f)
        shown = fraction;

    mode = newMode;
    target = juce::jlimit (0.0f, 1.0f, fraction);
    label = newLabel;
    detail = newDetail;
    repaint();
}

void ProgressStrip::tick()
{
    const auto now = nowMs();
    const auto dt = juce::jlimit (0.0, 100.0, now - lastTick);
    lastTick = now;

    if (mode == Mode::indeterminate && isShowing())
    {
        repaint();
        return;
    }

    // Engine updates arrive per chunk; glide between them (time constant 140 ms).
    if (std::abs (target - shown) > 0.0005f)
    {
        shown += (target - shown) * (float) (1.0 - std::exp (-dt / 140.0));
        repaint();
    }
}

void ProgressStrip::paint (juce::Graphics& g)
{
    auto area = getLocalBounds().toFloat();
    auto text = area.removeFromTop (18.0f);

    g.setFont (font (13.0f, true));
    g.setColour (mode == Mode::done ? colours::success : colours::text);
    g.drawText (label, text, juce::Justification::centredLeft, true);

    g.setFont (font (12.5f));
    g.setColour (colours::textDim);
    g.drawText (detail, text, juce::Justification::centredRight, true);

    auto track = area.removeFromBottom (4.0f);
    g.setColour (colours::surfaceHi);
    g.fillRoundedRectangle (track, 2.0f);

    if (mode == Mode::indeterminate)
    {
        if (prefersReducedMotion())
        {
            g.setColour (colours::textDim.withAlpha (0.5f));
            g.fillRoundedRectangle (track.withWidth (track.getWidth() * 0.3f), 2.0f);
            return;
        }

        // Constant motion: linear, one sweep every 1.2 s.
        const auto phase = (float) std::fmod (nowMs() / 1200.0, 1.0);
        const auto segment = track.getWidth() * 0.28f;
        const auto x = track.getX() - segment + (track.getWidth() + segment) * phase;
        g.saveState();
        g.reduceClipRegion (track.toNearestInt());
        g.setColour (colours::text.withAlpha (0.75f));
        g.fillRoundedRectangle ({ x, track.getY(), segment, track.getHeight() }, 2.0f);
        g.restoreState();
        return;
    }

    g.setColour (mode == Mode::done ? colours::success : colours::text);
    g.fillRoundedRectangle (track.withWidth (juce::jmax (track.getHeight(), track.getWidth() * shown)), 2.0f);
}

//==============================================================================
StemRow::StemRow (const juce::String& titleIn, juce::Colour accentIn, const juce::File& fileIn)
    : title (titleIn), accent (accentIn), file (fileIn), play (accentIn)
{
    addAndMakeVisible (play);
    play.onClick = [this] { if (onTogglePreview) onTogglePreview(); };
    addAndMakeVisible (reveal);
    reveal.onClick = [this] { file.revealToUser(); };
    setMouseCursor (juce::MouseCursor::DraggingHandCursor);
    setTitle (title);
    setDescription ("Drag into your project, or reveal " + file.getFileName());
    setAlpha (0.0f);
}

void StemRow::enter (int index)
{
    appear.snap (0.0f);
    appear.animateTo (1.0f, motion::enter, easeOut, index * motion::stagger);
}

void StemRow::setPlayback (bool playing, float progress)
{
    play.setPlaying (playing);
    playingAmount.animateTo (playing ? 1.0f : 0.0f, playing ? motion::fade : motion::hoverOut);
    if (playing && std::abs (progress - playProgress) > 0.0005f)
    {
        playProgress = progress;
        repaint();
    }
}

void StemRow::tick()
{
    const auto now = nowMs();
    const bool a = appear.tick (now), h = hover.tick (now), p = playingAmount.tick (now);
    const bool b = h || p;

    if (a)
    {
        setAlpha (appear.value);
        const auto lift = prefersReducedMotion() ? 0.0f : 6.0f * (1.0f - appear.value);
        setTransform (juce::AffineTransform::translation (0.0f, lift));
    }
    if (a || b)
        repaint();
}

void StemRow::resized()
{
    auto area = getLocalBounds().reduced (10);
    reveal.setBounds (area.removeFromRight (76));
    area.removeFromRight (8);
    play.setBounds (area.removeFromRight (32).withSizeKeepingCentre (32, 32));
}

void StemRow::mouseEnter (const juce::MouseEvent&) { hover.animateTo (1.0f, motion::hover); }
void StemRow::mouseExit (const juce::MouseEvent&)  { hover.animateTo (0.0f, motion::hoverOut); }

void StemRow::mouseDrag (const juce::MouseEvent& e)
{
    if (dragging || e.getDistanceFromDragStart() < 4 || ! file.existsAsFile())
        return;

    // Drops straight onto FL Studio's playlist (or Finder, or any DAW).
    dragging = true;
    juce::DragAndDropContainer::performExternalDragDropOfFiles ({ file.getFullPathName() }, false, this,
                                                                [safe = juce::Component::SafePointer (this)] {
                                                                    if (safe != nullptr)
                                                                        safe->dragging = false;
                                                                });
}

void StemRow::paint (juce::Graphics& g)
{
    auto bounds = getLocalBounds().toFloat();
    g.setColour (colours::surface.interpolatedWith (colours::surfaceHi, hover.value));
    g.fillRoundedRectangle (bounds, 10.0f);

    // Playback position: a hairline along the bottom edge, moving linearly.
    if (playingAmount.value > 0.0f)
    {
        g.saveState();
        juce::Path clip;
        clip.addRoundedRectangle (bounds, 10.0f);
        g.reduceClipRegion (clip);
        g.setColour (accent.withAlpha (0.9f * playingAmount.value));
        g.fillRect (bounds.removeFromBottom (2.0f).withWidth (bounds.getWidth() * playProgress));
        g.restoreState();
    }

    auto content = getLocalBounds().reduced (14, 0);
    content.removeFromRight (130);

    auto dot = content.removeFromLeft (10).toFloat().withSizeKeepingCentre (8.0f, 8.0f);
    g.setColour (accent);
    g.fillEllipse (dot);
    content.removeFromLeft (12);

    auto top = content.removeFromTop (getHeight() / 2).toFloat();
    g.setColour (colours::text);
    g.setFont (font (14.0f, true));
    g.drawText (title, top.withTrimmedBottom (1.0f), juce::Justification::bottomLeft);

    const auto subtitleFont = font (12.0f);
    g.setFont (subtitleFont);
    g.setColour (colours::textDim);
    const auto hint = juce::String (" ") + juce::String::fromUTF8 ("\xc2\xb7") + " drag into your project";
    const auto name = middleEllipsis (file.getFileName(), subtitleFont,
                                      (float) content.getWidth() - juce::GlyphArrangement::getStringWidth (subtitleFont, hint));
    g.drawText (name + hint, content.toFloat().withTrimmedTop (2.0f), juce::Justification::topLeft);
}

} // namespace vox
