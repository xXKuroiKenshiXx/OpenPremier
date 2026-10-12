# Native mono, stereo and 5.1 routing

OpenPremier mixes an explicitly selected sequence master: Mono, Stereo or Surround 5.1.
The native 5.1 channel order is **L, R, C, LFE, Ls, Rs**. Six-channel conformed caches use this
canonical order. This is native routing; adaptive buses, arbitrary speaker maps and external
audio plugin hosting are not implemented.

## Sequence and source channels

A Surround 5.1 master uses six channels throughout source interpolation, clip effects, clip
gain, track gain and master gain. A six-channel source preserves its C, LFE and surround channels;
it is not folded to L/R before reaching a 5.1 master.

Stereo and Mono masters retain the established stereo processing bus. A Mono master averages
that bus to one channel after the master fader. Mono sources feed both front channels; stereo
sources feed L/R. When either source is placed in a 5.1 sequence, C/LFE/Ls/Rs remain silent.
There is no synthesized center channel or bass management.

Conversion of a 5.1 master/source to stereo follows this explicit contract, with
`k = 1 / sqrt(2)` (approximately 0.7071):

```text
stereo L = L + k × C + k × Ls
stereo R = R + k × C + k × Rs
mono     = (stereo L + stereo R) / 2
```

LFE is omitted from the stereo/mono fold. The mixer does not normalize or limit the sum, so a
fold may exceed full scale. Six-channel output keeps LFE intact. Other discrete source layouts
retain the earlier fallback of their first two channels; no adaptive/discrete mapping parity
is claimed.

Nested sequences render their own master before conversion to the parent's processing bus.
A 5.1 child in a 5.1 parent preserves all six channels. A 5.1 child in a stereo parent folds to
stereo. A stereo or mono child in a 5.1 parent feeds only its front pair. A child intentionally
configured as stereo does not regain surround information when placed in a 5.1 parent.

## Gain, pan, effects and meters

Clip gain, clip Volume, transitions, track Volume and master Volume affect all six channels.
The existing Panner and left/right Channel Volume controls affect only front L/R; they are not
surround-position controls. The stereo balance law and the mono pan law retain their existing
behavior, including unity per front channel for a mono source at center and the existing
constant-power endpoint gains.

DSP effects receive the full six-channel buffer on a 5.1 master. Filters, delays and other DSP
that process every interleaved channel now operate on C/LFE/surround as well. Effects explicitly
implemented for stereo fronts—such as reverb, auto-pan, left/right amplify, channel swap/fill and
stereo expander—continue to operate on front L/R and leave other channels unchanged. This does
not establish a surround DSP conformance matrix.

Master peak meters observe the sequence master before playback-device adaptation, including
the LFE channel. The existing Audio Track Mixer displays two bars per track; for a 5.1 track,
these display its stereo fold and omit LFE. Track mute and solo continue to affect the entire
track, not individual channels.

## Playback, scrubbing and export

Live playback and scrubbing configure their mixer from the sequence master. Scrub fades apply
to every master channel. Playback then adapts the mixed master to the default output device's
channel count:

| Device channels | Adaptation |
|---|---|
| 1 | Mono master is copied; stereo is averaged; 5.1 uses the mono fold above. |
| 2 | Mono is duplicated; stereo is copied; 5.1 uses the stereo fold above. |
| 6 | A 5.1 master is copied in canonical order; mono/stereo feed L/R with other channels silent. |
| Other | A stereo preview occupies the first two channels; additional channels are silent. |

The device backend exposes a channel count here, not a validated speaker-position map. The
six-channel device path assumes canonical order; verify a physical surround device with a
channel-identification fixture before relying on its speaker positions. No host routing or
adaptive hardware support is claimed.

Offline rendering uses the same sequence graph and then converts to the requested export
channel count. Select a 5.1 master and six export channels to preserve six-channel output.
Choosing a stereo or mono export intentionally folds the master. Source layouts and source
metadata are not rewritten by rendering.

## Validation and remaining limits

Original PCM fixtures exercise one impulse per channel, matching six-channel streaming preview
and offline output, explicit stereo/mono folds, nested layouts, front pan and gains, mute,
resampling/reverse channel isolation, center-channel filtering, silent live/scrub master meters
and the device adapter. Existing stereo/tape/crossfade tests remain part of the audio suite.

This validation establishes native signal routing, not Premiere equivalence. Physical 5.1
hardware, every codec's speaker labels, every DSP, arbitrary routing, adaptive layouts,
sample-by-sample automation across different block sizes, and external plugins remain unverified.
Track/master gain and some effect parameters still use block sampling; sharing the rendering
code does not prove identical results for every animated effect at every block size.
The existing audio nesting depth guard remains eight levels. Deep nesting and distinct DSP
state for repeated instances of the same nested sequence are outside this routing validation.
