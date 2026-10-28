#include "DropZone.h"

#include "Widgets.h"

using namespace vox;

DropZone::DropZone()
{
    setMouseCursor (juce::MouseCursor::PointingHandCursor);
    setTitle ("Audio file");
    setDescription ("Drop a song here, or click to choose one");
}

void DropZone::setFile (const juce::File& f, const juce::String& d)
{
    file = f;
    details = d;
    repaint();
}

void DropZone::openChooser()
{
    if (! isEnabled())
        return;

    const auto start = file.existsAsFile() ? file.getParentDirectory()
                                           : juce::File::getSpecialLocation (juce::File::userMusicDirectory);
    chooser = std::make_unique<juce::FileChooser> ("Choose a song to separate", start, supportedAudioWildcard());
    chooser->launchAsync (juce::FileBrowserComponent::openMode | juce::FileBrowserComponent::canSelectFiles,
                          [safe = juce::Component::SafePointer (this)] (const juce::FileChooser& fc) {
                              if (safe == nullptr)
                                  return;
                              const auto chosen = fc.getResult();
                              if (isSupportedAudioFile (chosen) && safe->onFileChosen)
                                  safe->onFileChosen (chosen);
                          });
}

juce::File DropZone::firstSupported (const juce::StringArray& files)
{
    for (const auto& path : files)
        if (const juce::File f (path); isSupportedAudioFile (f))
            return f;
    return {};
}

void DropZone::setDragState (DragState s)
{
    drag = s;
    accept.animateTo (s == DragState::accept ? 1.0f : 0.0f, s == DragState::accept ? motion::hover : motion::hoverOut);
    reject.animateTo (s == DragState::reject ? 1.0f : 0.0f, s == DragState::reject ? motion::hover : motion::hoverOut);
    repaint();
}

void DropZone::tick()
{
    const auto now = nowMs();
    const bool a = hover.tick (now), b = accept.tick (now), c = reject.tick (now);
    if (a || b || c)
        repaint();
}

void DropZone::enablementChanged()
{
    if (! isEnabled())
        setDragState (DragState::none);
    repaint();
}

// Interested in every drag while enabled, so unsupported files get visible
// "can't use this" feedback rather than an unexplained refusal.
bool DropZone::isInterestedInFileDrag (const juce::StringArray&) { return isEnabled(); }

void DropZone::fileDragEnter (const juce::StringArray& files, int, int)
{
    setDragState (firstSupported (files) != juce::File() ? DragState::accept : DragState::reject);
}

void DropZone::fileDragExit (const juce::StringArray&) { setDragState (DragState::none); }

void DropZone::filesDropped (const juce::StringArray& files, int, int)
{
    const auto chosen = firstSupported (files);
    setDragState (DragState::none);
    if (chosen != juce::File() && onFileChosen)
        onFileChosen (chosen);
}

void DropZone::mouseEnter (const juce::MouseEvent&) { if (isEnabled()) hover.animateTo (1.0f, motion::hover); }
void DropZone::mouseExit (const juce::MouseEvent&)  { hover.animateTo (0.0f, motion::hoverOut); }

void DropZone::mouseUp (const juce::MouseEvent& e)
{
    if (e.mouseWasClicked())
        openChooser();
}

void DropZone::paint (juce::Graphics& g)
{
    const float enabledAlpha = isEnabled() ? 1.0f : 0.45f;
    auto bounds = getLocalBounds().toFloat().reduced (0.5f);

    auto fill = colours::surface.interpolatedWith (colours::surfaceHi, 0.5f * hover.value + 0.5f * accept.value);
    fill = fill.interpolatedWith (colours::danger.withAlpha (1.0f), 0.08f * reject.value);
    g.setColour (fill.withMultipliedAlpha (enabledAlpha));
    g.fillRoundedRectangle (bounds, 12.0f);

    auto outline = colours::border.interpolatedWith (colours::textDim, 0.4f * hover.value);
    outline = outline.interpolatedWith (colours::text, accept.value).interpolatedWith (colours::danger, reject.value);

    juce::Path border;
    border.addRoundedRectangle (bounds, 12.0f);
    g.setColour (outline.withMultipliedAlpha (enabledAlpha));
    if (file.existsAsFile() && drag == DragState::none)
    {
        g.strokePath (border, juce::PathStrokeType (1.0f));
    }
    else
    {
        const float dashes[] = { 5.0f, 4.0f };
        juce::Path dashed;
        juce::PathStrokeType (1.25f).createDashedStroke (dashed, border, dashes, 2);
        g.fillPath (dashed);
    }

    auto content = getLocalBounds().reduced (20);
    const bool hasFile = file.existsAsFile();

    // Arrow-into-tray glyph, nudged down while a valid file hovers.
    auto icon = content.removeFromTop (content.getHeight() / 2).toFloat();
    const auto nudge = prefersReducedMotion() ? 0.0f : 3.0f * accept.value;
    const auto c = juce::Point<float> (icon.getCentreX(), icon.getBottom() - 16.0f + nudge);
    juce::Path glyph;
    if (hasFile && drag == DragState::none)
    {
        // A small waveform for "loaded".
        const float bars[] = { 0.35f, 0.7f, 1.0f, 0.55f, 0.85f, 0.4f, 0.6f };
        for (int i = 0; i < 7; ++i)
        {
            const auto h = 22.0f * bars[i];
            glyph.addRoundedRectangle (c.x - 21.0f + (float) i * 6.0f, c.y - h / 2.0f, 3.0f, h, 1.5f);
        }
        g.setColour (colours::text.withMultipliedAlpha (enabledAlpha));
        g.fillPath (glyph);
    }
    else
    {
        glyph.startNewSubPath (c.x, c.y - 13.0f);
        glyph.lineTo (c.x, c.y + 3.0f);
        glyph.startNewSubPath (c.x - 6.0f, c.y - 3.0f);
        glyph.lineTo (c.x, c.y + 3.0f);
        glyph.lineTo (c.x + 6.0f, c.y - 3.0f);
        glyph.startNewSubPath (c.x - 13.0f, c.y + 5.0f);
        glyph.lineTo (c.x - 13.0f, c.y + 11.0f);
        glyph.lineTo (c.x + 13.0f, c.y + 11.0f);
        glyph.lineTo (c.x + 13.0f, c.y + 5.0f);
        g.setColour (outline.interpolatedWith (colours::text, 0.5f).withMultipliedAlpha (enabledAlpha));
        g.strokePath (glyph, juce::PathStrokeType (1.6f, juce::PathStrokeType::curved, juce::PathStrokeType::rounded));
    }

    juce::String headline, sub;
    switch (drag)
    {
        case DragState::accept: headline = "Release to load"; sub = "VoxSplit will split its vocals"; break;
        case DragState::reject: headline = "VoxSplit can't read that file"; sub = "Use WAV, AIFF, FLAC, MP3, M4A or OGG"; break;
        case DragState::none:
            if (hasFile) { headline = file.getFileName(); sub = details + "   " + juce::String::fromUTF8 ("\xc2\xb7") + "   drop or click to replace"; }
            else         { headline = "Drop a song here"; sub = "or click to browse  " + juce::String::fromUTF8 ("\xc2\xb7") + "  WAV, AIFF, FLAC, MP3, M4A"; }
            break;
    }

    content.removeFromTop (12);
    const auto headlineFont = font (15.0f, true);
    g.setFont (headlineFont);
    g.setColour ((drag == DragState::reject ? colours::danger : colours::text).withMultipliedAlpha (enabledAlpha));
    g.drawText (middleEllipsis (headline, headlineFont, (float) content.getWidth()),
                content.removeFromTop (20), juce::Justification::centred);

    g.setFont (font (12.5f));
    g.setColour (colours::textDim.withMultipliedAlpha (enabledAlpha));
    g.drawText (sub, content.removeFromTop (20), juce::Justification::centred, true);
}
