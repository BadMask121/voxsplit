// `VoxSplitTests --snapshot out.png` renders the settings block off-screen for a
// visual check, without opening a window. Mirrors PluginEditor's layout.

#include <juce_gui_basics/juce_gui_basics.h>

#include "../Source/Theme.h"
#include "../Source/Widgets.h"

namespace
{
    struct Settings final : juce::Component
    {
        struct Row
        {
            const char* name;
            const char* role;
            vox::PassSlider slider;
            bool active;
            juce::Rectangle<int> label, value;
        };

        vox::SegmentedToggle quality { { "Best", "Fast" } };
        Row kim { "Kim Vocal 2", "vocals \xc2\xb7 0 = file is vocals", { 0, 5 }, true, {}, {} };
        Row roformer { "BS-RoFormer", "lead/backing \xc2\xb7 Best", { 1, 5 }, true, {}, {} };
        Row mdx { "MDX Karaoke 2", "lead/backing \xc2\xb7 Fast", { 1, 5 }, false, {}, {} };
        juce::Rectangle<int> qualityLabel;

        Settings (bool best, int vocal, int rof, int fast)
        {
            addAndMakeVisible (quality);
            quality.setSelected (best ? 0 : 1, false);
            roformer.active = best;
            mdx.active = ! best;
            kim.slider.setValue (vocal, false);
            roformer.slider.setValue (rof, false);
            mdx.slider.setValue (fast, false);
            for (auto* r : { &kim, &roformer, &mdx })
            {
                addAndMakeVisible (r->slider);
                r->slider.setAlpha (r->active ? 1.0f : 0.45f);
            }
        }

        void resized() override
        {
            auto area = getLocalBounds().reduced (24, 10);
            auto q = area.removeFromTop (32);
            qualityLabel = q.removeFromLeft (150);
            quality.setBounds (q.removeFromLeft (124));
            for (auto* r : { &kim, &roformer, &mdx })
            {
                area.removeFromTop (6);
                auto row = area.removeFromTop (34);
                r->label = row.removeFromLeft (150);
                r->value = row.removeFromRight (18);
                r->slider.setBounds (row.reduced (6, 0));
            }
        }

        void paint (juce::Graphics& g) override
        {
            g.fillAll (vox::colours::background);
            g.setFont (vox::font (12.5f));
            g.setColour (vox::colours::textDim);
            g.drawText ("Quality", qualityLabel, juce::Justification::centredLeft);
            for (auto* r : { &kim, &roformer, &mdx })
            {
                const float alpha = r->active ? 1.0f : 0.45f;
                auto label = r->label;
                g.setColour (vox::colours::text.withAlpha (alpha));
                g.setFont (vox::font (12.5f, true));
                g.drawText (r->name, label.removeFromTop (label.getHeight() / 2 + 1), juce::Justification::bottomLeft);
                g.setColour (vox::colours::textDim.withAlpha (alpha));
                g.setFont (vox::font (10.5f));
                g.drawText (juce::String::fromUTF8 (r->role), label, juce::Justification::topLeft);
                g.setColour (vox::colours::text.withAlpha (alpha));
                g.setFont (vox::font (13.0f, true));
                g.drawText (juce::String (r->slider.getValue()), r->value, juce::Justification::centredRight);
            }
        }
    };
}

int renderSnapshot (const juce::File& out)
{
    constexpr int w = 540, h = 172;
    juce::Image image (juce::Image::ARGB, w * 2, h * 2 * 2, true);
    juce::Graphics g (image);
    g.addTransform (juce::AffineTransform::scale (2.0f));
    int y = 0;
    for (auto best : { true, false })
    {
        Settings settings (best, best ? 1 : 0, 1, 3);
        settings.setBounds (0, 0, w, h);
        g.saveState();
        g.setOrigin (0, y);
        settings.paintEntireComponent (g, true);
        g.restoreState();
        y += h;
    }
    juce::PNGImageFormat png;
    out.deleteFile();
    juce::FileOutputStream stream (out);
    return png.writeImageToStream (image, stream) ? 0 : 1;
}
