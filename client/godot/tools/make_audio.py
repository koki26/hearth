"""Generates the game's music and sound effects as .wav files.

    python client/godot/tools/make_audio.py

No dependencies and no licence questions: everything here is synthesised from
sine waves and noise. The music loops seamlessly because every oscillator and
every tremolo is snapped to a whole number of cycles over the loop length, so
the last sample runs into the first with no click.

Replace any file in client/godot/audio with a real recording of the same name
and the game picks it up; nothing in the code knows how these were made.
"""

import math
import random
import struct
import wave
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "audio"
MUSIC_RATE = 22050  # Pads and muzak carry nothing above 11 kHz.
SFX_RATE = 44100

A4 = 440.0
NAMES = {"c": 0, "d": 2, "e": 4, "f": 5, "g": 7, "a": 9, "b": 11}


def pitch(name):
    """Turns a note name such as a4, c#5 or bb3 into hertz."""
    step = NAMES[name[0]]
    rest = name[1:]
    if rest[0] == "#":
        step, rest = step + 1, rest[1:]
    elif rest[0] == "b":
        step, rest = step - 1, rest[1:]
    return A4 * 2 ** ((step - 9) / 12 + (int(rest) - 4))


class Track:
    """A mono buffer that voices are added into."""

    def __init__(self, seconds, rate):
        self.rate = rate
        self.n = int(round(seconds * rate))
        self.seconds = self.n / rate
        self.buf = [0.0] * self.n

    def snap(self, freq):
        """Nearest frequency fitting a whole number of cycles in the loop,
        which is what makes the loop seamless."""
        return max(1, round(freq * self.seconds)) / self.seconds

    def add(self, at, samples, gain=1.0):
        """Mixes in at `at` seconds, wrapping past the end so a decaying tail
        lands at the start of the loop instead of being cut off."""
        start = int(round(at * self.rate))
        for i, s in enumerate(samples):
            self.buf[(start + i) % self.n] += s * gain

    def sustain(self, freq, gain, cycles, phase=0.0):
        """A tone across the whole loop, swelling `cycles` times."""
        f = self.snap(freq)
        for i in range(self.n):
            t = i / self.rate
            swell = 0.5 - 0.5 * math.cos(2 * math.pi * (cycles * t / self.seconds + phase))
            self.buf[i] += math.sin(2 * math.pi * f * t) * gain * swell

    def write(self, name):
        peak = max(max(self.buf), -min(self.buf), 1e-9)
        # Leave headroom so the game can layer voice on top without clipping.
        scale = 0.84 / peak
        frames = b"".join(
            struct.pack("<h", max(-32768, min(32767, int(s * scale * 32767)))) for s in self.buf
        )
        OUT.mkdir(parents=True, exist_ok=True)
        with wave.open(str(OUT / name), "wb") as f:
            f.setnchannels(1)
            f.setsampwidth(2)
            f.setframerate(self.rate)
            f.writeframes(frames)
        print("%s: %.1fs, %d KiB" % (name, self.seconds, len(frames) // 1024))


def decay(n, rate, tau, attack=0.004):
    """Pluck envelope: near-instant attack, exponential fall."""
    rise = max(1, int(attack * rate))
    return [min(1.0, i / rise) * math.exp(-(i / rate) / tau) for i in range(n)]


def vibe(freq, seconds, rate, tau=0.9):
    """Vibraphone-ish: a fundamental, its octave and a soft fourth harmonic,
    with the slow tremolo that makes the classic lift-music sound."""
    n = int(seconds * rate)
    env = decay(n, rate, tau)
    out = []
    for i in range(n):
        t = i / rate
        trem = 0.82 + 0.18 * math.sin(2 * math.pi * 5.2 * t)
        tone = (
            math.sin(2 * math.pi * freq * t)
            + 0.30 * math.sin(2 * math.pi * freq * 2 * t)
            + 0.08 * math.sin(2 * math.pi * freq * 4 * t)
        )
        out.append(tone * env[i] * trem * 0.5)
    return out


def bass(freq, seconds, rate):
    n = int(seconds * rate)
    env = decay(n, rate, seconds * 0.5, attack=0.01)
    return [
        (
            math.sin(2 * math.pi * freq * i / rate)
            + 0.22 * math.sin(2 * math.pi * freq * 2 * i / rate)
        )
        * env[i]
        * 0.5
        for i in range(n)
    ]


def shaker(seconds, rate, rng):
    n = int(seconds * rate)
    env = decay(n, rate, 0.028, attack=0.001)
    # Two-pole highpass by differencing twice: a brush, not a thump.
    noise = [rng.uniform(-1.0, 1.0) for _ in range(n + 2)]
    return [(noise[i + 2] - 2 * noise[i + 1] + noise[i]) * env[i] * 0.09 for i in range(n)]


def pad(freq, seconds, rate):
    """A soft voice that swells in and fades out: two slightly detuned sines
    with a slow attack. The building block of the hub chords."""
    n = int(seconds * rate)
    rise = max(1, int(0.8 * rate))
    out = [0.0] * n
    for detune, share in ((1.0, 1.0), (1.0035, 0.7)):
        f = freq * detune
        step = 2 * math.pi * f / rate
        phase = 0.0
        for i in range(n):
            env = min(1.0, i / rise) * math.exp(-(i / rate) / (seconds * 0.9))
            out[i] += math.sin(phase) * env * share
            phase += step
    return [v * 0.2 for v in out]


def hub_music():
    """The hub: Am - F - C - G, round twice, with a tune over it.

    Something to stand and talk over, but actually a piece of music rather
    than one held chord. Eight bars at 60 bpm is exactly 32 seconds, and every
    voice is added with `Track.add`, whose tails wrap past the end -- so the
    last bar rings into the first and the loop has no seam."""
    bar = 4.0
    track = Track(8 * bar, MUSIC_RATE)
    rng = random.Random(5)

    chords = [
        ("a2", ["a3", "c4", "e4", "a4"]),
        ("f2", ["f3", "a3", "c4", "f4"]),
        ("c3", ["c3", "e3", "g3", "c4"]),
        ("g2", ["g3", "b3", "d4", "g4"]),
    ]
    # Two four-bar phrases: the second answers the first an octave up.
    tune = [
        [(0.0, "e5", 2.5), (2.5, "c5", 1.5)],
        [(0.0, "a4", 3.0)],
        [(0.0, "g4", 1.5), (1.5, "e4", 2.5)],
        [(0.0, "d5", 2.0), (2.0, "b4", 1.5)],
        [(0.0, "a5", 2.5), (2.5, "g5", 1.5)],
        [(0.0, "f5", 3.0)],
        [(0.0, "e5", 1.5), (1.5, "g5", 2.5)],
        [(0.0, "d5", 2.0), (2.0, "e5", 2.5)],
    ]
    for b in range(8):
        at = b * bar
        root, voicing = chords[b % 4]
        track.add(at, bass(pitch(root), bar * 0.95, MUSIC_RATE), 0.5)
        for note in voicing:
            track.add(at, pad(pitch(note), bar * 1.35, MUSIC_RATE), 0.45)
        for offset, note, length in tune[b]:
            track.add(at + offset, vibe(pitch(note), length, MUSIC_RATE, tau=length * 0.45), 0.3)
        # A brush on two and four. Barely there: it is a pulse, not a beat.
        for beat in (1.0, 3.0):
            track.add(at + beat, shaker(0.12, MUSIC_RATE, rng), 0.45)

    # A tonic drone under the chords, spanning the whole loop and snapped to
    # whole cycles, so it loops by construction. Without it the eighth bar had
    # decayed to a third of the opening level by the time it came round again,
    # which is a lull you hear every thirty seconds.
    for note, gain, swells in (("a1", 0.16, 1), ("a2", 0.1, 2), ("e3", 0.05, 3)):
        track.sustain(pitch(note), gain, swells)

    # A low breath under all of it. Noise generated once over the loop length
    # loops by construction.
    smooth = 0.0
    for i in range(track.n):
        smooth += (rng.uniform(-1.0, 1.0) - smooth) * 0.002
        track.buf[i] += smooth * 1.0
    track.write("hub.wav")


def elevator_music():
    """Dm7 - G7 - Cmaj7 - A7, twice, with a vibraphone line over a walking
    bass and a brush shaker. Deliberately bland: it is lift music."""
    bpm = 100.0
    beat = 60.0 / bpm
    bar = 4 * beat
    track = Track(8 * bar, MUSIC_RATE)
    rng = random.Random(11)

    chords = [
        ("d3", ["f3", "a3", "c4", "e4"]),
        ("g2", ["f3", "b3", "d4", "e4"]),
        ("c3", ["e3", "g3", "b3", "d4"]),
        ("a2", ["c#3", "g3", "a3", "e4"]),
    ]
    # Two passes: the melody answers itself the second time round.
    tunes = [
        [(0.0, "a4"), (1.0, "f4"), (2.0, "e4"), (3.5, "d4")],
        [(0.0, "b4"), (1.5, "d5"), (2.5, "b4"), (3.0, "a4")],
        [(0.0, "c5"), (1.0, "g4"), (2.0, "e4"), (3.0, "g4")],
        [(0.0, "e4"), (1.5, "c#4"), (2.5, "e4"), (3.0, "a4")],
        [(0.0, "f4"), (1.0, "a4"), (2.0, "d5"), (3.5, "c5")],
        [(0.0, "b4"), (1.5, "f4"), (2.5, "d4"), (3.0, "b3")],
        [(0.0, "g4"), (1.0, "e5"), (2.0, "d5"), (3.0, "b4")],
        [(0.0, "a4"), (1.5, "e4"), (2.5, "c#4"), (3.0, "a3")],
    ]
    for b in range(8):
        root, chord = chords[b % 4]
        at = b * bar
        # Walking bass: root on one, its fifth on three.
        track.add(at, bass(pitch(root), beat * 1.8, MUSIC_RATE), 0.55)
        track.add(at + 2 * beat, bass(pitch(root) * 1.5, beat * 1.6, MUSIC_RATE), 0.40)
        # Chords on the off-beats, the way a comping piano sits back.
        for offset in (1.0, 2.5):
            for note in chord:
                track.add(at + offset * beat, vibe(pitch(note), beat * 1.6, MUSIC_RATE, tau=0.45), 0.13)
        for i, note in tunes[b]:
            track.add(at + i * beat, vibe(pitch(note), beat * 1.4, MUSIC_RATE), 0.40)
        for eighth in range(8):
            track.add(at + eighth * beat * 0.5, shaker(0.1, MUSIC_RATE, rng), 0.7 if eighth % 2 else 1.0)
    track.write("elevator.wav")


def sfx():
    rng = random.Random(3)

    def tones(name, parts, gain=1.0):
        """parts: (start seconds, frequency, length seconds)."""
        end = max(s + d for s, _, d in parts) + 0.25
        track = Track(end, SFX_RATE)
        for start, freq, dur in parts:
            n = int(dur * SFX_RATE)
            env = decay(n, SFX_RATE, dur * 0.35, attack=0.002)
            body = [
                (
                    math.sin(2 * math.pi * freq * i / SFX_RATE)
                    + 0.25 * math.sin(2 * math.pi * freq * 3 * i / SFX_RATE)
                )
                * env[i]
                * 0.5
                for i in range(n)
            ]
            track.add(start, body, gain)
        track.write(name)

    tones("click.wav", [(0.0, 2100.0, 0.04)])
    tones("mute.wav", [(0.0, 880.0, 0.09), (0.07, 587.0, 0.16)])
    tones("unmute.wav", [(0.0, 587.0, 0.09), (0.07, 880.0, 0.16)])
    tones("menu.wav", [(0.0, 1320.0, 0.05), (0.04, 1760.0, 0.12)])

    # A lift bell: a struck metal bar, so a long tail and an inharmonic partial.
    bell = Track(2.0, SFX_RATE)
    n = int(1.7 * SFX_RATE)
    env = decay(n, SFX_RATE, 0.55, attack=0.001)
    bell.add(
        0.0,
        [
            (
                math.sin(2 * math.pi * 1318.5 * i / SFX_RATE)
                + 0.55 * math.sin(2 * math.pi * 2637.0 * i / SFX_RATE)
                + 0.20 * math.sin(2 * math.pi * 3990.0 * i / SFX_RATE)
            )
            * env[i]
            * 0.45
            for i in range(n)
        ],
    )
    bell.write("ding.wav")

    # Picking up and putting down: a soft knock with a falling pitch.
    for name, f0, f1, tau in (("grab.wav", 260.0, 190.0, 0.09), ("drop.wav", 200.0, 120.0, 0.14)):
        track = Track(0.45, SFX_RATE)
        n = int(0.35 * SFX_RATE)
        env = decay(n, SFX_RATE, tau, attack=0.002)
        phase = 0.0
        body = []
        for i in range(n):
            f = f0 + (f1 - f0) * (i / n)
            phase += 2 * math.pi * f / SFX_RATE
            knock = 0.3 * rng.uniform(-1.0, 1.0) * math.exp(-(i / SFX_RATE) / 0.012)
            body.append((math.sin(phase) + knock) * env[i] * 0.5)
        track.add(0.0, body)
        track.write(name)

    camera_and_chat()


def camera_and_chat():
    """The camera's shutter, and the pop a chat line arrives with."""
    rng = random.Random(7)

    # Two clicks of noise 70 ms apart, the second softer: a mirror going up and
    # coming back down, which is what people still hear as "a photo".
    shutter = Track(0.35, SFX_RATE)
    for start, gain, tau in ((0.0, 0.8, 0.006), (0.07, 0.5, 0.01)):
        n = int(0.06 * SFX_RATE)
        env = decay(n, SFX_RATE, tau, attack=0.0005)
        last = 0.0
        body = []
        for i in range(n):
            # One-pole smoothing takes the hiss off the top of the noise.
            last += 0.45 * (rng.uniform(-1.0, 1.0) - last)
            body.append(last * env[i])
        shutter.add(start, body, gain)
    shutter.write("shutter.wav")

    track = Track(0.4, SFX_RATE)
    for start, freq, dur in ((0.0, 987.8, 0.06), (0.05, 1479.98, 0.13)):
        n = int(dur * SFX_RATE)
        env = decay(n, SFX_RATE, dur * 0.35, attack=0.002)
        track.add(start, [math.sin(2 * math.pi * freq * i / SFX_RATE) * env[i] * 0.3 for i in range(n)])
    track.write("chat.wav")


if __name__ == "__main__":
    hub_music()
    elevator_music()
    sfx()
