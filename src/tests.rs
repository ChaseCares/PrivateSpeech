use super::*;

#[cfg(test)]
mod test_suite {
    #[test]
    fn hash() {
        let text = "This is a test";
        let hash = super::calculate_hash(&text);
        assert_eq!(hash, 10_995_228_888_654_166_610);
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

        super::menu_update(&handle.clone(), &sink);

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
        let config_file = format!("{config_path}/config.toml");
        let file_content = std::fs::read_to_string(config_file).unwrap();
        let config: Config = Config::try_from(file_content.as_str()).unwrap();

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
