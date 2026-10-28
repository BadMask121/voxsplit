#pragma once

#include <juce_audio_processors/juce_audio_processors.h>

#include "DropZone.h"
#include "PluginProcessor.h"
#include "Widgets.h"

/**
    Drop a file, press Separate, drag the stems into the project.

    The editor never blocks: it polls the job's snapshot once per display
    frame (only re-reading it when the version counter moves) and the job's
    worker thread never calls back into the UI.
*/
class VoxSplitEditor final : public juce::AudioProcessorEditor
{
public:
    explicit VoxSplitEditor (VoxSplitProcessor&);
    ~VoxSplitEditor() override;

private:
    void paint (juce::Graphics&) override;
    void resized() override;
    bool keyPressed (const juce::KeyPress&) override;

    void chooseFile (const juce::File&);
    void primaryClicked();
    void toggleLog();
    void tick();
    void sync (bool initial);
    void showStems (const juce::Array<SeparationJob::Output>&, bool animate);
    void togglePreview (const juce::File&);

    VoxSplitProcessor& processor;
    SeparationJob& job;
    StemPreview& preview;

    DropZone dropZone;
    vox::PressableButton primary { "Separate", vox::PressableButton::Style::primary };
    vox::PressableButton logToggle { "Show log", vox::PressableButton::Style::link };
    vox::SegmentedToggle quality { { "Best", "Fast" } };
    vox::PassSlider passes { 1, 5 };
    vox::ProgressStrip progress;
    juce::OwnedArray<vox::StemRow> rows;
    juce::TextEditor console;

    juce::uint32 seenVersion = 0, seenEstimate = 0;
    SeparationJob::Phase shownPhase = SeparationJob::Phase::idle;
    int shownLogSize = -1;
    juce::String status, device;
    juce::Colour statusColour;
    juce::Rectangle<int> headerArea, statusArea, rowsArea, qualityLabelArea, passesLabelArea, passesValueArea;
    bool logOpen = false;
    vox::Tween progressAlpha;

    juce::VBlankAttachment vblank { this, [this] { tick(); } };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (VoxSplitEditor)
};
