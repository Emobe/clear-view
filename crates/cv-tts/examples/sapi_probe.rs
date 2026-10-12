//! Dev check for the SAPI backend (roadmap 8.7b, ADR 0012). Not shipped.
//!
//! `cargo run -p cv-tts --example sapi_probe -- [--voice N] [--rate 0-100] [--volume 0-100] [--say "text"]`
//!
//! Lists the SAPI voices, speaks a short sentence (or `--say`) and waits for its finished report,
//! then speaks a long sentence and stops it after 500 ms. Every step prints a `[tts]` line with a
//! time in milliseconds, so the stop can be compared with what you hear.

use std::time::{Duration, Instant};

use cv_tts::SapiBackend;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

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

fn wait_finished(backend: &SapiBackend, id: u32, start: Instant, limit: Duration) -> bool {
    let began = Instant::now();
    while began.elapsed() < limit {
        for done in backend.take_finished() {
            println!("[tts] {:>6} ms finished id {done}", start.elapsed().as_millis());
            if done == id {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn run(args: Args) -> Result<(), String> {
    let backend = SapiBackend::new()?;
    let voices = backend.voices();
    if voices.is_empty() {
        return Err("no SAPI voices installed".into());
    }
    for (i, v) in voices.iter().enumerate() {
        println!("[tts] voice {i}: {} ({}) {}", v.name, v.language, v.id.id);
    }
    if let Some(n) = args.voice {
        let voice = voices.get(n).ok_or(format!("--voice {n}: there are only {} voices", voices.len()))?;
        if !backend.set_voice(&voice.id) {
            return Err(format!("could not set voice {n}"));
        }
        println!("[tts] using voice {n}: {}", voice.name);
    }
    backend.set_rate(args.rate);
    backend.set_volume(args.volume);
    println!("[tts] rate {} volume {}", args.rate, args.volume);

    let start = Instant::now();
    let id = backend.speak(&args.say).ok_or("speak returned no id")?;
    println!("[tts] {:>6} ms speak id {id}: {}", start.elapsed().as_millis(), args.say);
    if !wait_finished(&backend, id, start, Duration::from_secs(15)) {
        return Err("no finished report within 15 s".into());
    }

    let start = Instant::now();
    let id = backend.speak(LONG).ok_or("speak returned no id")?;
    println!("[tts] {:>6} ms speak id {id}: long sentence", start.elapsed().as_millis());
    std::thread::sleep(Duration::from_millis(500));
    backend.stop();
    println!("[tts] {:>6} ms stop called", start.elapsed().as_millis());
    // Anything reported after the stop is a purge report; it must not arrive seconds later.
    wait_finished(&backend, id, start, Duration::from_secs(2));
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
