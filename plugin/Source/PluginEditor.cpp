#include "PluginEditor.h"

using namespace vox;
using Phase = SeparationJob::Phase;

namespace
{
    constexpr int width = 540;
    // Without the log; Vocal Remover hides two settings rows (quality + refinement).
    constexpr int baseHeight = 648;
    constexpr int leadBackingOnlyHeight = 32 + 6 + 34 + 6;
    constexpr int logHeight = 150;

    juce::String percent (float f) { return juce::String (juce::roundToInt (f * 100.0f)) + "%"; }

    /** "about 25 s", "about 2 min 10 s" — rounded so the number doesn't flicker. */
    juce::String about (double seconds)
    {
        if (seconds < 60.0)
            return "about " + juce::String (juce::jmax (5, (int) std::ceil (seconds / 5.0) * 5)) + " s";
        const auto rounded = (int) std::round (seconds / 10.0) * 10;
        const auto secs = rounded % 60;
        return "about " + juce::String (rounded / 60) + " min" + (secs > 0 ? " " + juce::String (secs) + " s" : juce::String());
    }

    /** Countdown while running: "1:05 left", "25 s left". */
    juce::String timeLeft (double seconds)
    {
        if (seconds < 0.0) return {};
        if (seconds < 5.0) return "a few seconds left";
        if (seconds < 60.0) return juce::String ((int) std::ceil (seconds / 5.0) * 5) + " s left";
        const auto s = (int) std::round (seconds);
        return juce::String (s / 60) + ":" + juce::String (s % 60).paddedLeft ('0', 2) + " left";
    }

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

    addAndMakeVisible (tool);
    tool.setSelected (vocalRemover() ? 1 : 0, false);
    tool.onChange = [this] (int index) {
        processor.setTool (index == 1 ? SeparationJob::Tool::vocalRemover : SeparationJob::Tool::leadBacking);
        showTool();
        sync (false);
    };

    addAndMakeVisible (quality);
    quality.setSelected (processor.getQuality() == SeparationJob::Quality::fast ? 1 : 0, false);
    quality.onChange = [this] (int index) {
        processor.setQuality (index == 1 ? SeparationJob::Quality::fast : SeparationJob::Quality::best);
        showQualityPasses (true);
        sync (false);
    };

    vocalRow.slider.setValue (processor.getVocalPasses(), false);
    vocalRow.slider.onChange = [this] (int n) { processor.setVocalPasses (n); sync (false); };
    refineRow.slider.onChange = [this] (int n) { processor.setPasses (processor.getQuality(), n); sync (false); };
    for (auto* row : passRows)
    {
        row->slider.setTitle (row->name);
        addAndMakeVisible (row->slider);
    }
    showQualityPasses (false);
    showTool();

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

    setSize (width, editorHeight());
    sync (true);
}

VoxSplitEditor::~VoxSplitEditor()
{
    // With the window gone there'd be no way to stop it.
    preview.stop();
}

int VoxSplitEditor::editorHeight() const
{
    return baseHeight - (vocalRemover() ? leadBackingOnlyHeight : 0) + (logOpen ? logHeight : 0);
}

void VoxSplitEditor::showTool()
{
    const bool remover = vocalRemover();
    quality.setVisible (! remover);
    refineRow.slider.setVisible (! remover);
    // The vocal remover *is* the vocal stage, so it runs at least once.
    vocalRow.slider.setRange (remover ? 1 : 0, 5);
    vocalRow.caption = remover ? juce::String ("Kim Vocal 2")
                               : juce::String::fromUTF8 ("Kim Vocal 2 \xc2\xb7 0 = file is vocals");
    // The new tool's stems are different files; drop the old rows.
    rows.clear();
    // The host resizes the window; no animation is possible across that boundary.
    setSize (width, editorHeight());
    resized();
    repaint();
}

void VoxSplitEditor::showQualityPasses (bool animate)
{
    const auto q = processor.getQuality();
    refineRow.caption = q == SeparationJob::Quality::best ? "BS-RoFormer (Best)" : "MDX Karaoke 2 (Fast)";
    refineRow.slider.setValue (processor.getPasses (q), animate);
    repaint();
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
    job.start (input, SeparationJob::defaultOutputFolder (input), processor.currentSettings());
}

void VoxSplitEditor::toggleLog()
{
    logOpen = ! logOpen;
    logToggle.setLabel (logOpen ? "Hide log" : "Show log");
    console.setVisible (logOpen);
    // The host resizes the window; no animation is possible across that boundary.
    setSize (width, editorHeight());
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
    if (job.version() != seenVersion || processor.getEstimate()->version != seenEstimate)
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
    seenEstimate = processor.getEstimate()->version;
    const auto s = job.snapshot();
    const bool active = s.phase == Phase::preparing || s.phase == Phase::running;
    const bool hasInput = processor.getInputFile().existsAsFile();

    dropZone.setEnabled (! active);
    quality.setEnabled (! active);
    tool.setEnabled (! active);
    for (auto* row : passRows)
        row->slider.setEnabled (! active);
    primary.setEnabled (active || hasInput);
    primary.setLabel (active ? "Cancel" : (s.phase == Phase::failed ? "Try again" : "Separate"));
    primary.setStyle (active ? PressableButton::Style::ghost : PressableButton::Style::primary);

    if (s.device.isNotEmpty())
        device = s.device;

    // Progress strip.
    const auto stageText = s.stage == "vocals"        ? juce::String ("Isolating vocals")
                         : s.stage == "vocals_refine" ? juce::String ("Cleaning up the vocals")
                         : s.stage == "refine"        ? juce::String ("Cleaning up the lead")
                                                      : juce::String ("Splitting lead from backing");
    const auto stageCount = "Stage " + juce::String (s.stageIndex) + " of " + juce::String (s.stages);
    switch (s.phase)
    {
        case Phase::preparing:
            progress.setState (ProgressStrip::Mode::indeterminate, 0.0f, "Loading models",
                               s.etaSec > 0.0 ? about (s.etaSec) + " in total" : juce::String());
            break;
        case Phase::running:
        {
            const auto dot = juce::String ("   ") + juce::String::fromUTF8 ("\xc2\xb7") + "   ";
            const auto left = timeLeft (s.etaSec);
            progress.setState (ProgressStrip::Mode::determinate, s.overall, stageText,
                               stageCount + dot + percent (s.overall) + (left.isNotEmpty() ? dot + left : juce::String()));
            break;
        }
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
        case Phase::idle:
        {
            const auto settings = processor.currentSettings();
            const double seconds = processor.getEstimate()->total (settings);
            status = ! hasInput    ? juce::String()
                   : seconds <= 0.0 ? juce::String::fromUTF8 ("Estimating time\xe2\x80\xa6")
                   : settings.tool == SeparationJob::Tool::leadBacking && settings.vocalPasses == 0
                       ? "Takes " + about (seconds) + ". Vocal passes is 0: the file must be vocals only."
                       : "Takes " + about (seconds) + " with these settings";
            break;
        }
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
    struct Kind { const char* stem; const char* title; juce::Colour colour; };
    const Kind kinds[] = { { "lead", "Lead vocals", colours::lead },
                           { "backing", "Backing vocals", colours::backing },
                           { "vocals", "Vocals", colours::lead },
                           { "instrumental", "Instrumental", colours::instrumental } };
    // Lead/backing runs only show the instrumental if the engine was asked for it.
    const bool remover = vocalRemover();
    int index = 0;
    for (const auto& kind : kinds)
        for (const auto& out : outputs)
            if (out.stem == kind.stem && (remover || out.stem != "instrumental"))
            {
                auto* row = rows.add (new StemRow (kind.title, kind.colour, out.file));
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

    dropZone.setBounds (area.removeFromTop (128));
    area.removeFromTop (14);

    // Settings: tool, quality (lead/backing only), then the pass sliders.
    constexpr int labelWidth = 150;
    auto toolRow = area.removeFromTop (32);
    toolLabelArea = toolRow.removeFromLeft (labelWidth);
    tool.setBounds (toolRow.removeFromLeft (248));
    if (! vocalRemover())
    {
        area.removeFromTop (6);
        auto qualityRow = area.removeFromTop (32);
        qualityLabelArea = qualityRow.removeFromLeft (labelWidth);
        quality.setBounds (qualityRow.removeFromLeft (124));
    }
    for (auto* row : passRows)
    {
        if (! row->slider.isVisible())
            continue;
        area.removeFromTop (6);
        auto r = area.removeFromTop (34);
        row->label = r.removeFromLeft (labelWidth);
        row->value = r.removeFromRight (18);
        row->slider.setBounds (r.reduced (6, 0));
    }
    area.removeFromTop (14);

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

    // Settings labels: "Quality", then each slider's name, caption and value.
    g.setFont (font (12.5f));
    g.setColour (colours::textDim);
    g.drawText ("Tool", toolLabelArea, juce::Justification::centredLeft);
    if (! vocalRemover())
        g.drawText ("Quality", qualityLabelArea, juce::Justification::centredLeft);
    for (auto* row : passRows)
    {
        if (! row->slider.isVisible())
            continue;
        const float alpha = row->slider.isEnabled() ? 1.0f : 0.45f;
        auto label = row->label;
        g.setColour (colours::text.withAlpha (alpha));
        g.setFont (font (12.5f, true));
        g.drawText (row->name, label.removeFromTop (label.getHeight() / 2 + 1), juce::Justification::bottomLeft);
        g.setColour (colours::textDim.withAlpha (alpha));
        g.setFont (font (10.5f));
        g.drawText (row->caption, label, juce::Justification::topLeft);
        g.setColour (colours::text.withAlpha (alpha));
        g.setFont (font (13.0f, true));
        g.drawText (juce::String (row->slider.getValue()), row->value, juce::Justification::centredRight);
    }

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
