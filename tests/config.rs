use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDataDir(PathBuf);

impl TestDataDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "upland-config-test-{}-{unique}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn home(&self) -> String {
        self.0.display().to_string()
    }

    fn generated(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        let rel = Path::new("Library/Application Support/upland/config.json");
        #[cfg(target_os = "windows")]
        let rel = Path::new("upland/config.json");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let rel = Path::new("upland/config.json");
        self.0.join(rel)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn env_from(dir: &str, port: &str) -> Vec<(String, String)> {
    vec![
        ("HOME".to_owned(), dir.to_owned()),
        ("XDG_DATA_HOME".to_owned(), dir.to_owned()),
        ("APPDATA".to_owned(), dir.to_owned()),
        ("UPLAND_PORT".to_owned(), port.to_owned()),
    ]
}

#[test]
#[cfg(unix)]
fn a_fresh_boot_writes_the_config_file_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TestDataDir::new("mode");
    let pairs = env_from(&dir.home(), "8111");
    let config = upland::Config::from_args(Vec::<String>::new(), |key| {
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    })
    .expect("fresh boot");

    let metadata = std::fs::metadata(&config.config_path).expect("generated file");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let contents = std::fs::read_to_string(&config.config_path).expect("readable");
    assert!(contents.contains("jwt_secret"));
}

#[test]
fn two_racing_boots_resolve_onto_one_generated_secret() {
    let dir = TestDataDir::new("race");
    let first = {
        let pairs = env_from(&dir.home(), "8112");
        upland::Config::from_args(Vec::<String>::new(), |key| {
            pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
        .expect("first boot")
    };
    let second = {
        let pairs = env_from(&dir.home(), "8112");
        upland::Config::from_args(Vec::<String>::new(), |key| {
            pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
        .expect("second boot")
    };
    assert_eq!(
        first.signing_secret.hex(),
        second.signing_secret.hex(),
        "both boots must mint with the one file on disk"
    );
    assert_eq!(second.config_path, dir.generated());
}

#[test]
fn an_explicit_config_path_is_never_generated() {
    let dir = TestDataDir::new("explicit");
    let target = dir.0.join("nowhere.json");
    let pairs = env_from(&dir.home(), "8113");
    let error = upland::Config::from_args(
        vec!["--config".to_owned(), target.display().to_string()],
        |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
    )
    .expect_err("explicit absent must fail, not create");
    assert!(error.contains("nowhere.json"), "{error}");
    assert!(!target.exists());
}

#[test]
fn two_threads_booting_a_fresh_default_directory_agree_on_one_secret() {
    // The ADR-0005 publish promise under real concurrency: exactly one
    // document survives, and both boots mint with the secret it holds.
    let dir = std::sync::Arc::new(TestDataDir::new("concurrent"));
    let pairs = std::sync::Arc::new(env_from(&dir.home(), "8114"));

    let boots = [0u8, 1].map(|_| {
        let pairs = std::sync::Arc::clone(&pairs);
        std::thread::spawn(move || {
            upland::Config::from_args(Vec::<String>::new(), |key| {
                pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
            })
            .expect("booting thread")
        })
    });
    let [first, second] = boots;
    let first = first.join().expect("boot");
    let second = second.join().expect("boot");

    assert_eq!(first.signing_secret.hex(), second.signing_secret.hex());
    let contents = std::fs::read_to_string(&first.config_path).expect("one file");
    let document: serde_json::Value = serde_json::from_str(&contents).expect("never torn");
    assert_eq!(
        document["auth"]["jwt_secret"].as_str().unwrap(),
        first.signing_secret.hex()
    );
}
