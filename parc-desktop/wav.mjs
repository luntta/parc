// Recordings reach whisper.cpp as 16 kHz mono 16-bit WAV, the format it reads directly. The renderer decodes
// what MediaRecorder captured and resamples it with Web Audio, so dictation needs no ffmpeg.
export const SAMPLE_RATE = 16000;
export const MAX_SECONDS = 120;

/** samples: mono floats in [-1, 1]. Returns a complete WAV file. */
export function encodeWav(samples, rate = SAMPLE_RATE) {
  const bytes = samples.length * 2, out = new Uint8Array(44 + bytes), view = new DataView(out.buffer);
  const tag = (offset, text) => { for (let i = 0; i < 4; i++) out[offset + i] = text.charCodeAt(i); };
  tag(0, 'RIFF'); view.setUint32(4, 36 + bytes, true); tag(8, 'WAVE');
  tag(12, 'fmt '); view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, rate, true); view.setUint32(28, rate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  tag(36, 'data'); view.setUint32(40, bytes, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return out;
}

/** blob: a MediaRecorder recording. Resolves to WAV bytes, at most MAX_SECONDS long. Browser only. */
export async function recordingToWav(blob) {
  const context = new AudioContext();
  let decoded;
  try { decoded = await context.decodeAudioData(await blob.arrayBuffer()); }
  catch { throw new Error('The recording couldn’t be read. Try recording again.'); }
  finally { context.close(); }
  const frames = Math.min(Math.ceil(decoded.duration * SAMPLE_RATE), MAX_SECONDS * SAMPLE_RATE);
  if (!frames) throw new Error('No speech detected. Try recording again.');
  // A one-channel destination mixes stereo down to mono as it resamples.
  const offline = new OfflineAudioContext(1, frames, SAMPLE_RATE);
  const source = offline.createBufferSource();
  source.buffer = decoded; source.connect(offline.destination); source.start();
  return encodeWav((await offline.startRendering()).getChannelData(0));
}
