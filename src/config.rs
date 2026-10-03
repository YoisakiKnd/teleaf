use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use directories::ProjectDirs;
use serde_json::{Value, json};

pub const MISSING_DATABASE_KEY: &str = "旧登录数据缺少本地密钥";

pub struct Config {
    api_id: i32,
    api_hash: String,
    database_key: String,
    data_dir: PathBuf,
    session: Option<String>,
}

impl Config {
    pub fn data_dir() -> Result<PathBuf, String> {
        Ok(match env::var_os("TG_DATA_DIR") {
            Some(path) => PathBuf::from(path),
            // Preserve existing sessions when upgrading from the tg-tui name.
            None => ProjectDirs::from("org", "tg-tui", "tg-tui")
                .ok_or("无法确定用户数据目录；请设置 TG_DATA_DIR")?
                .data_local_dir()
                .to_path_buf(),
        })
    }

    pub fn load() -> Result<Option<Self>, String> {
        let data_dir = Self::data_dir()?;
        let saved = read_settings(&data_dir)?;
        let (api_id, api_hash) = credentials(&saved);
        let (Some(api_id), Some(api_hash)) = (api_id, api_hash) else {
            return Ok(None);
        };
        let config = Self::with_credentials(
            &data_dir,
            &api_id,
            &api_hash,
            &saved,
            env::var("TG_DB_KEY").ok(),
        )?;
        if saved.get("database_key").is_none()
            || saved.get("api_id").is_none()
            || saved.get("api_hash").is_none()
        {
            config.save()?;
        }
        Ok(Some(config))
    }

    pub fn save_credentials(api_id: &str, api_hash: &str) -> Result<Self, String> {
        let data_dir = Self::data_dir()?;
        let saved = read_settings(&data_dir)?;
        let config = Self::with_credentials(
            &data_dir,
            api_id,
            api_hash,
            &saved,
            env::var("TG_DB_KEY").ok(),
        )?;
        config.save()?;
        Ok(config)
    }

    pub fn saved_credentials() -> (String, String) {
        let saved = Self::data_dir()
            .and_then(|path| read_settings(&path))
            .unwrap_or_else(|_| json!({}));
        let (id, hash) = credentials(&saved);
        (id.unwrap_or_default(), hash.unwrap_or_default())
    }

    pub fn save_recovery(
        api_id: &str,
        api_hash: &str,
        key: &str,
        fresh: bool,
    ) -> Result<Self, String> {
        Self::recover_at(&Self::data_dir()?, api_id, api_hash, key, fresh)
    }

    fn recover_at(
        data_dir: &Path,
        api_id: &str,
        api_hash: &str,
        key: &str,
        fresh: bool,
    ) -> Result<Self, String> {
        let mut saved = read_settings(data_dir)?;
        if fresh {
            let mut random = [0u8; 16];
            getrandom::getrandom(&mut random)
                .map_err(|error| format!("无法创建新登录：{error}"))?;
            let session: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            saved["session"] = json!(session);
            saved
                .as_object_mut()
                .ok_or("保存的配置格式不正确")?
                .remove("database_key");
        } else {
            if key.is_empty() {
                return Err("请填写旧版本使用的本地密钥；不知道密钥可选择重新登录".into());
            }
            saved["database_key"] = json!(key);
        }
        let config = Self::with_credentials(data_dir, api_id, api_hash, &saved, None)?;
        // A new session leaves the old database and downloads in place. Preserve
        // the previous config too, so its key and session path remain recoverable.
        if fresh {
            let directory = config.session_directory();
            create_private_dir(&directory)?;
            match fs::read(data_dir.join("config.json")) {
                Ok(bytes) => {
                    let path = directory.join("previous-config.json");
                    let mut options = fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    let mut file = options
                        .open(path)
                        .map_err(|error| format!("无法保留旧配置：{error}"))?;
                    file.write_all(&bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|error| format!("无法保留旧配置：{error}"))?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("无法保留旧配置：{error}")),
            }
        }
        config.save()?;
        Ok(config)
    }

    fn with_credentials(
        data_dir: &Path,
        api_id: &str,
        api_hash: &str,
        saved: &Value,
        environment_key: Option<String>,
    ) -> Result<Self, String> {
        let api_id = api_id
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or("API ID 应为大于零的数字")?;
        let api_hash = api_hash.trim();
        if api_hash.len() != 32 || !api_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("API Hash 应为官网提供的 32 位字符，请完整粘贴".into());
        }
        let session = match saved.get("session").and_then(Value::as_str) {
            Some(value)
                if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Some(value.to_owned())
            }
            Some(_) => return Err("保存的登录目录格式不正确".into()),
            None => None,
        };
        let directory = session.as_ref().map_or_else(
            || data_dir.to_path_buf(),
            |session| data_dir.join("sessions").join(session),
        );
        let database_key = match saved
            .get("database_key")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|key| !key.is_empty())
            .or_else(|| environment_key.filter(|key| !key.is_empty()))
        {
            Some(key) => key,
            None => {
                match fs::read_dir(directory.join("tdlib")) {
                    Ok(entries) => {
                        for entry in entries {
                            let entry =
                                entry.map_err(|error| format!("无法检查账号数据库：{error}"))?;
                            if entry.file_name() != ".DS_Store" {
                                return Err(MISSING_DATABASE_KEY.into());
                            }
                        }
                    }
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(format!("无法检查账号数据库：{error}"));
                    }
                    _ => {}
                }
                let mut random = [0u8; 32];
                getrandom::getrandom(&mut random)
                    .map_err(|error| format!("无法生成本地密钥：{error}"))?;
                STANDARD.encode(random)
            }
        };
        Ok(Self {
            api_id,
            api_hash: api_hash.into(),
            database_key,
            data_dir: data_dir.into(),
            session,
        })
    }

    fn save(&self) -> Result<(), String> {
        create_private_dir(&self.data_dir)?;
        let path = self.data_dir.join("config.json");
        let temporary = self
            .data_dir
            .join(format!("config-{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let payload = json!({
            "api_id": self.api_id, "api_hash": self.api_hash, "database_key": self.database_key,
            "session": self.session
        });
        let result = (|| -> std::io::Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(payload.to_string().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|error| format!("无法保存配置：{error}"))
    }

    pub fn api_id(&self) -> i32 {
        self.api_id
    }
    pub fn api_hash(&self) -> &str {
        &self.api_hash
    }
    pub fn directory(&self) -> &Path {
        &self.data_dir
    }

    fn session_directory(&self) -> PathBuf {
        self.session.as_ref().map_or_else(
            || self.data_dir.clone(),
            |session| self.data_dir.join("sessions").join(session),
        )
    }

    pub fn tdlib_parameters(&self, legacy: bool) -> Result<Value, String> {
        let directory = self.session_directory();
        let database_dir = directory.join("tdlib");
        let files_dir = directory.join("files");
        create_private_dir(&database_dir)?;
        create_private_dir(&files_dir)?;
        let mut parameters = json!({
            "use_test_dc": false,
            "database_directory": database_dir,
            "files_directory": files_dir,
            "use_file_database": true,
            "use_chat_info_database": true,
            "use_message_database": true,
            "use_secret_chats": true,
            "api_id": self.api_id,
            "api_hash": self.api_hash,
            "system_language_code": "zh-CN",
            "device_model": "Teleaf",
            "system_version": env::consts::OS,
            "application_version": env!("CARGO_PKG_VERSION")
        });
        if legacy {
            parameters["@type"] = json!("tdlibParameters");
            parameters["enable_storage_optimizer"] = json!(true);
            parameters["ignore_file_names"] = json!(false);
            Ok(json!({
                "@type": "setTdlibParameters", "parameters": parameters,
                "@extra": "set-tdlib-parameters-legacy"
            }))
        } else {
            parameters["@type"] = json!("setTdlibParameters");
            parameters["database_encryption_key"] =
                json!(STANDARD.encode(self.database_key.as_bytes()));
            parameters["@extra"] = json!("set-tdlib-parameters");
            Ok(parameters)
        }
    }

    pub fn encryption_key_request(&self) -> Value {
        json!({
            "@type": "checkDatabaseEncryptionKey",
            "encryption_key": STANDARD.encode(self.database_key.as_bytes()),
            "@extra": "database-key"
        })
    }
}

fn credentials(saved: &Value) -> (Option<String>, Option<String>) {
    let id = saved
        .get("api_id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .or_else(|| env::var("TG_API_ID").ok());
    let hash = saved
        .get("api_hash")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| env::var("TG_API_HASH").ok());
    (id, hash)
}

fn read_settings(data_dir: &Path) -> Result<Value, String> {
    match fs::read(data_dir.join("config.json")) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "保存的配置无法读取，请检查 config.json".to_owned())?;
            if !value.is_object() {
                return Err("保存的配置格式不正确，请检查 config.json".into());
            }
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("无法读取配置：{error}")),
    }
}

fn create_private_dir(path: &PathBuf) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|error| format!("无法创建数据目录 {}：{error}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("无法设置数据目录权限 {}：{error}", path.display()))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)
        .map_err(|error| format!("无法创建数据目录 {}：{error}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_dir() -> PathBuf {
        static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "tg-tui-settings-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn saved_credentials_keep_key_and_private_permissions() {
        let path = temporary_dir();
        let config = Config::with_credentials(
            &path,
            "123",
            "0123456789abcdef0123456789abcdef",
            &json!({}),
            None,
        )
        .expect("create config");
        assert_eq!(
            STANDARD
                .decode(&config.database_key)
                .expect("random key")
                .len(),
            32
        );
        config.save().expect("save");
        let saved = read_settings(&path).expect("read back");
        let reloaded = Config::with_credentials(
            &path,
            "456",
            "abcdef0123456789abcdef0123456789",
            &saved,
            None,
        )
        .expect("edit config");
        assert_eq!(config.database_key, reloaded.database_key);
        reloaded.save().expect("save edited");
        assert_eq!(read_settings(&path).expect("read edited")["api_id"], 456);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path.join("config.json"))
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn existing_database_requires_its_original_key() {
        let path = temporary_dir();
        fs::create_dir_all(path.join("tdlib")).expect("mkdir");
        fs::write(path.join("tdlib/td.binlog"), "existing database").expect("fixture");
        let hash = "0123456789abcdef0123456789abcdef";
        let error = Config::with_credentials(&path, "123", hash, &json!({}), None)
            .err()
            .expect("key missing");
        assert_eq!(error, MISSING_DATABASE_KEY);
        assert_eq!(
            Config::with_credentials(
                &path,
                "123",
                hash,
                &json!({"database_key":""}),
                Some(String::new())
            )
            .err()
            .unwrap(),
            MISSING_DATABASE_KEY
        );
        assert!(!path.join("config.json").exists());
        let config =
            Config::with_credentials(&path, "123", hash, &json!({}), Some("original-key".into()))
                .expect("migrate legacy key");
        config.save().expect("save");
        assert_eq!(
            read_settings(&path).expect("read")["database_key"],
            "original-key"
        );
        fs::remove_dir_all(path).expect("cleanup");
    }

    #[test]
    fn fresh_session_preserves_old_data_and_config_and_survives_reload() {
        let path = temporary_dir();
        fs::create_dir_all(path.join("tdlib")).unwrap();
        fs::create_dir_all(path.join("files")).unwrap();
        fs::write(path.join("tdlib/td.binlog"), b"old encrypted database").unwrap();
        fs::write(path.join("files/local-only"), b"old download").unwrap();
        let previous = json!({"api_id":123,"api_hash":"0123456789abcdef0123456789abcdef","database_key":"previous-key"});
        fs::write(path.join("config.json"), previous.to_string()).unwrap();
        let config =
            Config::recover_at(&path, "123", "0123456789abcdef0123456789abcdef", "", true).unwrap();
        let session = config.session_directory();
        assert_ne!(session, path);
        assert_eq!(read_settings(&session).unwrap(), json!({}));
        assert_eq!(
            fs::read(session.join("previous-config.json")).unwrap(),
            previous.to_string().as_bytes()
        );
        let params = config.tdlib_parameters(false).unwrap();
        assert_eq!(params["database_directory"], json!(session.join("tdlib")));
        assert_eq!(
            fs::read(path.join("tdlib/td.binlog")).unwrap(),
            b"old encrypted database"
        );
        assert_eq!(
            fs::read(path.join("files/local-only")).unwrap(),
            b"old download"
        );
        let saved = read_settings(&path).unwrap();
        let reloaded = Config::with_credentials(
            &path,
            "123",
            "0123456789abcdef0123456789abcdef",
            &saved,
            Some("stale-environment-key".into()),
        )
        .unwrap();
        assert_eq!(config.database_key, reloaded.database_key);
        assert_eq!(reloaded.session_directory(), session);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(session.join("previous-config.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn recovery_keeps_exact_key_and_ignores_macos_folder_metadata() {
        let path = temporary_dir();
        fs::create_dir_all(path.join("tdlib")).unwrap();
        fs::write(path.join("tdlib/.DS_Store"), b"metadata").unwrap();
        let hash = "0123456789abcdef0123456789abcdef";
        assert!(Config::with_credentials(&path, "123", hash, &json!({}), None).is_ok());
        fs::write(path.join("tdlib/td.binlog"), b"database").unwrap();
        assert!(Config::recover_at(&path, "123", hash, "", false).is_err());
        assert!(!path.join("config.json").exists());
        assert!(Config::recover_at(&path, "bad-id", hash, "", true).is_err());
        assert!(!path.join("sessions").exists());
        let config = Config::recover_at(&path, "123", hash, " original key ", false).unwrap();
        assert_eq!(config.database_key, " original key ");
        assert_eq!(config.session_directory(), path);
        assert_eq!(fs::read(path.join("tdlib/td.binlog")).unwrap(), b"database");
        assert_eq!(
            read_settings(&path).unwrap()["database_key"],
            " original key "
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn tdlib_parameter_formats_keep_the_same_credentials() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let data_dir =
            std::env::temp_dir().join(format!("tg-tui-config-{}-{nonce}", std::process::id()));
        let config = Config {
            api_id: 123,
            api_hash: "test-hash".into(),
            database_key: "secret".into(),
            data_dir: data_dir.clone(),
            session: None,
        };
        let modern = config.tdlib_parameters(false).expect("modern parameters");
        let legacy = config.tdlib_parameters(true).expect("legacy parameters");
        assert_eq!(modern["api_id"], 123);
        assert_eq!(legacy["parameters"]["api_id"], 123);
        assert_eq!(legacy["parameters"]["@type"], "tdlibParameters");
        assert_eq!(modern["database_encryption_key"], STANDARD.encode("secret"));
        assert_eq!(
            config.encryption_key_request()["encryption_key"],
            STANDARD.encode("secret")
        );
        let _ = fs::remove_dir_all(data_dir);
    }
}
