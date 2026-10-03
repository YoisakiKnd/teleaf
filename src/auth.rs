use crossterm::event::KeyCode;
use serde_json::{Value, json};

use crate::config::Config;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prompt {
    None,
    Phone,
    Code,
    Password,
    Email,
    EmailCode,
    FirstName,
    LastName,
}

pub struct SetupForm {
    pub api_id: String,
    pub api_hash: String,
    pub focused: usize,
    pub database_key: String,
    pub recovering: bool,
    pub new_login: bool,
}

impl SetupForm {
    pub fn field(&self, index: usize) -> &str {
        match index {
            0 => &self.api_id,
            1 => &self.api_hash,
            _ => &self.database_key,
        }
    }
    fn field_mut(&mut self) -> &mut String {
        match self.focused {
            0 => &mut self.api_id,
            1 => &mut self.api_hash,
            _ => &mut self.database_key,
        }
    }
}

pub struct AuthFlow {
    config: Option<Config>,
    initial_credentials: (String, String),
    pub setup: Option<SetupForm>,
    restart: bool,
    tried_legacy_parameters: bool,
    prompt: Prompt,
    input: String,
    pub cursor: usize,
    pub setup_cursors: [usize; 3],
    first_name: String,
    pub state: String,
    pub message: String,
    pub is_error: bool,
    pub detail: String,
    pub confirmation_link: Option<String>,
}

impl AuthFlow {
    pub fn new() -> Self {
        let loaded = Config::load();
        let mut flow = Self::empty();
        match loaded {
            Ok(Some(config)) => flow.config = Some(config),
            Ok(None) => {
                flow.initial_credentials = Config::saved_credentials();
                flow.begin_setup();
            }
            Err(error) => {
                flow.initial_credentials = Config::saved_credentials();
                flow.begin_setup();
                if error == crate::config::MISSING_DATABASE_KEY {
                    flow.begin_recovery();
                }
                flow.message = if flow.is_recovering() {
                    String::new()
                } else {
                    error
                };
                flow.is_error = !flow.message.is_empty();
            }
        }
        flow
    }

    pub(crate) fn empty() -> Self {
        Self {
            config: None,
            initial_credentials: (String::new(), String::new()),
            setup: None,
            restart: false,
            tried_legacy_parameters: false,
            prompt: Prompt::None,
            input: String::new(),
            cursor: usize::MAX,
            setup_cursors: [usize::MAX; 3],
            first_name: String::new(),
            state: String::new(),
            message: String::new(),
            is_error: false,
            detail: String::new(),
            confirmation_link: None,
        }
    }

    pub fn cancel_setup(&mut self) -> bool {
        if let Some(form) = &mut self.setup {
            if form.new_login {
                form.new_login = false;
                form.focused = 2;
                self.clear_message();
                return true;
            }
            if form.recovering {
                form.recovering = false;
                form.focused = 0;
                self.clear_message();
                return true;
            }
        }
        if self.config.is_some() && self.setup.is_some() {
            self.setup = None;
            self.clear_message();
            true
        } else {
            false
        }
    }

    pub fn begin_setup(&mut self) {
        self.setup = Some(SetupForm {
            api_id: self
                .config
                .as_ref()
                .map(|config| config.api_id().to_string())
                .unwrap_or_else(|| self.initial_credentials.0.clone()),
            api_hash: self
                .config
                .as_ref()
                .map(|config| config.api_hash().to_owned())
                .unwrap_or_else(|| self.initial_credentials.1.clone()),
            focused: 0,
            database_key: String::new(),
            recovering: false,
            new_login: false,
        });
        self.setup_cursors = [usize::MAX; 3];
        self.clear_message();
    }

    pub fn is_recovering(&self) -> bool {
        self.setup.as_ref().is_some_and(|form| form.recovering)
    }

    pub fn is_new_login(&self) -> bool {
        self.setup.as_ref().is_some_and(|form| form.new_login)
    }

    fn begin_recovery(&mut self) {
        if self.setup.is_none() {
            self.begin_setup();
        }
        if let Some(form) = &mut self.setup {
            form.recovering = true;
            form.new_login = false;
            form.focused = 2;
            form.database_key.clear();
        }
        self.clear_message();
    }

    pub fn choose_new_login(&mut self) {
        if let Some(form) = &mut self.setup
            && form.recovering
        {
            form.new_login = true;
        }
        self.clear_message();
    }

    fn save_setup(&mut self) {
        let Some(form) = &self.setup else {
            return;
        };
        let result = if form.recovering {
            Config::save_recovery(
                &form.api_id,
                &form.api_hash,
                &form.database_key,
                form.new_login,
            )
        } else {
            Config::save_credentials(&form.api_id, &form.api_hash)
        };
        match result {
            Ok(config) => {
                self.config = Some(config);
                self.setup = None;
                self.restart = true;
                self.clear_message();
            }
            Err(error) if error == crate::config::MISSING_DATABASE_KEY => self.begin_recovery(),
            Err(error) => {
                if let Some(form) = &mut self.setup
                    && (error.contains("API ID") || error.contains("API Hash"))
                {
                    form.focused = usize::from(error.contains("API Hash"));
                    form.recovering = false;
                    form.new_login = false;
                }
                self.message = error;
                self.is_error = true;
            }
        }
    }

    pub fn take_restart(&mut self) -> bool {
        std::mem::take(&mut self.restart)
    }

    pub fn config_summary(&self) -> (String, String) {
        match &self.config {
            Some(config) => (
                config.api_id().to_string(),
                config.directory().display().to_string(),
            ),
            None => (
                "尚未设置".into(),
                Config::data_dir()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
        }
    }

    pub fn clear_message(&mut self) {
        self.message.clear();
        self.is_error = false;
    }

    pub fn on_update(&mut self, value: &Value) -> Option<Value> {
        match value.get("@type").and_then(Value::as_str) {
            Some("updateAuthorizationState") => {
                let auth = &value["authorization_state"];
                let state = auth["@type"].as_str().unwrap_or("");
                if self.state != state {
                    self.input.clear();
                    self.cursor = usize::MAX;
                }
                self.state = state.into();
                self.prompt = match state {
                    "authorizationStateWaitPhoneNumber" => Prompt::Phone,
                    "authorizationStateWaitCode" => Prompt::Code,
                    "authorizationStateWaitPassword" => Prompt::Password,
                    "authorizationStateWaitEmailAddress" => Prompt::Email,
                    "authorizationStateWaitEmailCode" => Prompt::EmailCode,
                    "authorizationStateWaitRegistration" => Prompt::FirstName,
                    _ => Prompt::None,
                };
                if self.setup.is_none() {
                    self.clear_message();
                }
                self.confirmation_link = auth["link"].as_str().map(str::to_owned);
                self.detail = match state {
                    "authorizationStateWaitPhoneNumber" => {
                        "填写带国家区号的手机号，例如 +86 138…".into()
                    }
                    "authorizationStateWaitCode" => {
                        match auth
                            .pointer("/code_info/type/@type")
                            .and_then(Value::as_str)
                        {
                            Some("authenticationCodeTypeSms") => {
                                "请查看手机短信中的验证码。".into()
                            }
                            Some("authenticationCodeTypeEmailCode") => {
                                "请查看邮箱中的验证码。".into()
                            }
                            _ => "请查看已登录的 Telegram 设备收到的验证码。".into(),
                        }
                    }
                    "authorizationStateWaitPassword" => {
                        let hint = auth["password_hint"].as_str().unwrap_or("");
                        if hint.is_empty() {
                            "此账号启用了两步验证，请输入密码。".into()
                        } else {
                            format!("两步验证密码提示：{hint}")
                        }
                    }
                    "authorizationStateWaitEmailAddress" => "Telegram 需要邮箱来完成验证。".into(),
                    "authorizationStateWaitEmailCode" => format!(
                        "验证码已发送到 {}",
                        auth["email_address_pattern"].as_str().unwrap_or("你的邮箱")
                    ),
                    "authorizationStateWaitOtherDeviceConfirmation" => {
                        "在已登录的 Telegram 设备上打开下面的链接确认登录。".into()
                    }
                    "authorizationStateWaitRegistration" => {
                        "创建账号：先输入名字，再输入姓氏。继续即接受 Telegram 服务条款。".into()
                    }
                    "authorizationStateReady" => "登录成功，正在加载会话。".into(),
                    "authorizationStateClosing" | "authorizationStateClosed" => {
                        "连接已关闭，请重新启动。".into()
                    }
                    _ => "正在连接 Telegram，请稍候…".into(),
                };
                if self.setup.is_some() {
                    return None;
                }
                if state == "authorizationStateWaitTdlibParameters" {
                    return self.parameter_request(false);
                }
                if state == "authorizationStateWaitEncryptionKey" {
                    return self.config.as_ref().map(Config::encryption_key_request);
                }
            }
            Some("error") => {
                let code = value["code"].as_i64().unwrap_or_default();
                if code == 404
                    && value["@extra"]
                        .as_str()
                        .is_some_and(|extra| extra.starts_with("load-chats"))
                {
                    return None;
                }
                if value["@extra"].as_str() == Some("set-tdlib-parameters")
                    && !self.tried_legacy_parameters
                    && value["message"]
                        .as_str()
                        .is_some_and(|text| text.contains("Parameters aren't specified"))
                {
                    self.tried_legacy_parameters = true;
                    return self.parameter_request(true);
                }
                let message = value["message"].as_str().unwrap_or("未知错误");
                let database_request = matches!(
                    value["@extra"].as_str(),
                    Some("set-tdlib-parameters" | "set-tdlib-parameters-legacy" | "database-key")
                );
                if database_request
                    && matches!(message, "Wrong database encryption key" | "Wrong password")
                {
                    self.begin_recovery();
                    self.message = "本地密钥不匹配，请重试；不知道密钥可选择重新登录。".into();
                    self.is_error = true;
                    return None;
                }
                self.message = match message {
                    "UPDATE_APP_TO_LOGIN" => {
                        "当前 TDLib 需要更新，请运行 sh scripts/install-tdlib.sh 后重启。".into()
                    }
                    "PHONE_NUMBER_INVALID" => "手机号格式不正确，请加上国家区号后重试。".into(),
                    "PHONE_CODE_INVALID" => "验证码不正确，请重新输入。".into(),
                    "PHONE_CODE_EXPIRED" => "验证码已过期，请重新启动登录。".into(),
                    "PASSWORD_HASH_INVALID" => "两步验证密码不正确，请重试。".into(),
                    "API_ID_INVALID" => "API 配置不正确，按 F3 重新填写。".into(),
                    _ => format!("请求失败（{code}）：{message}"),
                };
                self.is_error = true;
            }
            _ => {}
        }
        None
    }

    fn parameter_request(&mut self, legacy: bool) -> Option<Value> {
        match self.config.as_ref()?.tdlib_parameters(legacy) {
            Ok(request) => Some(request),
            Err(error) => {
                self.message = error;
                self.is_error = true;
                None
            }
        }
    }

    pub fn title(&self) -> &'static str {
        if self.is_new_login() {
            return "保留旧数据，重新登录";
        }
        if self.is_recovering() {
            return "恢复已有登录";
        }
        if self.setup.is_some() {
            return "欢迎使用 Teleaf";
        }
        match self.prompt {
            Prompt::Phone => "登录 Telegram",
            Prompt::Code => "输入验证码",
            Prompt::Password => "两步验证",
            Prompt::Email | Prompt::EmailCode => "验证邮箱",
            Prompt::FirstName | Prompt::LastName => "创建 Telegram 账号",
            Prompt::None => {
                if self.confirmation_link.is_some() {
                    "确认登录"
                } else {
                    "连接 Telegram"
                }
            }
        }
    }

    pub fn input_label(&self) -> Option<&'static str> {
        match self.prompt {
            Prompt::None => None,
            Prompt::Phone => Some("手机号"),
            Prompt::Code => Some("验证码"),
            Prompt::Password => Some("两步验证密码"),
            Prompt::Email => Some("邮箱地址"),
            Prompt::EmailCode => Some("邮箱验证码"),
            Prompt::FirstName => Some("名字"),
            Prompt::LastName => Some("姓氏（可留空）"),
        }
    }

    #[cfg(test)]
    pub fn visible_input(&self) -> String {
        if self.prompt == Prompt::Password {
            "•".repeat(
                unicode_segmentation::UnicodeSegmentation::graphemes(self.input.as_str(), true)
                    .count(),
            )
        } else {
            self.input.clone()
        }
    }

    pub fn has_input(&self) -> bool {
        self.setup.is_some() || self.prompt != Prompt::None
    }

    pub fn clear_input(&mut self) {
        if let Some(form) = &mut self.setup {
            if !form.new_login {
                form.field_mut().clear();
            }
        } else {
            self.input.clear();
            self.cursor = usize::MAX;
        }
        self.setup_cursors = [usize::MAX; 3];
    }

    pub fn paste(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(4096)
            .collect();
        if let Some(form) = &mut self.setup {
            if form.new_login {
                return;
            }
            let index = form.focused;
            let value = if index == 2 {
                text.as_str()
            } else {
                text.trim()
            };
            crate::text::insert(
                form.field_mut(),
                &mut self.setup_cursors[index],
                value,
                4096,
            );
        } else if self.prompt == Prompt::Password {
            crate::text::insert(&mut self.input, &mut self.cursor, &text, 4096);
        } else {
            crate::text::insert(&mut self.input, &mut self.cursor, text.trim(), 4096);
        }
        self.clear_message();
    }

    pub fn key(&mut self, key: KeyCode) -> Option<Value> {
        if let Some(form) = &mut self.setup {
            match key {
                KeyCode::F(5) if form.recovering => self.choose_new_login(),
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                    if !form.recovering {
                        form.focused = 1 - form.focused;
                    }
                }
                KeyCode::Enter
                    if !form.recovering && form.focused == 0 && form.api_hash.is_empty() =>
                {
                    form.focused = 1
                }
                KeyCode::Enter => self.save_setup(),
                key if !form.new_login => {
                    let index = form.focused;
                    crate::text::edit(form.field_mut(), &mut self.setup_cursors[index], key);
                }
                _ => {}
            }
            return None;
        }
        match key {
            KeyCode::Enter
                if self.has_input()
                    && (!self.input.trim().is_empty() || self.prompt == Prompt::LastName) =>
            {
                let input = std::mem::take(&mut self.input);
                self.cursor = usize::MAX;
                self.is_error = false;
                self.message = "已提交，请稍候…".into();
                return Some(match self.prompt {
                    Prompt::Phone => {
                        json!({"@type": "setAuthenticationPhoneNumber", "phone_number": input.trim(), "settings": null})
                    }
                    Prompt::Code => {
                        json!({"@type": "checkAuthenticationCode", "code": input.trim()})
                    }
                    Prompt::Password => {
                        json!({"@type": "checkAuthenticationPassword", "password": input})
                    }
                    Prompt::Email => {
                        json!({"@type": "setAuthenticationEmailAddress", "email_address": input.trim()})
                    }
                    Prompt::EmailCode => {
                        json!({"@type": "checkAuthenticationEmailCode", "code": {"@type": "emailAddressAuthenticationCode", "code": input.trim()}})
                    }
                    Prompt::FirstName => {
                        self.first_name = input;
                        self.prompt = Prompt::LastName;
                        self.message.clear();
                        return None;
                    }
                    Prompt::LastName => {
                        json!({"@type": "registerUser", "first_name": self.first_name, "last_name": input, "disable_notification": false})
                    }
                    Prompt::None => unreachable!(),
                });
            }
            key if self.has_input() => {
                crate::text::edit(&mut self.input, &mut self.cursor, key);
            }
            _ => {}
        }
        None
    }

    pub fn input_text(&self) -> &str {
        &self.input
    }
    pub fn is_password(&self) -> bool {
        self.prompt == Prompt::Password
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> AuthFlow {
        AuthFlow::empty()
    }

    #[test]
    fn database_key_error_exposes_recovery_and_preserves_credentials() {
        let mut auth = flow();
        auth.begin_setup();
        let form = auth.setup.as_mut().unwrap();
        form.api_id = "123".into();
        form.api_hash = "0123456789abcdef0123456789abcdef".into();
        auth.on_update(&json!({"@type":"error","code":400,"message":"Wrong database encryption key","@extra":"set-tdlib-parameters"}));
        assert!(auth.is_recovering());
        assert_eq!(auth.setup.as_ref().unwrap().focused, 2);
        auth.paste(" exact key ");
        assert_eq!(auth.setup.as_ref().unwrap().database_key, " exact key ");
        auth.key(KeyCode::Tab);
        assert_eq!(auth.setup.as_ref().unwrap().focused, 2);
        auth.key(KeyCode::F(5));
        assert!(auth.is_new_login());
        assert!(!auth.take_restart());
        auth.paste("ignored");
        assert_eq!(auth.setup.as_ref().unwrap().database_key, " exact key ");
        assert!(auth.cancel_setup());
        assert!(auth.is_recovering() && !auth.is_new_login());
        assert_eq!(auth.setup.as_ref().unwrap().api_id, "123");
        assert_eq!(
            auth.setup.as_ref().unwrap().api_hash,
            "0123456789abcdef0123456789abcdef"
        );
        auth.on_update(
            &json!({"@type":"error","code":400,"message":"Wrong password","@extra":"database-key"}),
        );
        assert!(auth.is_recovering());
        assert!(auth.setup.as_ref().unwrap().database_key.is_empty());
        assert!(auth.message.contains("不匹配"));
    }

    #[test]
    fn login_requests_follow_authorization_state() {
        let mut auth = flow();
        auth.on_update(&json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": "authorizationStateWaitPhoneNumber"}}));
        auth.paste("+8613800000000");
        assert_eq!(
            auth.key(KeyCode::Enter),
            Some(
                json!({"@type": "setAuthenticationPhoneNumber", "phone_number": "+8613800000000", "settings": null})
            )
        );
        auth.on_update(&json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": "authorizationStateWaitPassword"}}));
        auth.paste("ab");
        assert_eq!(auth.visible_input(), "••");
        assert_eq!(
            auth.key(KeyCode::Enter),
            Some(json!({"@type": "checkAuthenticationPassword", "password": "ab"}))
        );
        assert!(auth.visible_input().is_empty());
    }

    #[test]
    fn outdated_tdlib_login_error_has_actionable_message() {
        let mut auth = flow();
        auth.on_update(&json!({"@type": "error", "code": 406, "message": "UPDATE_APP_TO_LOGIN"}));
        assert!(auth.message.contains("TDLib"));
        assert!(auth.message.contains("install-tdlib.sh"));
    }

    #[test]
    fn confirmation_link_and_password_hint_are_visible() {
        let mut auth = flow();
        auth.on_update(&json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": "authorizationStateWaitOtherDeviceConfirmation", "link": "tg://login?token=example"}}));
        assert_eq!(
            auth.confirmation_link.as_deref(),
            Some("tg://login?token=example")
        );
        auth.on_update(&json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": "authorizationStateWaitPassword", "password_hint": "提示"}}));
        assert!(auth.detail.contains("提示"));
        assert!(auth.confirmation_link.is_none());
    }

    #[test]
    fn authorization_updates_do_not_erase_setup_or_partial_login_input() {
        let mut auth = flow();
        auth.begin_setup();
        auth.paste("123");
        auth.on_update(&json!({"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateWaitTdlibParameters"}}));
        assert_eq!(auth.setup.as_ref().expect("form").api_id, "123");
        auth.setup = None;
        let phone = json!({"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateWaitPhoneNumber"}});
        auth.on_update(&phone);
        auth.paste("+86138");
        auth.on_update(&phone);
        assert_eq!(auth.visible_input(), "+86138");
    }
}
