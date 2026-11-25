/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
class CatAudio {
  private ctx: AudioContext | null = null;
  private enabled = true;
  private graphReady = false;
  private masterGain: GainNode | null = null;
  private currentIntensity = 0;

  public setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (!enabled) {
      this.setPurrIntensity(0);
    }
  }

  private ensurePurrGraph(): boolean {
    if (!this.enabled) return false;
    if (!this.ctx) {
      const AC =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      if (!AC) return false;
      this.ctx = new AC();
    }
    if (this.ctx.state === 'suspended') {
      void this.ctx.resume().catch(() => {});
    }
    if (this.graphReady) return true;

    const ctx = this.ctx;
    const sampleRate = ctx.sampleRate;
    const bufLen = Math.floor(sampleRate * 3.0);
    const noiseBuf = ctx.createBuffer(1, bufLen, sampleRate);
    const data = noiseBuf.getChannelData(0);
    // Smooth warm noise buffer for organic feline throat & breath texture
    for (let i = 0; i < bufLen; i++) {
      data[i] = Math.random() * 2 - 1;
    }

    const noiseSrc = ctx.createBufferSource();
    noiseSrc.buffer = noiseBuf;
    noiseSrc.loop = true;

    // Layer 1: Warm chest rumble (~190 Hz)
    const warmFilter = ctx.createBiquadFilter();
    warmFilter.type = 'bandpass';
    warmFilter.frequency.value = 190;
    warmFilter.Q.value = 1.15;
    const warmGain = ctx.createGain();
    warmGain.gain.value = 1.45;

    // Layer 2: Soft breathy throat resonance (~520 Hz)
    const breathFilter = ctx.createBiquadFilter();
    breathFilter.type = 'bandpass';
    breathFilter.frequency.value = 520;
    breathFilter.Q.value = 1.4;
    const breathGain = ctx.createGain();
    breathGain.gain.value = 0.34;

    // 24.5 Hz feline purr flutter amplitude modulation (0.30 .. 1.00)
    const flutterGain = ctx.createGain();
    flutterGain.gain.value = 0.65;
    const flutterLfo = ctx.createOscillator();
    flutterLfo.type = 'sine';
    flutterLfo.frequency.value = 24.5;
    const flutterDepth = ctx.createGain();
    flutterDepth.gain.value = 0.35;
    flutterLfo.connect(flutterDepth);
    flutterDepth.connect(flutterGain.gain);

    // Slow cozy breathing cadence (~0.68 Hz: inhale/exhale swell)
    const breathCycleGain = ctx.createGain();
    breathCycleGain.gain.value = 0.76;
    const breathLfo = ctx.createOscillator();
    breathLfo.type = 'sine';
    breathLfo.frequency.value = 0.68;
    const breathDepth = ctx.createGain();
    breathDepth.gain.value = 0.24;
    breathLfo.connect(breathDepth);
    breathDepth.connect(breathCycleGain.gain);

    // Soft lowpass polish so it sounds cozy on both speakers & headphones
    const polishFilter = ctx.createBiquadFilter();
    polishFilter.type = 'lowpass';
    polishFilter.frequency.value = 1400;

    // Master output gain (starts silent, ramps smoothly during petting)
    const master = ctx.createGain();
    master.gain.value = 0;

    noiseSrc.connect(warmFilter);
    warmFilter.connect(warmGain);
    warmGain.connect(flutterGain);

    noiseSrc.connect(breathFilter);
    breathFilter.connect(breathGain);
    breathGain.connect(flutterGain);

    flutterGain.connect(breathCycleGain);
    breathCycleGain.connect(polishFilter);
    polishFilter.connect(master);
    master.connect(ctx.destination);

    noiseSrc.start();
    flutterLfo.start();
    breathLfo.start();

    this.masterGain = master;
    this.graphReady = true;
    return true;
  }

  /**
   * Smoothly drives real-time organic cat purring (0 = silent, 1 = full cozy purr).
   */
  public setPurrIntensity(intensity: number): void {
    const target = this.enabled ? Math.max(0, Math.min(1, intensity)) : 0;
    if (target <= 0.01 && !this.graphReady) return;
    if (Math.abs(target - this.currentIntensity) < 0.005) return;
    this.currentIntensity = target;

    if (target > 0.01 && !this.ensurePurrGraph()) return;
    if (!this.ctx || !this.masterGain) return;

    if (target > 0.01 && this.ctx.state === 'suspended') {
      void this.ctx.resume().catch(() => {});
    }

    const now = this.ctx.currentTime;
    const targetGain = target * 0.26;
    const timeConst = target > 0.01 ? 0.14 : 0.22;
    this.masterGain.gain.setTargetAtTime(targetGain, now, timeConst);
  }
}

export const catAudio = new CatAudio();
