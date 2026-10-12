//! Plays 16-bit PCM on the default output device with WASAPI (ADR 0012, ADR 0013). Shared mode,
//! event driven: the owner waits on the event it gave the player, and calls [`Player::pump`] each
//! time it is signalled. The player opens the default render endpoint when it starts, so a changed
//! default device is used by the next utterance.

use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

use crate::event::Event;
use crate::wav::WavPcm;

pub struct Player {
    client: IAudioClient,
    render: IAudioRenderClient,
    buffer_frames: u32,
    frame_bytes: usize,
    pcm: Vec<u8>,
    written: usize,
}

impl Player {
    /// Opens the default output device for `wav`'s format and starts playing `pcm` (the sample
    /// data, a whole number of frames). `event` is signalled each time the device wants data.
    pub fn start(wav: &WavPcm, pcm: Vec<u8>, event: &Event) -> Result<Self, String> {
        let frame_bytes = wav.frame_bytes();
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: wav.channels,
            nSamplesPerSec: wav.sample_rate,
            nAvgBytesPerSec: wav.sample_rate * frame_bytes as u32,
            nBlockAlign: frame_bytes as u16,
            wBitsPerSample: wav.bits_per_sample,
            cbSize: 0,
        };
        let step = |what: &str, e: windows::core::Error| format!("{what} failed: {e}");
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| step("CoCreateInstance MMDeviceEnumerator", e))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| step("GetDefaultAudioEndpoint", e))?;
            let client: IAudioClient =
                device.Activate(CLSCTX_ALL, None).map_err(|e| step("Activate IAudioClient", e))?;
            // Event driven and shared: both durations are 0 (IAudioClient::Initialize). The engine
            // converts our format to the device's.
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    0,
                    0,
                    &format,
                    None,
                )
                .map_err(|e| step("IAudioClient::Initialize", e))?;
            client.SetEventHandle(event.handle()).map_err(|e| step("SetEventHandle", e))?;
            let buffer_frames = client.GetBufferSize().map_err(|e| step("GetBufferSize", e))?;
            let render: IAudioRenderClient = client.GetService().map_err(|e| step("GetService", e))?;
            let mut player = Self { client, render, buffer_frames, frame_bytes, pcm, written: 0 };
            // Fill the buffer before starting so the first period is not silence.
            player.fill()?;
            player.client.Start().map_err(|e| step("IAudioClient::Start", e))?;
            Ok(player)
        }
    }

    /// Writes what fits in the device buffer. Returns true once all of the audio has been played.
    pub fn pump(&mut self) -> Result<bool, String> {
        self.fill()?;
        if self.written < self.pcm.len() {
            return Ok(false);
        }
        let padding = unsafe { self.client.GetCurrentPadding() }
            .map_err(|e| format!("GetCurrentPadding failed: {e}"))?;
        Ok(padding == 0)
    }

    fn fill(&mut self) -> Result<(), String> {
        let padding = unsafe { self.client.GetCurrentPadding() }
            .map_err(|e| format!("GetCurrentPadding failed: {e}"))?;
        let free = self.buffer_frames.saturating_sub(padding) as usize;
        let left = (self.pcm.len() - self.written) / self.frame_bytes;
        let frames = free.min(left);
        if frames == 0 {
            return Ok(());
        }
        let bytes = frames * self.frame_bytes;
        unsafe {
            let dest = self.render.GetBuffer(frames as u32).map_err(|e| format!("GetBuffer failed: {e}"))?;
            std::ptr::copy_nonoverlapping(self.pcm[self.written..].as_ptr(), dest, bytes);
            self.render.ReleaseBuffer(frames as u32, 0).map_err(|e| format!("ReleaseBuffer failed: {e}"))?;
        }
        self.written += bytes;
        Ok(())
    }

    /// Stops the audio at once and drops what is buffered.
    pub fn stop(self) {
        unsafe {
            let _ = self.client.Stop();
            let _ = self.client.Reset();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // After `stop` this is a second Stop on a stopped stream, which WASAPI allows.
        let _ = unsafe { self.client.Stop() };
    }
}
