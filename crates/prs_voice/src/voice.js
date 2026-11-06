// The browser's half of voice: the microphone, WebCodecs and Web Audio. The game decides who is
// talking and how each voice should sound, and this only does what it is told. Nothing is created
// until it is needed: no microphone before the first push to talk, and nothing is ever stored.

const MIC_IDLE = 0, MIC_ASKING = 1, MIC_LIVE = 2, MIC_REFUSED = 3, MIC_NONE = 4, MIC_NO_CODEC = 5;
const SAMPLE_RATE = 48000;

const WORKLET = `registerProcessor("voice-tap", class extends AudioWorkletProcessor {
  constructor() { super(); this.held = new Float32Array(480); this.count = 0; }
  process(inputs) {
    const input = inputs[0][0];
    if (input) for (let i = 0; i < input.length; i++) {
      this.held[this.count++] = input[i];
      if (this.count === this.held.length) { this.port.postMessage(this.held.slice()); this.count = 0; }
    }
    return true;
  }
});`;

let cfg = null;
let ctx = null;
let output = null;
let volume = 1;
let canHear = false;
let canSpeak = false;
let mic = MIC_IDLE;
let wanted = false;
let stream = null;
let tap = null;
let encoder = null;
let sampleClock = 0;
const frames = [];
const speakers = new Map();
const MOST_FRAMES_WAITING = 8;
const SENT_KEPT = 256;

// Counts and clock times for tests. No audio goes in here.
const stats = { captured: 0, encoded: 0, received: 0, decoded: 0, played: 0, late: 0, errors: 0, peak: 0, sentAt: [], heardAt: [] };
globalThis.frontcraftVoice = stats;

function encoderConfig() {
  return {
    codec: "opus", sampleRate: SAMPLE_RATE, numberOfChannels: 1, bitrate: cfg.bitrate,
    opus: { format: "opus", frameDuration: cfg.frameMs * 1000, application: "voip", signal: "voice" },
  };
}

const decoderConfig = { codec: "opus", sampleRate: SAMPLE_RATE, numberOfChannels: 1 };

export function voice_configure(bitrate, frameMs, jitterMs, mostBehindMs, radioLow, radioHigh, radioGain, idleMs) {
  if (cfg) return;
  cfg = { bitrate, frameMs, jitter: jitterMs / 1000, mostBehind: mostBehindMs / 1000, radioLow, radioHigh, radioGain, idleMs };
  if (typeof AudioDecoder !== "undefined") {
    AudioDecoder.isConfigSupported(decoderConfig).then((r) => { canHear = !!r.supported; }, () => {});
  }
  if (typeof AudioEncoder !== "undefined") {
    AudioEncoder.isConfigSupported(encoderConfig()).then((r) => { canSpeak = !!r.supported; }, () => {});
  }
  setInterval(forgetQuiet, 1000);
}

export function voice_can_hear() { return canHear; }
// Apple's systems send Caps Lock down on one press and up on the next, so it can't mean "held" there.
export function voice_caps_lock_can_be_held() { return !/Mac|iPhone|iPad/.test(navigator.platform || ""); }
export function voice_mic_state() { return mic; }

function audio() {
  if (!ctx) {
    ctx = new AudioContext({ sampleRate: SAMPLE_RATE, latencyHint: "interactive" });
    output = ctx.createGain();
    output.gain.value = volume;
    // Whatever someone sends, this is as loud as it gets.
    const limiter = ctx.createDynamicsCompressor();
    limiter.threshold.value = -12;
    limiter.knee.value = 6;
    limiter.ratio.value = 20;
    limiter.attack.value = 0.002;
    limiter.release.value = 0.1;
    output.connect(limiter).connect(ctx.destination);
  }
  if (ctx.state === "suspended") ctx.resume();
  return ctx;
}

// Push to talk. The first one is what asks the browser for the microphone.
export function voice_ptt(down) {
  wanted = down;
  if (stream) stream.getAudioTracks().forEach((t) => { t.enabled = down; });
  if (!down && encoder && encoder.state === "configured") encoder.flush().catch(() => {});
  if (down && !stream && mic !== MIC_ASKING) openMic();
}

async function openMic() {
  if (!canSpeak) { mic = MIC_NO_CODEC; return; }
  if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) { mic = MIC_NONE; return; }
  mic = MIC_ASKING;
  let opened;
  try {
    opened = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 },
    });
    const context = audio();
    if (!tap) {
      const url = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
      await context.audioWorklet.addModule(url);
      URL.revokeObjectURL(url);
      tap = new AudioWorkletNode(context, "voice-tap", { numberOfOutputs: 0, channelCount: 1 });
      tap.port.onmessage = (e) => capture(e.data);
    }
    context.createMediaStreamSource(opened).connect(tap);
  } catch (e) {
    if (opened) opened.getTracks().forEach((t) => t.stop());
    mic = e && e.name === "NotFoundError" ? MIC_NONE : MIC_REFUSED;
    return;
  }
  // Turned off while the browser was asking.
  if (mic !== MIC_ASKING) { opened.getTracks().forEach((t) => t.stop()); return; }
  stream = opened;
  stream.getAudioTracks().forEach((t) => { t.enabled = wanted; });
  mic = MIC_LIVE;
}

// The microphone off in the settings: let it go altogether, and the browser's indicator with it.
export function voice_close_mic() {
  wanted = false;
  if (stream) stream.getTracks().forEach((t) => t.stop());
  stream = null;
  if (encoder && encoder.state !== "closed") encoder.close();
  encoder = null;
  mic = MIC_IDLE;
}

function capture(samples) {
  if (!wanted || !stream) return;
  stats.captured++;
  if (!encoder || encoder.state === "closed") {
    encoder = new AudioEncoder({
      output: (chunk) => {
        const bytes = new Uint8Array(chunk.byteLength);
        chunk.copyTo(bytes);
        stats.sentAt[stats.encoded++ % SENT_KEPT] = Date.now();
        if (frames.length >= MOST_FRAMES_WAITING) frames.shift();
        frames.push(bytes);
      },
      error: () => { stats.errors++; encoder = null; },
    });
    encoder.configure(encoderConfig());
  }
  // Behind means the machine can't keep up, and this is no time to make it wait for more.
  if (encoder.encodeQueueSize > 4) return;
  const data = new AudioData({
    format: "f32-planar", sampleRate: SAMPLE_RATE, numberOfFrames: samples.length, numberOfChannels: 1,
    timestamp: Math.round((sampleClock * 1e6) / SAMPLE_RATE), data: samples,
  });
  sampleClock += samples.length;
  encoder.encode(data);
  data.close();
}

export function voice_take_frame() { return frames.shift(); }

function makeSpeaker(id, radio) {
  const context = audio();
  const s = { id, radio, input: context.createGain(), next: 0, playing: new Set(), order: [], clock: 0, heardAt: 0 };
  if (radio) {
    const low = context.createBiquadFilter();
    low.type = "highpass";
    low.frequency.value = cfg.radioLow;
    const high = context.createBiquadFilter();
    high.type = "lowpass";
    high.frequency.value = cfg.radioHigh;
    s.input.gain.value = cfg.radioGain;
    s.input.connect(low).connect(high).connect(output);
  } else {
    s.muffle = context.createBiquadFilter();
    s.muffle.type = "lowpass";
    s.muffle.frequency.value = 20000;
    s.pan = context.createStereoPanner();
    s.input.gain.value = 0;
    s.input.connect(s.muffle).connect(s.pan).connect(output);
  }
  s.decoder = makeDecoder(s);
  return s;
}

function makeDecoder(s) {
  const decoder = new AudioDecoder({
    output: (data) => schedule(s, data),
    // Whatever made it fail came from another player, so start again with the next frame.
    error: () => { stats.errors++; s.decoder = null; },
  });
  decoder.configure(decoderConfig);
  return decoder;
}

function schedule(s, data) {
  stats.decoded++;
  const seq = s.order.shift();
  const samples = new Float32Array(data.numberOfFrames);
  data.copyTo(samples, { planeIndex: 0, format: "f32-planar" });
  const rate = data.sampleRate;
  data.close();
  if (!ctx || ctx.state !== "running") return;
  for (const v of samples) stats.peak = Math.max(stats.peak, Math.abs(v));
  const now = ctx.currentTime;
  if (s.next < now) s.next = now + cfg.jitter;
  if (s.next > now + cfg.mostBehind) { stats.late++; return; }
  const buffer = ctx.createBuffer(1, samples.length, rate);
  buffer.copyToChannel(samples, 0);
  const source = ctx.createBufferSource();
  source.buffer = buffer;
  source.connect(s.input);
  source.onended = () => s.playing.delete(source);
  s.playing.add(source);
  source.start(s.next);
  stats.played++;
  if (seq !== undefined) stats.heardAt[seq] = Date.now() + (s.next - now) * 1000;
  s.next += buffer.duration;
}

export function voice_play(id, radio, seq, bytes) {
  stats.received++;
  if (!canHear || !cfg) return;
  const key = id * 2 + (radio ? 1 : 0);
  let s = speakers.get(key);
  if (!s) {
    s = makeSpeaker(id, radio);
    speakers.set(key, s);
  }
  s.heardAt = performance.now();
  if (!s.decoder || s.decoder.state === "closed") s.decoder = makeDecoder(s);
  s.order.push(seq);
  s.decoder.decode(new EncodedAudioChunk({ type: "key", timestamp: s.clock++ * cfg.frameMs * 1000, data: bytes }));
}

// How a voice in the world sounds from where the listener is now. Eased, so it never steps.
export function voice_shape(id, gain, cutoff, pan) {
  const s = speakers.get(id * 2);
  if (!s || !ctx) return;
  const t = ctx.currentTime;
  s.input.gain.setTargetAtTime(gain, t, 0.03);
  s.muffle.frequency.setTargetAtTime(cutoff, t, 0.03);
  s.pan.pan.setTargetAtTime(pan, t, 0.03);
}

function endSpeaker(key, s) {
  for (const source of s.playing) { try { source.stop(); } catch {} }
  s.playing.clear();
  if (s.decoder && s.decoder.state !== "closed") s.decoder.close();
  s.input.disconnect();
  speakers.delete(key);
}

// Muted: not another sound from them, including what was already on its way to the speakers.
export function voice_stop(id) {
  for (const key of [id * 2, id * 2 + 1]) {
    const s = speakers.get(key);
    if (s) endSpeaker(key, s);
  }
}

function forgetQuiet() {
  const now = performance.now();
  for (const [key, s] of speakers) {
    if (now - s.heardAt > cfg.idleMs) endSpeaker(key, s);
  }
}

export function voice_volume(level) {
  volume = level;
  if (ctx) output.gain.setTargetAtTime(level, ctx.currentTime, 0.03);
}
