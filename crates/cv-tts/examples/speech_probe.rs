//! Dev check for both speech engines (roadmap 8.7c, ADR 0012). Not shipped.
//!
//! `cargo run -p cv-tts --example speech_probe -- [--voice N] [--rate 0-100] [--volume 0-100] [--say "text"]`
//!
//! Lists the voices of both engines (OneCore first), says which one `choose_voice` would pick,
//! speaks a short sentence (or `--say`) and waits for its finished report, then speaks a long
//! sentence and stops it after 500 ms. It waits on the backends' own handles, the way the speech
//! thread will (ADR 0013). Every step prints a `[tts]` line with a time in milliseconds, so the
//! stop can be compared with what you hear.

use std::time::{Duration, Instant};

use cv_core::voice::{VoiceChoice, choose_voice};
use cv_tts::{Engines, SpeechBackend, display_language};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::WaitForMultipleObjects;

const LONG: &str = "This is a long sentence that goes on and on, so that there is plenty of speech \
left to stop. It keeps going for several seconds, and the only thing that should end it early is \
the stop call that this probe makes half a second after it starts speaking.";

struct Args {
    voice: Option<usize>,
    rate: u32,
    volume: u32,
    say: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { voice: None, rate: 50, volume: 100, say: "Hello, this is the clear view voice test.".into() };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        let number = |v: String| v.parse::<u32>().map_err(|_| format!("{flag}: not a number: {v}"));
        match flag.as_str() {
            "--voice" => args.voice = Some(number(value()?)? as usize),
            "--rate" => args.rate = number(value()?)?,
            "--volume" => args.volume = number(value()?)?,
            "--say" => args.say = value()?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

/// Waits on the engines' handles (at most 20 ms at a time, as a backstop) until `id` is reported.
fn wait_finished(engines: &mut Engines, id: u32, start: Instant, limit: Duration) -> bool {
    let began = Instant::now();
    while began.elapsed() < limit {
        let handles = engines.wait_handles();
        // Which handle fired, or that 20 ms passed, does not matter: either way, ask for reports.
        unsafe { WaitForMultipleObjects(&handles, false, 20) };
        for done in engines.take_finished() {
            println!("[tts] {:>6} ms finished id {done}", start.elapsed().as_millis());
            if done == id {
                return true;
            }
        }
    }
    false
}

fn run(args: Args) -> Result<(), String> {
    let mut engines = Engines::new()?;
    let voices = engines.voices();
    if voices.is_empty() {
        return Err("no voices installed".into());
    }
    for (i, v) in voices.iter().enumerate() {
        println!("[tts] voice {i}: {:?} {} ({}) {}", v.id.engine, v.name, v.language, v.id.id);
    }
    let default = engines.onecore_default();
    let language = display_language();
    let choice = choose_voice(None, &voices, default.as_ref(), &language);
    let chosen = match &choice {
        VoiceChoice::Use { voice, .. } => voice.clone(),
        VoiceChoice::NoVoices => return Err("choose_voice found no voices".into()),
    };
    println!("[tts] display language {language:?}; choose_voice picks {:?} {}", chosen.id.engine, chosen.name);
    let voice = match args.voice {
        Some(n) => voices.get(n).ok_or(format!("--voice {n}: there are only {} voices", voices.len()))?,
        None => &chosen,
    };
    if !engines.set_voice(&voice.id) {
        return Err(format!("could not set voice {}", voice.name));
    }
    println!("[tts] using {:?} {}", voice.id.engine, voice.name);
    engines.set_rate(args.rate);
    engines.set_volume(args.volume);
    println!("[tts] rate {} volume {}", args.rate, args.volume);

    let start = Instant::now();
    let id = 1;
    if !engines.speak(id, &args.say) {
        return Err("speak was refused".into());
    }
    println!("[tts] {:>6} ms speak id {id}: {}", start.elapsed().as_millis(), args.say);
    if !wait_finished(&mut engines, id, start, Duration::from_secs(20)) {
        return Err("no finished report within 20 s".into());
    }

    let start = Instant::now();
    let id = 2;
    if !engines.speak(id, LONG) {
        return Err("speak was refused".into());
    }
    println!("[tts] {:>6} ms speak id {id}: long sentence", start.elapsed().as_millis());
    // Let it start (synthesis takes a moment), then stop 500 ms after the call.
    let until = Duration::from_millis(500);
    while start.elapsed() < until {
        let handles = engines.wait_handles();
        unsafe { WaitForMultipleObjects(&handles, false, 20) };
        engines.take_finished();
    }
    engines.stop();
    println!("[tts] {:>6} ms stop called", start.elapsed().as_millis());
    // A stopped utterance is never reported, so nothing should print here.
    wait_finished(&mut engines, id, start, Duration::from_secs(2));
    println!("[tts] {:>6} ms done", start.elapsed().as_millis());
    Ok(())
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[tts] {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        eprintln!("[tts] CoInitializeEx failed: {e}");
        std::process::exit(1);
    }
    let result = run(args);
    unsafe { CoUninitialize() };
    if let Err(e) = result {
        eprintln!("[tts] {e}");
        std::process::exit(1);
    }
}
