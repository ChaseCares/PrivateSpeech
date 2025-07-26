// Private Speech - A text to speech program that uses a local TTS server
// Copyright (C) 2023  Chase C <hi@chasecares.dev>
// SPDX-License-Identifier: GPL-2.0-only

use std::io::{BufReader, Write};
use std::process;
use std::process::Command;
use std::time::Duration;
use std::{ffi::OsStr, fs::File};

use directories::ProjectDirs;
use regex::Regex;
use reqwest::{blocking::Client, StatusCode};
use rodio::{Decoder, OutputStreamBuilder, Sink};
use sysinfo::{Pid, System};

use config::Config;
#[cfg(target_os = "linux")]
use menu::Menu;

mod config;
#[cfg(target_os = "linux")]
mod menu;

// Used for substitutions and to remove unwanted test
// TODO: Make this not a return a copy
fn re(regex: &str, haystack: &str, rep: &str) -> String {
    let r = Regex::new(regex).unwrap_or_else(|_| panic!("Invalid regex: {}", regex));
    r.replace_all(haystack, rep).to_string()
}

// Calculate a hash of the text to use as the file name so that
// if we encountered the same string again, we can just use the same file. speeeeeed
fn calculate_hash<T: std::hash::Hash>(text: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    std::hash::Hasher::finish(&hasher)
}

// Get the clipboard contents (Wayland exclusiveley)
fn get_clipboard() -> String {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().unwrap();
    let text = clipboard.get_text().unwrap();
    println!("Clipboard text was: {}", text);
    text
}

// Get the audio from the local coqui-ai TTS server
fn get_audio(
    input_text: &str,
    base_url: &str,
    speaker_id: &str,
    output_path: &str,
    timeout: Duration,
) -> Result<(), StatusCode> {
    // input_text should never be empty
    input_text
        .is_empty()
        .then(|| panic!("get_audio was passed an empty string"));

    let full_url = format!("{base_url}api/tts");
    let params = vec![("text", input_text), ("speaker_id", speaker_id)];

    let client = Client::builder().timeout(timeout).build().unwrap();

    let response = client.post(full_url).form(&params).send().unwrap();

    if response.status().is_success() {
        let mut file = File::create(output_path).unwrap();
        let content = response.bytes().unwrap();
        file.write_all(&content).unwrap();
        Ok(())
    } else {
        Err(response.status())
    }
}

// Process the text, stripping unwanted text and replacing text with substitutions
fn process_text(
    mut input: String,
    substitutions: &[(String, String)],
    strip_regex: &[String],
) -> String {
    if !strip_regex.is_empty() {
        input = re(&strip_regex.join("|"), &input, "");
    }

    for (regex, rep) in substitutions {
        input = re(regex, &input, rep);
    }

    input.trim().to_owned()
}

// Split the text into chunks that are roughly min_length long
fn chunk_text<'a>(
    mut text: &'a str,
    min_length: usize,
    quick_first: bool,
    quick_first_length: usize,
    split_on: &'a [char],
) -> Vec<&'a str> {
    text = text.trim();
    let mut spaces = 0;
    let mut chunks: Vec<&str> = vec![];

    // If the text is short, just read it all
    if text.len() < min_length {
        chunks.push(text);
    } else {
        // Create a tiny first chunk to get the audio playing quickly
        if quick_first {
            let mut first_chunk_point = 0;
            text.chars().any(|c| {
                first_chunk_point += c.len_utf8();
                if c == ' ' {
                    spaces += 1;
                }
                if spaces == quick_first_length {
                    return true;
                }
                false
            });
            chunks = vec![text[..first_chunk_point].trim()];
            text = &text[first_chunk_point..];
        }

        let mut start: usize = 0;
        let mut length: usize = 0;
        for slice in text.split_inclusive(|c| split_on.contains(&c)) {
            length += slice.len();

            if length > min_length {
                chunks.push(text[start..(start + length)].trim());
                start += length;
                length = 0;
            }
        }
        if length > 0 {
            chunks.push(text[start..(start + length)].trim());
        }
    }

    chunks
}

// Modify the speed of the audio file, requires ffmpeg
// Verify with: `ffmpeg -filters | grep atempo`. Tested with ffmpeg 6
fn modify_speed(clip_path: String, speed: f32) -> Result<(), std::io::Error> {
    let tmp_path = clip_path.replace(".wav", ".tmp.wav");
    std::fs::rename(&clip_path, &tmp_path)?;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");
    cmd.arg("-loglevel");
    cmd.arg("quiet");
    cmd.arg("-i");
    cmd.arg(&tmp_path);
    cmd.arg("-filter:a");
    cmd.arg(format!("atempo={}", speed));
    cmd.arg("-vn");
    cmd.arg(clip_path);

    cmd.output()
        .map(|_| std::fs::remove_file(tmp_path).unwrap())
}

#[cfg(target_os = "linux")]
fn menu_update(handle: ksni::Handle<Menu>, sink: &Sink) {
    handle.update(|tray: &mut Menu| {
        tray.status = if tray.playing {
            sink.play();
            "Playing".into()
        } else {
            sink.pause();
            "Paused".into()
        };
    });
}

fn main() {
    const APP_NAME: &str = "private_speech";

    let sys = System::new_all();
    let mut previous_process: Option<&sysinfo::Process> = None;
    let this_process = Pid::from_u32(process::id());

    for process in sys.processes_by_name(OsStr::new(APP_NAME)) {
        if process.pid() != this_process {
            previous_process = Some(process);
        }
    }

    if let Some(process) = previous_process {
        process.kill();
    } else {
        // Audio output
        let stream_handle = OutputStreamBuilder::open_default_stream().unwrap();
        let sink = Sink::connect_new(stream_handle.mixer());

        // Tray icon
        #[cfg(target_os = "linux")]
        let handle: Option<ksni::Handle<Menu>>;
        #[cfg(target_os = "linux")]
        {
            let service = ksni::TrayService::new(Menu {
                playing: true,
                status: "Playing".into(),
            });

            handle = Some(service.handle());
            service.spawn();
        }

        // Config
        // https://github.com/dirs-dev/directories-rs#example
        // Config location for Linux: /home/user/.config/private_speech
        let project_dirs = ProjectDirs::from("dev", "chasecares", APP_NAME).unwrap();
        let config_path = project_dirs.config_dir().to_str().unwrap();
        let config_file = format!("{}/config.toml", config_path);

        let file_content = match std::fs::read_to_string(config_file.clone()) {
            Ok(content) => content,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    println!(
                        "Config file not found, please create one at: {}",
                        config_file
                    );
                    println!("See example config at TODO")
                } else {
                    println!("Error reading config file: {}", err);
                }
                std::process::exit(1);
            }
        };

        let config: Config = Config::try_from(file_content.as_str()).unwrap();

        // Create the tmp dir if it doesn't exist
        std::fs::create_dir_all(&config.tmp_dir).unwrap();

        // Get the text from the clipboard and process it
        let input = process_text(
            get_clipboard(),
            &config.substitutions,
            &config.strip_regexes,
        );

        // Get the audio for each chunk of text and append it to the sink to be played
        for text_chunk in &chunk_text(
            &input,
            config.min_length,
            config.quick_first_chunk,
            config.quick_first_chunk_length,
            &config.split_on,
        ) {
            #[cfg(target_os = "linux")]
            menu_update(handle.clone().unwrap(), &sink);

            println!("Playing: {}", text_chunk);
            let audio_path = &format!("{}/{}.wav", config.tmp_dir, calculate_hash(text_chunk));
            // Try to open the file, if it doesn't exist, get it from the server
            match File::open(audio_path) {
                Ok(file) => {
                    sink.append(Decoder::new(BufReader::new(file)).unwrap());
                }
                Err(_) => {
                    get_audio(
                        text_chunk,
                        config.url.as_ref(),
                        config.speaker_id.as_deref().unwrap(),
                        audio_path,
                        Duration::from_secs(config.timeout as u64),
                    )
                    .unwrap_or_else(|status_code| {
                        panic!("Get audio failed with status code: {}", status_code)
                    });

                    if config.playback_speed != 1.0 {
                        match modify_speed(audio_path.to_owned(), config.playback_speed) {
                            Ok(_) => {}
                            Err(err) => {
                                if err.kind() == std::io::ErrorKind::NotFound {
                                    println!(
                                        "ffmpeg not found, please install it to modify playback speed"
                                    );
                                } else {
                                    println!("Error modifying playback speed: {}", err);
                                }
                                std::process::exit(1);
                            }
                        }
                    }

                    sink.append(
                        Decoder::new(BufReader::new(File::open(audio_path).unwrap())).unwrap(),
                    );
                }
            }
        }
        // Play the audio until there's nothing left to play or exited via the menu
        #[cfg(target_os = "linux")]
        while !sink.empty() {
            // Update the menu tool tip text and play/pause status
            menu_update(handle.clone().unwrap(), &sink);
            std::thread::sleep(Duration::from_millis(200));
        }

        // Play the audio until there's nothing left to play
        #[cfg(not(target_os = "linux"))]
        sink.sleep_until_end();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash() {
        let text = "This is a test";
        let hash = super::calculate_hash(&text);
        assert_eq!(hash, 10995228888654166610);
    }

    #[test]
    fn clipboard() {
        use arboard::Clipboard;
        let text = "This is a test";

        let mut clipboard = Clipboard::new().unwrap();
        clipboard.set_text(text).unwrap();

        let output = super::get_clipboard();
        assert_eq!(output, text);

        assert_ne!(output, "This is not a test");
    }

    #[test]
    fn process_text() {
        let input = "This is, a test";
        let substitutions = vec![("test".to_string(), "toast".to_string())];
        let strip_regex = vec![",".to_string()];
        let output = super::process_text(input.to_string(), &substitutions, &strip_regex);
        assert_eq!(output, "This is a toast");
    }

    #[test]
    fn chunk_text() {
        let mut input = "This is, a test";
        let mut min_length = 5;
        let mut quick_first = false;
        let quick_first_length = 1;
        let split_on = vec!['.', ',', '\n'];
        // Tests if text.len() < min_length
        let output = super::chunk_text(
            input,
            min_length,
            quick_first,
            quick_first_length,
            &split_on,
        );
        assert_eq!(output, vec!["This is,", "a test"]);

        // Tests if text.len() > min_length
        min_length = 10;
        let output = super::chunk_text(
            input,
            min_length,
            quick_first,
            quick_first_length,
            &split_on,
        );
        assert_eq!(output, vec![input]);

        // Tests if quick_first is true and if text.len() < min_length
        quick_first = true;
        let output = super::chunk_text(
            input,
            min_length,
            quick_first,
            quick_first_length,
            &split_on,
        );
        assert_eq!(output, vec!["This", "is, a test"]);

        // Tests if quick_first is true, if text.len() > min_length and if length > 0
        input = "This is a long test that should, be split into multiple chunks, and should be split on punctuation.";
        let output = super::chunk_text(
            input,
            min_length,
            quick_first,
            quick_first_length,
            &split_on,
        );
        assert_eq!(
            output,
            vec![
                "This",
                "is a long test that should,",
                "be split into multiple chunks,",
                "and should be split on punctuation.",
            ]
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn menu() {
        use rodio::{OutputStreamBuilder, Sink};

        let service = ksni::TrayService::new(super::Menu {
            playing: true,
            status: "Testing".into(),
        });

        let handle = service.handle();
        service.spawn();

        let stream_handle = OutputStreamBuilder::open_default_stream().unwrap();
        let sink = Sink::connect_new(stream_handle.mixer());

        handle.update(|tray: &mut super::Menu| {
            assert!(tray.playing);
            assert_eq!(tray.status, "Testing");
        });

        super::menu_update(handle.clone(), &sink);

        handle.update(|tray: &mut super::Menu| {
            tray.playing = false;
            assert!(!tray.playing);
            assert_eq!(tray.status, "Playing");
        });

        handle.shutdown();
    }

    fn load_config() -> super::Config {
        use directories::ProjectDirs;

        use crate::config::Config;

        let project_dirs = ProjectDirs::from("com", "chasecares", "private_speech").unwrap();
        let config_path = project_dirs.config_dir().to_str().unwrap();
        let config_file = format!("{}/config.toml", config_path);
        let file_content = std::fs::read_to_string(config_file).unwrap();
        let config: Config = super::Config::try_from(file_content.as_str()).unwrap();

        config
    }

    fn now() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    }

    #[test]
    fn audio_and_config() {
        let config = load_config();
        assert_ne!(
            config.url,
            url::Url::parse("http://[0100::0]:5002").unwrap()
        );

        let audio_path = "/tmp/test.wav";

        let mut result = super::get_audio(
            now().to_string().as_str(),
            config.url.as_ref(),
            config.speaker_id.as_deref().unwrap(),
            audio_path,
            std::time::Duration::from_secs(config.timeout as u64),
        );
        assert!(result.is_ok());

        let metadata = std::fs::metadata(audio_path).unwrap().len();
        assert!(metadata > 20000);

        result = super::get_audio(
            now().to_string().as_str(),
            config.url.as_ref(),
            config.speaker_id.as_deref().unwrap(),
            audio_path,
            std::time::Duration::from_secs(config.timeout as u64),
        );

        assert!(result.is_ok());
        assert_ne!(metadata, std::fs::metadata(audio_path).unwrap().len());
    }
}
