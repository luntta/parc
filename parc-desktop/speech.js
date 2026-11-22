// Reports lifecycle changes so vault and document navigation cannot redirect a transcript.
export function dictation(api, phase, status, append) {
  let session;
  function stop(cancel = false) {
    if (!session || session.recorder.state === 'inactive') return;
    session.cancelled = cancel;
    phase('stopping'); session.recorder.stop(); session.stream.getTracks().forEach(t => t.stop());
  }
  async function start() {
    phase('starting'); status('Checking local dictation…');
    let stream;
    try {
      await api.speechReady();
      stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      const recorder = new MediaRecorder(stream, { mimeType: 'audio/webm;codecs=opus', audioBitsPerSecond: 64000 });
      const chunks = [], s = session = { recorder, stream };
      recorder.ondataavailable = e => { if (e.data.size) chunks.push(e.data); };
      recorder.onerror = () => { s.failed = true; stop(true); };
      recorder.onstop = async () => {
        clearTimeout(s.timer); stream.getTracks().forEach(t => t.stop()); session = null;
        try {
          if (s.failed) throw new Error('Recording failed. Check your microphone.');
          if (s.cancelled) { status('Recording discarded.'); return; }
          phase('transcribing'); status('Transcribing on this device…');
          append(await api.transcribe(new Uint8Array(await new Blob(chunks).arrayBuffer())));
          status('Review the transcript before saving.');
        } catch (err) { status(err); } finally { phase('idle'); }
      };
      recorder.start(1000); s.timer = setTimeout(() => stop(), 120000);
      phase('recording'); status('Recording. Press Escape to discard it.');
    } catch (err) {
      stream?.getTracks().forEach(t => t.stop()); session = null;
      status(err.name === 'NotAllowedError' ? 'Microphone access denied. Check system settings, or type your note.' : err); phase('idle');
    }
  }
  return { start, stop };
}
