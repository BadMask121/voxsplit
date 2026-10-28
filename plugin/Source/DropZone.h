#pragma once

#include <juce_gui_basics/juce_gui_basics.h>

#include "Theme.h"

/**
    "Drop a song here" target. Accepts files dragged from Finder or the DAW's
    browser (juce::FileDragAndDropTarget, i.e. OS-level drags) and falls back to
    a file chooser on click. Unsupported files are shown as rejected while
    hovering, instead of silently doing nothing on drop.
*/
class DropZone final : public juce::Component, public juce::FileDragAndDropTarget
{
public:
    DropZone();

    std::function<void (const juce::File&)> onFileChosen;

    void setFile (const juce::File& file, const juce::String& details);
    void openChooser();

private:
    enum class DragState { none, accept, reject };

    void paint (juce::Graphics&) override;
    void mouseEnter (const juce::MouseEvent&) override;
    void mouseExit (const juce::MouseEvent&) override;
    void mouseUp (const juce::MouseEvent&) override;
    void enablementChanged() override;

    bool isInterestedInFileDrag (const juce::StringArray& files) override;
    void fileDragEnter (const juce::StringArray& files, int, int) override;
    void fileDragExit (const juce::StringArray&) override;
    void filesDropped (const juce::StringArray& files, int, int) override;

    static juce::File firstSupported (const juce::StringArray& files);
    void setDragState (DragState);
    void tick();

    juce::File file;
    juce::String details;
    DragState drag = DragState::none;
    vox::Tween hover, accept, reject;
    std::unique_ptr<juce::FileChooser> chooser;
    juce::VBlankAttachment vblank { this, [this] { tick(); } };
};
