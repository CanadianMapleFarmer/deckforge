#include "signalsmith-stretch/signalsmith-stretch.h"

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

extern "C" {

Stretch *df_stretch_new(float sample_rate) {
    auto *stretch = new Stretch(0x6465636b);
    stretch->presetDefault(2, sample_rate, true);
    return stretch;
}

void df_stretch_free(Stretch *stretch) { delete stretch; }

int df_stretch_input_latency(const Stretch *stretch) { return stretch->inputLatency(); }

int df_stretch_output_latency(const Stretch *stretch) { return stretch->outputLatency(); }

int df_stretch_preroll(const Stretch *stretch) { return stretch->blockSamples() + stretch->intervalSamples(); }

void df_stretch_reset(Stretch *stretch) { stretch->reset(); }

void df_stretch_seek(Stretch *stretch, const float *left, const float *right, int frames, double rate) {
    const float *inputs[2] = {left, right};
    stretch->seek(inputs, frames, rate);
}

void df_stretch_process(Stretch *stretch, const float *in_left, const float *in_right, int in_frames, float *out_left, float *out_right, int out_frames) {
    const float *inputs[2] = {in_left, in_right};
    float *outputs[2] = {out_left, out_right};
    stretch->process(inputs, in_frames, outputs, out_frames);
}

}
