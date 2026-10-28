#include "PluginEditor.h"

using namespace vox;
using Phase = SeparationJob::Phase;

namespace
{
    constexpr int width = 540;
    constexpr int baseHeight = 510;
    constexpr int logHeight = 150;

    juce::String percent (float f) { return juce::String (juce::roundToInt (f * 100.0f)) + "%"; }

    juce::String deviceLabel (const juce::String& d)
    {
        if (d == "coreml")   return "CoreML";
        if (d == "cuda")     return "CUDA";
        if (d == "directml") return "DirectML";
        if (d == "cpu")      return "CPU";
        return "Runs offline";
    }
}

VoxSplitEditor::VoxSplitEditor (VoxSplitProcessor& p)
    : juce::AudioProcessorEditor (p), processor (p), job (p.getJob()), preview (p.getPreview())
{
    setOpaque (true);
    setWantsKeyboardFocus (true);

    addAndMakeVisible (dropZone);
    dropZone.onFileChosen = [this] (const juce::File& f) { chooseFile (f); };

    addAndMakeVisible (primary);
    primary.onClick = [this] { primaryClicked(); };

    addAndMakeVisible (logToggle);
    logToggle.onClick = [this] { toggleLog(); };

    addAndMakeVisible (progress);
    progress.setAlpha (0.0f);

    console.setMultiLine (true);
    console.setReadOnly (true);
    console.setCaretVisible (false);
    console.setScrollbarsShown (true);
    console.setFont (monoFont (11.5f));
    console.setColour (juce::TextEditor::backgroundColourId, colours::surface);
    console.setColour (juce::TextEditor::textColourId, colours::textDim);
    console.setColour (juce::TextEditor::outlineColourId, juce::Colours::transparentBlack);
    console.setColour (juce::TextEditor::focusedOutlineColourId, juce::Colours::transparentBlack);
    addChildComponent (console);

    if (const auto input = processor.getInputFile(); input.existsAsFile())
        dropZone.setFile (input, processor.describe (input));

    setSize (width, baseHeight);
    sync (true);
}

VoxSplitEditor::~VoxSplitEditor()
{
    // With the window gone there'd be no way to stop it.
    preview.stop();
}

void VoxSplitEditor::togglePreview (const juce::File& f)
{
    if (preview.isPlaying() && preview.getFile() == f)
        preview.stop();
    else
        preview.play (f); // one preview at a time: starting another replaces it
}

void VoxSplitEditor::chooseFile (const juce::File& f)
{
    if (job.isActive())
        return;

    preview.stop();
    processor.setInputFile (f);
    dropZone.setFile (f, processor.describe (f));
    job.restore (f, {}); // a new song clears the previous song's stems
}

void VoxSplitEditor::primaryClicked()
{
    if (job.isActive())
    {
        job.cancel();
        return;
    }

    const auto input = processor.getInputFile();
    if (! input.existsAsFile())
    {
        dropZone.openChooser();
        return;
    }

    preview.stop(); // the stems being previewed are about to be replaced
    rows.clear();
    job.start (input, SeparationJob::defaultOutputFolder (input));
}

void VoxSplitEditor::toggleLog()
{
    logOpen = ! logOpen;
    logToggle.setLabel (logOpen ? "Hide log" : "Show log");
    console.setVisible (logOpen);
    // The host resizes the window; no animation is possible across that boundary.
    setSize (width, baseHeight + (logOpen ? logHeight : 0));
}

bool VoxSplitEditor::keyPressed (const juce::KeyPress& key)
{
    // Keyboard actions take effect instantly: no press animation.
    if (key == juce::KeyPress::returnKey && primary.isEnabled())
    {
        primaryClicked();
        return true;
    }
    if (key == juce::KeyPress::escapeKey && job.isActive())
    {
        job.cancel();
        return true;
    }
    if (key == juce::KeyPress::spaceKey && ! rows.isEmpty())
    {
        // Space previews the lead (or stops whatever is playing).
        if (preview.isPlaying())
            preview.stop();
        else
            togglePreview (rows.getFirst()->getFile());
        return true;
    }
    if (key == juce::KeyPress ('o', juce::ModifierKeys::commandModifier, 0))
    {
        dropZone.openChooser();
        return true;
    }
    return false;
}

void VoxSplitEditor::tick()
{
    if (job.version() != seenVersion)
        sync (false);

    if (progressAlpha.tick (nowMs()))
        progress.setAlpha (progressAlpha.value);

    const bool playing = preview.isPlaying();
    const auto playingFile = preview.getFile();
    const auto position = playing ? preview.getProgress() : 0.0f;
    for (auto* row : rows)
        row->setPlayback (playing && row->getFile() == playingFile, position);
}

void VoxSplitEditor::sync (bool initial)
{
    seenVersion = job.version();
    const auto s = job.snapshot();
    const bool active = s.phase == Phase::preparing || s.phase == Phase::running;
    const bool hasInput = processor.getInputFile().existsAsFile();

    dropZone.setEnabled (! active);
    primary.setEnabled (active || hasInput);
    primary.setLabel (active ? "Cancel" : (s.phase == Phase::failed ? "Try again" : "Separate"));
    primary.setStyle (active ? PressableButton::Style::ghost : PressableButton::Style::primary);

    if (s.device.isNotEmpty())
        device = s.device;

    // Progress strip.
    const auto stageText = s.stage == "vocals" ? juce::String ("Isolating vocals") : juce::String ("Splitting lead from backing");
    const auto stageCount = "Stage " + juce::String (s.stageIndex) + " of " + juce::String (s.stages);
    switch (s.phase)
    {
        case Phase::preparing:
            progress.setState (ProgressStrip::Mode::indeterminate, 0.0f, "Loading models", "");
            break;
        case Phase::running:
            progress.setState (ProgressStrip::Mode::determinate, s.overall, stageText,
                               stageCount + "   " + juce::String::fromUTF8 ("\xc2\xb7") + "   " + percent (s.overall));
            break;
        case Phase::finished:
            progress.setState (ProgressStrip::Mode::done, 1.0f, "Done",
                               s.elapsedSec > 0.0 ? juce::String (s.elapsedSec, 1) + " s on " + deviceLabel (device) : juce::String());
            break;
        case Phase::idle: case Phase::failed: case Phase::cancelled:
            break;
    }

    const bool showProgress = active || s.phase == Phase::finished;
    if (initial)
    {
        progressAlpha.snap (showProgress ? 1.0f : 0.0f);
        progress.setAlpha (progressAlpha.value);
    }
    else
    {
        progressAlpha.animateTo (showProgress ? 1.0f : 0.0f, showProgress ? motion::fade : motion::hoverOut);
    }

    // Status line next to the button.
    statusColour = colours::textDim;
    switch (s.phase)
    {
        case Phase::failed:    status = s.error; statusColour = colours::danger; break;
        case Phase::cancelled: status = "Cancelled. Nothing was saved."; break;
        case Phase::finished:  status = "Saved to Music / VoxSplit / " + s.input.getFileNameWithoutExtension(); break;
        case Phase::idle:      status = hasInput ? "Stems are saved to Music / VoxSplit" : ""; break;
        case Phase::preparing: case Phase::running: status = ""; break;
    }

    // Stems appear once, when the run finishes. Opening the editor on a
    // finished job shows them immediately; animating on every open gets old.
    if (s.phase == Phase::finished && rows.isEmpty())
        showStems (s.outputs, ! initial && shownPhase != Phase::finished);
    else if (s.phase != Phase::finished)
        rows.clear();

    if (s.log.size() != shownLogSize)
    {
        shownLogSize = s.log.size();
        console.setText (s.log.joinIntoString ("\n"), false);
        console.moveCaretToEnd();
    }

    shownPhase = s.phase;
    resized();
    repaint();
}

void VoxSplitEditor::showStems (const juce::Array<SeparationJob::Output>& outputs, bool animate)
{
    int index = 0;
    for (const auto* stem : { "lead", "backing" })
        for (const auto& out : outputs)
            if (out.stem == stem)
            {
                const bool lead = out.stem == "lead";
                auto* row = rows.add (new StemRow (lead ? "Lead vocals" : "Backing vocals",
                                                   lead ? colours::lead : colours::backing, out.file));
                row->onTogglePreview = [this, file = out.file] { togglePreview (file); };
                addAndMakeVisible (row);
                if (animate)
                    row->enter (index);
                else
                    row->setAlpha (1.0f);
                ++index;
            }
}

void VoxSplitEditor::resized()
{
    auto area = getLocalBounds().reduced (24, 22);

    headerArea = area.removeFromTop (40);
    area.removeFromTop (16);

    dropZone.setBounds (area.removeFromTop (150));
    area.removeFromTop (16);

    auto actions = area.removeFromTop (36);
    primary.setBounds (actions.removeFromLeft (128));
    statusArea = actions.withTrimmedLeft (16);
    area.removeFromTop (20);

    progress.setBounds (area.removeFromTop (26));
    area.removeFromTop (16);

    rowsArea = area.removeFromTop (52 * 2 + 8);
    auto r = rowsArea;
    for (auto* row : rows)
    {
        row->setBounds (r.removeFromTop (52));
        r.removeFromTop (8);
    }
    area.removeFromTop (12);

    logToggle.setBounds (area.removeFromTop (20).removeFromLeft (80));
    area.removeFromTop (8);
    console.setBounds (area);
}

void VoxSplitEditor::paint (juce::Graphics& g)
{
    g.fillAll (colours::background);

    auto header = headerArea;
    auto titleRow = header.removeFromTop (22);
    g.setColour (colours::text);
    g.setFont (font (18.0f, true));
    g.drawText ("VoxSplit", titleRow, juce::Justification::centredLeft);

    g.setColour (colours::textDim);
    g.setFont (font (12.5f));
    g.drawText ("Lead and backing vocal separation, on this Mac", header, juce::Justification::centredLeft);

    // Device chip.
    const auto chipText = deviceLabel (device);
    const auto chipFont = font (11.5f, true);
    const auto chipWidth = juce::GlyphArrangement::getStringWidth (chipFont, chipText) + 30.0f;
    const auto chip = headerArea.toFloat().removeFromRight (chipWidth).withSizeKeepingCentre (chipWidth, 24.0f).withY ((float) headerArea.getY());
    g.setColour (colours::surface);
    g.fillRoundedRectangle (chip, 12.0f);
    g.setColour (colours::border);
    g.drawRoundedRectangle (chip.reduced (0.5f), 12.0f, 1.0f);
    g.setColour (device.isEmpty() || device == "cpu" ? colours::textDim : colours::success);
    g.fillEllipse (chip.getX() + 11.0f, chip.getCentreY() - 3.0f, 6.0f, 6.0f);
    g.setColour (colours::text);
    g.setFont (chipFont);
    g.drawText (chipText, chip.withTrimmedLeft (22.0f).withTrimmedRight (8.0f), juce::Justification::centredLeft);

    // Status next to the primary button.
    const auto statusFont = font (12.5f);
    g.setFont (statusFont);
    g.setColour (statusColour);
    g.drawFittedText (status, statusArea, juce::Justification::centredLeft, 2, 1.0f);

    // Placeholder where stems will land.
    if (rows.isEmpty())
    {
        juce::Path outline;
        outline.addRoundedRectangle (rowsArea.toFloat().reduced (0.5f), 10.0f);
        const float dashes[] = { 3.0f, 4.0f };
        juce::Path dashed;
        juce::PathStrokeType (1.0f).createDashedStroke (dashed, outline, dashes, 2);
        g.setColour (colours::border);
        g.fillPath (dashed);
        g.setColour (colours::textDim.withAlpha (0.7f));
        g.setFont (font (12.5f));
        g.drawText ("Lead and backing stems will appear here", rowsArea, juce::Justification::centred);
    }
}
